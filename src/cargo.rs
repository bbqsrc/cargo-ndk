use std::path::{Path, PathBuf};
use std::process::Command;

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

#[cfg(target_os = "windows")]
const BIN_EXT: &str = ".exe";
#[cfg(not(target_os = "windows"))]
const BIN_EXT: &str = "";

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
        &format!("{}{}-clang{}{}", tool_triple, platform, postfix, CLANG_EXT),
    ]
    .iter()
    .collect()
}

fn toolchain_triple(triple: &str) -> &str {
    match triple {
        "armv7-linux-androideabi" => "arm-linux-androideabi",
        _ => triple,
    }
}

fn toolchain_suffix(triple: &str, arch: &str, bin: &str) -> PathBuf {
    [
        "toolchains",
        "llvm",
        "prebuilt",
        arch,
        "bin",
        &format!("{}-{}{}", toolchain_triple(triple), bin, BIN_EXT),
    ]
    .iter()
    .collect()
}

fn cargo_env_target_cfg(triple: &str, key: &str) -> String {
    format!("CARGO_TARGET_{}_{}", &triple.replace("-", "_"), key).to_uppercase()
}

pub(crate) fn run(
    dir: &Path,
    ndk_home: &Path,
    triple: &str,
    platform: u8,
    cargo_args: &[String],
    cargo_manifest: &Path,
) -> std::process::ExitStatus {
    let target_ar = Path::new(&ndk_home).join(toolchain_suffix(triple, ARCH, "ar"));
    let target_linker = Path::new(&ndk_home).join(clang_suffix(triple, ARCH, platform, ""));
    let target_cxx = Path::new(&ndk_home).join(clang_suffix(triple, ARCH, platform, "++"));

    let cc_key = format!("CC_{}", &triple);
    let ar_key = format!("AR_{}", &triple);
    let cxx_key = format!("CXX_{}", &triple);
    let cargo_bin = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());

    log::debug!("ar: {}", &target_ar.display());
    log::debug!("linker: {}", &target_linker.display());
    log::debug!("cargo: {}", &cargo_bin);

    let mut cargo_cmd = Command::new(cargo_bin);
    cargo_cmd
        .current_dir(dir)
        .env(ar_key, &target_ar)
        .env(cc_key, &target_linker)
        .env(cxx_key, &target_cxx)
        .env(cargo_env_target_cfg(triple, "ar"), &target_ar)
        .env(cargo_env_target_cfg(triple, "linker"), &target_linker)
        .args(cargo_args);

    match dir.parent() {
        Some(parent) => {
            if parent != dir {
                log::debug!("Working directory does not match manifest-path");
                cargo_cmd.arg("--manifest-path").arg(&cargo_manifest);
            }
        }
        _ => {
            log::warn!("Parent of current working directory does not exist");
        }
    }

    cargo_cmd
        .arg("--target")
        .arg(&triple)
        .status()
        .expect("cargo crashed")
}

pub(crate) fn strip(ndk_home: &Path, triple: &str, bin_path: &Path) -> std::process::ExitStatus {
    let target_strip = Path::new(&ndk_home).join(toolchain_suffix(triple, ARCH, "strip"));

    log::debug!("strip: {}", &target_strip.display());

    Command::new(target_strip)
        .arg(&bin_path)
        .status()
        .expect("strip crashed")
}
