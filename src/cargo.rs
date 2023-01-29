use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
};

use cargo_metadata::semver::Version;

#[cfg(target_os = "macos")]
const ARCH: &str = "darwin-x86_64";
#[cfg(target_os = "linux")]
const ARCH: &str = "linux-x86_64";
#[cfg(target_os = "windows")]
const ARCH: &str = "windows-x86_64";

#[cfg(target_os = "windows")]
const CLANG_EXT: &str = ".cmd";
#[cfg(not(target_os = "windows"))]
const CLANG_EXT: &str = "";

fn clang_suffix(triple: &str, arch: &str, platform: u8, postfix: &str) -> PathBuf {
    let tool_triple = match triple {
        "arm-linux-androideabi" => "armv7a-linux-androideabi",
        "armv7-linux-androideabi" => "armv7a-linux-androideabi",
        _ => triple,
    };

    [
        "toolchains",
        "llvm",
        "prebuilt",
        arch,
        "bin",
        &format!("{tool_triple}{platform}-clang{postfix}{CLANG_EXT}"),
    ]
    .iter()
    .collect()
}

fn ndk23_tool(arch: &str, tool: &str) -> PathBuf {
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
) -> std::process::ExitStatus {
    log::debug!("Detected NDK version: {:?}", &version);

    if version.major < 23 {
        log::error!("NDK versions less than r23 are not supported. Install an up-to-date version of the NDK.");
        std::process::exit(1);
    }

    let target_linker = ndk_home.join(clang_suffix(triple, ARCH, platform, ""));
    let target_cxx = ndk_home.join(clang_suffix(triple, ARCH, platform, "++"));
    let target_sysroot = ndk_home.join(sysroot_suffix(ARCH));
    let target_ar = ndk_home.join(ndk23_tool(ARCH, "llvm-ar"));

    let cc_key = format!("CC_{}", &triple);
    let ar_key = format!("AR_{}", &triple);
    let cxx_key = format!("CXX_{}", &triple);
    let bindgen_clang_args_key = format!("BINDGEN_EXTRA_CLANG_ARGS_{}", &triple.replace('-', "_"));
    let cargo_bin = env::var("CARGO").unwrap_or_else(|_| "cargo".into());

    log::debug!("cargo: {}", &cargo_bin);
    log::debug!("{}={}", &ar_key, &target_ar.display());
    log::debug!("{}={}", &cc_key, &target_linker.display());
    log::debug!("{}={}", &cxx_key, &target_cxx.display());
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
        .env(ar_key, &target_ar)
        .env(cc_key, &target_linker)
        .env(cxx_key, &target_cxx)
        .env(cargo_env_target_cfg(triple, "ar"), &target_ar)
        .env(cargo_env_target_cfg(triple, "linker"), &target_linker);

    let extra_include = format!("{}/usr/include/{}", &target_sysroot.display(), triple);
    if bindgen {
        let bindgen_args = format!(
            "--sysroot={} -I{}",
            &target_sysroot.display(),
            extra_include
        );
        cargo_cmd.env(bindgen_clang_args_key, bindgen_args.clone());
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
    let target_strip = ndk_home.join(ndk23_tool(ARCH, "llvm-strip"));

    log::debug!("strip: {}", &target_strip.display());

    Command::new(target_strip)
        .arg(bin_path)
        .status()
        .expect("strip crashed")
}
