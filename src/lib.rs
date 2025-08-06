use std::path::PathBuf;

pub mod cargo;
pub mod cli;
pub mod meta;
pub mod shell;

#[cfg(target_os = "macos")]
pub(crate) const ARCH: &str = "darwin-x86_64";
#[cfg(any(target_os = "linux", target_os = "android"))]
pub(crate) const ARCH: &str = "linux-x86_64";
#[cfg(target_os = "windows")]
pub(crate) const ARCH: &str = "windows-x86_64";

#[cfg(all(target_os = "android", not(cargo_ndk_on_android)))]
compile_error!(
    r#"
Building cargo-ndk on Android is not supported. This binary is intended to be run on your host OS.

Set CARGO_NDK_ON_ANDROID to override this check (for example, building for Termux)."
"#
);

#[cfg(not(any(
    target_os = "android",
    target_os = "macos",
    target_os = "linux",
    target_os = "windows"
)))]
compile_error!("Unsupported target OS");

pub(crate) fn clang_target(rust_target: &str, api_level: u8) -> String {
    let target = match rust_target {
        "arm-linux-androideabi" => "armv7a-linux-androideabi",
        "armv7-linux-androideabi" => "armv7a-linux-androideabi",
        _ => rust_target,
    };
    format!("--target={target}{api_level}")
}

pub(crate) fn sysroot_target(rust_target: &str) -> &str {
    (match rust_target {
        "armv7-linux-androideabi" => "arm-linux-androideabi",
        _ => rust_target,
    }) as _
}

pub(crate) fn ndk_tool(arch: &str, tool: &str) -> PathBuf {
    ["toolchains", "llvm", "prebuilt", arch, "bin", tool]
        .iter()
        .collect()
}

pub(crate) fn sysroot_suffix(arch: &str) -> PathBuf {
    ["toolchains", "llvm", "prebuilt", arch, "sysroot"]
        .iter()
        .collect()
}
