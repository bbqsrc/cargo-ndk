use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
};

use cargo_metadata::{camino::Utf8PathBuf, semver::Version};

#[cfg(target_os = "macos")]
const ARCH: &str = "darwin-x86_64";
#[cfg(target_os = "linux")]
const ARCH: &str = "linux-x86_64";
#[cfg(target_os = "windows")]
const ARCH: &str = "windows-x86_64";

/// Separator for CARGO_ENCODED_RUSTFLAGS
const FLAGS_SEP: &str = "\x1f";

fn cargo_target_dir(out_dir: &Utf8PathBuf) -> PathBuf {
    std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| out_dir.clone().into_std_path_buf())
}

/// On a "best effort" basis, try and load any rustflags that have been configured
/// via Cargo's config files.
///
/// It's not feasible to try and support target.<cfg>.rustflags from outside Cargo
/// and so we exit with an error if we find any, to avoid the risk that we
/// discard configured rustflags
fn read_cargo_config_rustflags(out_dir: &Utf8PathBuf, target: &str) -> String {
    let mut cargo_config = cargo::util::config::Config::default().unwrap();
    let target_dir = cargo_target_dir(out_dir);
    if let Err(err) = cargo_config.configure(
        0,
        true,
        None,
        false,
        false,
        true,
        &Some(target_dir),
        &[],
        &[],
    ) {
        log::error!("Failed to load Cargo config: {err:?}");
        std::process::exit(1);
    }

    // check for any target.<cfg>.rustflags
    let target_cfg_rustflags = cargo_config.target_cfgs().unwrap();
    if !target_cfg_rustflags.is_empty() {
        log::error!("target.<cfg>.rustflags aren't supported because it's not possible to resolve these outside of cargo");
        std::process::exit(1);
    }

    // target.rustflags
    if let Ok(target_rustflags) = cargo_config.target_cfg_triple(target) {
        if let Some(rustflags) = target_rustflags.rustflags {
            return rustflags.val.as_slice().join(FLAGS_SEP);
        }
    }

    // build.rustflags
    if let Ok(build_rustflags) = &cargo_config.build_config() {
        if let Some(rustflags) = &build_rustflags.rustflags {
            return rustflags.as_slice().join(FLAGS_SEP);
        }
    }

    String::new()
}

// This is a bit of a nightmare to deal with outside of cargo itself
//
// There are four mutually exclusive sources of extra flags. They are checked in order, with the first one being used:
//
// 1. CARGO_ENCODED_RUSTFLAGS environment variable.
// 2. RUSTFLAGS environment variable.
// 3. All matching target.<triple>.rustflags and target.<cfg>.rustflags config entries joined together.
// 4. build.rustflags config value.
//
// It's also worth noting that it's possible to affect these via `--config` command line arguments
// but we're not making any attempt to handle these currently.
//
// Most of this is borrowed from `ndk-build`, https://github.com/rust-mobile/cargo-apk/commit/170b4df5af7ab15d778c7725989fe8a2eea639e1
fn read_cargo_rustflags(out_dir: &Utf8PathBuf, target: &str) -> String {
    // Read initial CARGO_ENCODED_/RUSTFLAGS
    match std::env::var("CARGO_ENCODED_RUSTFLAGS") {
        Ok(val) => {
            if std::env::var_os("RUSTFLAGS").is_some() {
                log::error!(
                    "Both `CARGO_ENCODED_RUSTFLAGS` and `RUSTFLAGS` were found in the environment, please clear one or the other"
                );
                std::process::exit(1);
            }

            val
        }
        Err(std::env::VarError::NotPresent) => {
            match std::env::var("RUSTFLAGS") {
                Ok(val) => {
                    // Same as cargo
                    // https://github.com/rust-lang/cargo/blob/f6de921a5d807746e972d9d10a4d8e1ca21e1b1f/src/cargo/core/compiler/build_context/target_info.rs#L682-L690
                    val.split(' ')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join(FLAGS_SEP)
                }
                Err(std::env::VarError::NotPresent) => read_cargo_config_rustflags(out_dir, target),
                Err(std::env::VarError::NotUnicode(_)) => {
                    log::error!("RUSTFLAGS environment variable contains non-unicode characters");
                    std::process::exit(1);
                }
            }
        }
        Err(std::env::VarError::NotUnicode(_)) => {
            log::error!(
                "CARGO_ENCODED_RUSTFLAGS environment variable contains non-unicode characters"
            );
            std::process::exit(1);
        }
    }
}

fn clang_target(rust_target: &str, api_level: u8) -> String {
    let target = match rust_target {
        "arm-linux-androideabi" => "armv7a-linux-androideabi",
        "armv7-linux-androideabi" => "armv7a-linux-androideabi",
        _ => rust_target,
    };
    format!("--target={target}{api_level}")
}

fn ndk_tool(arch: &str, tool: &str) -> PathBuf {
    ["toolchains", "llvm", "prebuilt", arch, "bin", tool]
        .iter()
        .collect()
}

fn sysroot_suffix(arch: &str) -> PathBuf {
    ["toolchains", "llvm", "prebuilt", arch, "sysroot"]
        .iter()
        .collect()
}

fn cargo_env_target_cfg(triple: &str, key: &str) -> String {
    format!("CARGO_TARGET_{}_{}", &triple.replace('-', "_"), key).to_uppercase()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run(
    dir: &Path,
    ndk_home: &Path,
    version: &Version,
    triple: &str,
    platform: u8,
    cargo_args: &[String],
    cargo_manifest: &Path,
    bindgen: bool,
    #[allow(unused_variables)] out_dir: &Utf8PathBuf,
) -> std::process::ExitStatus {
    log::debug!("Detected NDK version: {:?}", &version);

    if version.major < 23 {
        log::error!("NDK versions less than r23 are not supported. Install an up-to-date version of the NDK.");
        std::process::exit(1);
    }

    let clang_target = clang_target(triple, platform);

    // Note: considering that there is an upstream quoting bug in the clang .cmd
    // wrappers on Windows, and considering that there is also a performance
    // overhead (especially on Windows) from using the wrapper scripts we
    // intentionally avoid them and instead pass a
    // `--target=<triple><api_level>` to clang via `CARGO_ENCODED_RUSTFLAGS` and
    // for the cc crate via `CFLAGS_<triple>` and `CXXFLAGS_<triple>`
    //
    // See: https://github.com/android/ndk/issues/1856
    //
    // For reference, this is also the same approach taken in the ndk-build crate.
    //
    let target_linker = ndk_home.join(ndk_tool(ARCH, "clang"));
    let target_cflags = clang_target.clone();
    let target_cxx = ndk_home.join(ndk_tool(ARCH, "clang++"));
    let target_cxxflags = clang_target.clone();
    let target_sysroot = ndk_home.join(sysroot_suffix(ARCH));
    let target_ar = ndk_home.join(ndk_tool(ARCH, "llvm-ar"));
    let target_ranlib = ndk_home.join(ndk_tool(ARCH, "llvm-ranlib"));

    let cc_key = format!("CC_{}", &triple);
    let cflags_key = format!("CFLAGS_{}", &triple);
    let ar_key = format!("AR_{}", &triple);
    let cxx_key = format!("CXX_{}", &triple);
    let cxxflags_key = format!("CXXFLAGS_{}", &triple);
    let ranlib_key = format!("RANLIB_{}", &triple);
    let bindgen_clang_args_key = format!("BINDGEN_EXTRA_CLANG_ARGS_{}", &triple.replace('-', "_"));
    let cargo_bin = env::var("CARGO").unwrap_or_else(|_| "cargo".into());

    log::debug!("cargo: {}", &cargo_bin);
    log::debug!("{}={}", &ar_key, &target_ar.display());
    log::debug!("{}={}", &cc_key, &target_linker.display());
    log::debug!("{}={}", &cflags_key, &target_cflags);
    log::debug!("{}={}", &cxx_key, &target_cxx.display());
    log::debug!("{}={}", &cxxflags_key, &target_cxxflags);
    log::debug!("{}={}", &ranlib_key, &target_ranlib.display());
    log::debug!(
        "{}={}",
        cargo_env_target_cfg(triple, "ar"),
        &target_ar.display()
    );
    log::debug!(
        "{}={}",
        cargo_env_target_cfg(triple, "linker"),
        &target_linker.display()
    );
    log::debug!(
        "{}={}",
        &bindgen_clang_args_key,
        &std::env::var(&bindgen_clang_args_key).unwrap_or_default()
    );
    log::debug!("Args: {:?}", &cargo_args);

    // Insert Cargo arguments before any `--` arguments.
    let arg_insertion_position = cargo_args
        .iter()
        .enumerate()
        .find(|e| e.1.trim() == "--")
        .map_or(cargo_args.len(), |e| e.0);

    let mut cargo_args: Vec<OsString> = cargo_args.iter().map(Into::into).collect();

    let mut cargo_cmd = Command::new(cargo_bin);

    cargo_cmd
        .current_dir(dir)
        .env(&ar_key, &target_ar)
        .env(&cc_key, &target_linker)
        .env(&cxx_key, &target_cxx)
        .env(&ranlib_key, &target_ranlib)
        .env(cargo_env_target_cfg(triple, "ar"), &target_ar)
        .env(cargo_env_target_cfg(triple, "linker"), &target_linker);

    let mut rustflags = read_cargo_rustflags(out_dir, triple);
    if !rustflags.is_empty() {
        // Avoid creating an empty '' rustc argument
        rustflags.push_str(FLAGS_SEP);
    }
    rustflags.push_str("-Clink-arg=");
    rustflags.push_str(&clang_target);
    cargo_cmd.env("CARGO_ENCODED_RUSTFLAGS", &rustflags);
    cargo_cmd.env_remove("RUSTFLAGS"); // any RUSTFLAGS were promoted to _ENCODED flags
    log::debug!(
        "CARGO_ENCODED_RUSTFLAGS={}",
        rustflags.replace(FLAGS_SEP, ",")
    );

    let extra_include = format!("{}/usr/include/{}", &target_sysroot.display(), triple);
    if bindgen {
        let bindgen_args = format!(
            "--sysroot={} -I{}",
            &target_sysroot.display(),
            extra_include
        );
        cargo_cmd.env(bindgen_clang_args_key, bindgen_args.replace('\\', "/"));
        log::debug!("bindgen_args={}", bindgen_args);
    }

    match dir.parent() {
        Some(parent) => {
            if parent != dir {
                log::debug!("Working directory does not match manifest-path");
                cargo_args.insert(arg_insertion_position, cargo_manifest.into());
                cargo_args.insert(arg_insertion_position, "--manifest-path".into());
            }
        }
        _ => {
            log::warn!("Parent of current working directory does not exist");
        }
    }

    cargo_args.insert(arg_insertion_position, triple.into());
    cargo_args.insert(arg_insertion_position, "--target".into());

    cargo_cmd.args(cargo_args).status().expect("cargo crashed")
}

pub(crate) fn strip(ndk_home: &Path, bin_path: &Path) -> std::process::ExitStatus {
    let target_strip = ndk_home.join(ndk_tool(ARCH, "llvm-strip"));

    log::debug!("strip: {}", &target_strip.display());

    Command::new(target_strip)
        .arg(bin_path)
        .status()
        .expect("strip crashed")
}
