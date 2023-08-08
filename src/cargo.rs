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
    // wrappers on Windows we intentionally avoid any wrapper scripts and
    // instead pass a `--target=<triple><api_level>` argument to clang by using
    // cargo-ndk itself as a linker wrapper.
    //
    // See: https://github.com/android/ndk/issues/1856
    //
    // Note: it's not possible to pass `-Clink-arg=` arguments via
    // CARGO_ENCODED_RUSTFLAGS because that could trample rustflags that are
    // configured for the project and there's no practical way to read all
    // user-configured rustflags from outside of cargo itself.
    //
    let self_path = std::fs::canonicalize(std::env::args().next().unwrap())
        .expect("Failed to canonicalize absolute path to cargo-ndk");

    // Environment variables for the `cc` crate
    let cc_key = format!("CC_{}", &triple);
    let cflags_key = format!("CFLAGS_{}", &triple);
    let cxx_key = format!("CXX_{}", &triple);
    let cxxflags_key = format!("CXXFLAGS_{}", &triple);
    let ar_key = format!("AR_{}", &triple);
    let ranlib_key = format!("RANLIB_{}", &triple);

    // Environment variables for cargo
    let cargo_ar_key = cargo_env_target_cfg(triple, "ar");
    let cargo_linker_key = cargo_env_target_cfg(triple, "linker");
    let bindgen_clang_args_key = format!("BINDGEN_EXTRA_CLANG_ARGS_{}", &triple.replace('-', "_"));

    let cargo_bin = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let target_cc = ndk_home.join(ndk_tool(ARCH, "clang"));
    let target_cflags = clang_target.clone();
    let target_cxx = ndk_home.join(ndk_tool(ARCH, "clang++"));
    let target_cxxflags = clang_target.clone();
    let target_sysroot = ndk_home.join(sysroot_suffix(ARCH));
    let target_ar = ndk_home.join(ndk_tool(ARCH, "llvm-ar"));
    let target_ranlib = ndk_home.join(ndk_tool(ARCH, "llvm-ranlib"));
    let target_linker = self_path;

    log::debug!("{cc_key}={target_cc:?}");
    log::debug!("{cflags_key}={target_cflags}");
    log::debug!("{cxx_key}={target_cxx:?}");
    log::debug!("{cxxflags_key}={target_cxxflags}");
    log::debug!("{ar_key}={target_ar:?}");
    log::debug!("{ranlib_key}={target_ranlib:?}");

    log::debug!("cargo: {cargo_bin}");
    log::debug!("{cargo_ar_key}={target_ar:?}");
    log::debug!("{cargo_linker_key}={target_linker:?}");
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
        .env(&cc_key, &target_cc)
        .env(&cflags_key, &target_cflags)
        .env(&cxx_key, &target_cxx)
        .env(&cxxflags_key, &target_cxxflags)
        .env(&ar_key, &target_ar)
        .env(&ranlib_key, &target_ranlib)
        .env(cargo_ar_key, &target_ar)
        .env(cargo_linker_key, &target_linker)
        .env("_CARGO_NDK_LINK_TARGET", &clang_target) // Recognized by main() so we know when we're acting as a wrapper
        .env("_CARGO_NDK_LINK_CLANG", &target_cc);

    let extra_include = format!("{}/usr/include/{}", &target_sysroot.display(), triple);
    if bindgen {
        let bindgen_args = format!(
            "--sysroot={} -I{}",
            &target_sysroot.display(),
            extra_include
        );
        let bindgen_clang_args = bindgen_args.replace('\\', "/");
        log::debug!("{bindgen_clang_args_key}={bindgen_clang_args:?}");
        cargo_cmd.env(bindgen_clang_args_key, bindgen_clang_args);
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
