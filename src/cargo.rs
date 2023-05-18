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
    // instead pass a `--target=<triple><api_level>` argument to clang via a
    // `RUSTC_WRAPPER` and for the cc crate via `CFLAGS_<triple>` and
    // `CXXFLAGS_<triple>`
    //
    // See: https://github.com/android/ndk/issues/1856
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

    // Set cargo-ndk itself as the rustc wrapper so we can add -Clink-arg=--target=<triple><api-level>
    // after all other rustflags have been resolved by Cargo
    //
    // Note: it's not possible to pass the linker argument via CARGO_ENCODED_RUSTFLAGS because that could
    // trample rustflags that are configured for the project and there's no practical way to read all
    // user-configured rustflags from outside of cargo itself.
    //
    let self_path = std::fs::canonicalize(std::env::args().next().unwrap())
        .expect("Failed to canonicalize absolute path to cargo-ndk");
    log::debug!("RUSTC_WRAPPER={self_path:?}");

    cargo_cmd.env("RUSTC_WRAPPER", &self_path);
    cargo_cmd.env("_CARGO_NDK_RUSTC_TARGET", &clang_target); // Recognised by main() so we know when we're acting as a wrapper

    // Make sure we daisy chain to any wrapper that is already configured by RUSTC_WRAPPER
    if let Ok(rustc_wrapper) = std::env::var("RUSTC_WRAPPER") {
        cargo_cmd.env("_CARGO_NDK_RUSTC_WRAPPER", rustc_wrapper);
    }

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
