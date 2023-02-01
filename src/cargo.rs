use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io::{Result, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use cargo_metadata::camino::Utf8PathBuf;
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

fn create_libgcc_linker_script_workaround(target_dir: &Utf8PathBuf) -> Result<Utf8PathBuf> {
    let libgcc_workaround_dir = target_dir.join("cargo-ndk").join("libgcc-workaround");
    std::fs::create_dir_all(&libgcc_workaround_dir)?;
    let libgcc_workaround_file = libgcc_workaround_dir.join("libgcc.a");
    let mut file = std::fs::File::create(libgcc_workaround_file)?;
    file.write_all(b"INPUT(-lunwind)")?;

    Ok(libgcc_workaround_dir)
}

enum RustFlags {
    Empty,
    Encoded(String),
    Plain(String),
}

impl RustFlags {
    fn from_env() -> Self {
        if let Ok(encoded) = std::env::var("CARGO_ENCODED_RUSTFLAGS") {
            return Self::Encoded(encoded);
        }

        if let Ok(plain) = std::env::var("RUSTFLAGS") {
            return Self::Plain(plain);
        }

        Self::Empty
    }

    fn append(&mut self, flag: &str) {
        match self {
            RustFlags::Empty => {
                *self = Self::Plain(flag.into());
            }
            RustFlags::Encoded(encoded) => {
                if !encoded.is_empty() {
                    encoded.push('\x1f');
                }
                encoded.push_str(flag);
            }
            RustFlags::Plain(plain) => {
                if !plain.is_empty() {
                    plain.push(' ');
                }
                plain.push_str(flag);
            }
        }
    }

    fn as_env_var(&self) -> Option<(&str, &str)> {
        Some(match self {
            RustFlags::Encoded(x) => ("CARGO_ENCODED_RUSTFLAGS", x),
            RustFlags::Plain(x) => ("RUSTFLAGS", x),
            RustFlags::Empty => return None,
        })
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run(
    dir: &Path,
    target_dir: &Utf8PathBuf,
    ndk_home: &Path,
    version: Version,
    triple: &str,
    platform: u8,
    cargo_args: &[String],
    cargo_manifest: &Path,
    bindgen: bool,
) -> std::process::ExitStatus {
    log::debug!("Detected NDK version: {:?}", &version);

    let target_linker = Path::new(&ndk_home).join(clang_suffix(triple, ARCH, platform, ""));
    let target_cxx = Path::new(&ndk_home).join(clang_suffix(triple, ARCH, platform, "++"));
    let target_sysroot = Path::new(&ndk_home).join(sysroot_suffix(ARCH));
    let target_ar = if version.major >= 23 {
        Path::new(&ndk_home).join(ndk23_tool(ARCH, "llvm-ar"))
    } else {
        Path::new(&ndk_home).join(toolchain_suffix(triple, ARCH, "ar"))
    };

    let cc_key = format!("CC_{}", &triple);
    let ar_key = format!("AR_{}", &triple);
    let cxx_key = format!("CXX_{}", &triple);
    let bindgen_clang_args_key = format!("BINDGEN_EXTRA_CLANG_ARGS_{}", &triple.replace('-', "_"));
    let cargo_bin = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());

    log::trace!(
        "Env: {:#?}",
        std::env::vars().into_iter().collect::<BTreeMap<_, _>>()
    );

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
        &std::env::var(bindgen_clang_args_key.clone()).unwrap_or_default()
    );
    log::debug!("Args: {:?}", &cargo_args);

    // Read initial RUSTFLAGS
    let mut rustflags = RustFlags::from_env();

    // Insert Cargo arguments before any `--` arguments.
    let arg_insertion_position = cargo_args
        .iter()
        .enumerate()
        .find(|e| e.1.trim() == "--")
        .map(|e| e.0)
        .unwrap_or(cargo_args.len());

    let mut cargo_args: Vec<OsString> = cargo_args.iter().map(|arg| arg.into()).collect();

    let mut cargo_cmd = Command::new(cargo_bin);
    cargo_cmd
        .current_dir(dir)
        .env(ar_key, &target_ar)
        .env(cc_key, &target_linker)
        .env(cxx_key, &target_cxx)
        .env(cargo_env_target_cfg(triple, "ar"), &target_ar)
        .env(cargo_env_target_cfg(triple, "linker"), &target_linker);

    // NDK releases >= 23 beta3 no longer include libgcc which rust's pre-built
    // standard libraries depend on. As a workaround for newer NDKs we redirect
    // libgcc to libunwind.
    //
    // Note: there is a tiny chance of a false positive here while the first two
    // beta releases for NDK v23 didn't yet include libunwind for all
    // architectures.
    //
    // Note: even though rust-lang merged a fix to support linking the standard
    // libraries against newer NDKs they still (up to 1.62.0 at time of writing)
    // choose to build binaries (distributed by rustup) against an older NDK
    // release (presumably aiming for broader compatibility) which means that
    // even the latest versions still require this workaround.
    //
    // Ref: https://github.com/rust-lang/rust/pull/85806
    if version.major >= 23 {
        match create_libgcc_linker_script_workaround(target_dir) {
            Ok(libdir) => {
                // Note that we don't use `cargo rustc` to pass custom library search paths to
                // rustc and instead use `CARGO_ENCODED_RUSTFLAGS` because it affects the building
                // of all transitive cdylibs (which all need this workaround).
                rustflags.append(&format!("-L{libdir}"));
                let (k, v) = rustflags.as_env_var().unwrap();
                cargo_cmd.env(k, v);
            }
            Err(e) => {
                log::error!("Failed to create libgcc.a linker script workaround");
                log::error!("{}", e);
                std::process::exit(1);
            }
        }
    }

    let extra_include = format!("{}/usr/include/{}", &target_sysroot.display(), triple);
    if bindgen {
        let bindgen_args = format!(
            "--sysroot={} -I{}",
            &target_sysroot.display(),
            extra_include
        );
        cargo_cmd.env(
            bindgen_clang_args_key,
            bindgen_args.clone().replace('\\', "/"),
        );
        log::debug!("bindgen_args={}", bindgen_args);
    }

    match dir.parent() {
        Some(parent) => {
            if parent != dir {
                log::debug!("Working directory does not match manifest-path");
                cargo_args.insert(
                    arg_insertion_position,
                    cargo_manifest.as_os_str().to_owned(),
                );
                cargo_args.insert(
                    arg_insertion_position,
                    OsStr::new("--manifest-path").to_owned(),
                );
            }
        }
        _ => {
            log::warn!("Parent of current working directory does not exist");
        }
    }

    cargo_args.insert(arg_insertion_position, OsStr::new(triple).to_owned());
    cargo_args.insert(arg_insertion_position, OsStr::new("--target").to_owned());

    cargo_cmd.args(cargo_args).status().expect("cargo crashed")
}

pub(crate) fn strip(
    ndk_home: &Path,
    triple: &str,
    bin_path: &Path,
    version: Version,
) -> std::process::ExitStatus {
    let target_strip = if version.major >= 23 {
        Path::new(&ndk_home).join(ndk23_tool(ARCH, "llvm-strip"))
    } else {
        Path::new(&ndk_home).join(toolchain_suffix(triple, ARCH, "strip"))
    };

    log::debug!("strip: {}", &target_strip.display());

    Command::new(target_strip)
        .arg(bin_path)
        .status()
        .expect("strip crashed")
}
