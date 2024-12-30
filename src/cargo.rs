use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    io::BufReader,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, Result};
use cargo_metadata::{camino::Utf8PathBuf, semver::Version, Artifact, Message};

use crate::shell::Shell;

#[cfg(target_os = "macos")]
const ARCH: &str = "darwin-x86_64";
#[cfg(target_os = "linux")]
const ARCH: &str = "linux-x86_64";
#[cfg(target_os = "windows")]
const ARCH: &str = "windows-x86_64";

#[cfg(target_os = "android")]
compile_error!(
    "You cannot build cargo-ndk _for_ Android. Build it for your host OS and run it with cargo."
);

#[cfg(not(any(
    target_os = "android",
    target_os = "macos",
    target_os = "linux",
    target_os = "windows"
)))]
compile_error!("Unsupported target OS");
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
const ARCH: &str = "unknown";

pub(crate) fn clang_target(rust_target: &str, api_level: u8) -> String {
    let target = match rust_target {
        "arm-linux-androideabi" => "armv7a-linux-androideabi",
        "armv7-linux-androideabi" => "armv7a-linux-androideabi",
        _ => rust_target,
    };
    format!("--target={target}{api_level}")
}

fn sysroot_target(rust_target: &str) -> &str {
    (match rust_target {
        "armv7-linux-androideabi" => "arm-linux-androideabi",
        _ => rust_target,
    }) as _
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

#[inline]
fn env_var_with_key(key: String) -> Option<(String, String)> {
    env::var(&key).map(|value| (key, value)).ok()
}

// Derived from getenv_with_target_prefixes in `cc` crate.
fn cc_env(var_base: &str, triple: &str) -> (String, Option<String>) {
    let triple_u = triple.replace('-', "_");
    let most_specific_key = format!("{}_{}", var_base, triple);

    env_var_with_key(most_specific_key.to_string())
        .or_else(|| env_var_with_key(format!("{}_{}", var_base, triple_u)))
        .or_else(|| env_var_with_key(format!("TARGET_{}", var_base)))
        .or_else(|| env_var_with_key(var_base.to_string()))
        .map(|(key, value)| (key, Some(value)))
        .unwrap_or_else(|| (most_specific_key, None))
}

pub(crate) fn build_env(
    triple: &str,
    ndk_home: &Path,
    clang_target: &str,
    bindgen: bool,
) -> BTreeMap<String, OsString> {
    let self_path = std::fs::canonicalize(env::args().next().unwrap())
        .expect("Failed to canonicalize absolute path to cargo-ndk")
        .parent()
        .unwrap()
        .join("cargo-ndk");

    // Environment variables for the `cc` crate
    let (cc_key, _cc_value) = cc_env("CC", triple);
    let (cflags_key, cflags_value) = cc_env("CFLAGS", triple);
    let (cxx_key, _cxx_value) = cc_env("CXX", triple);
    let (cxxflags_key, cxxflags_value) = cc_env("CXXFLAGS", triple);
    let (ar_key, _ar_value) = cc_env("AR", triple);
    let (ranlib_key, _ranlib_value) = cc_env("RANLIB", triple);

    // Environment variables for cargo
    let cargo_ar_key = cargo_env_target_cfg(triple, "ar");
    let cargo_linker_key = cargo_env_target_cfg(triple, "linker");
    let bindgen_clang_args_key = format!("BINDGEN_EXTRA_CLANG_ARGS_{}", &triple.replace('-', "_"));
    
    let target_cc = ndk_home.join(ndk_tool(ARCH, "clang"));
    let target_cflags = match cflags_value {
        Some(v) => format!("{clang_target} {v}"),
        None => clang_target.to_string(),
    };
    let target_cxx = ndk_home.join(ndk_tool(ARCH, "clang++"));
    let target_cxxflags = match cxxflags_value {
        Some(v) => format!("{clang_target} {v}"),
        None => clang_target.to_string(),
    };
    let cargo_ndk_sysroot_path_key = "CARGO_NDK_SYSROOT_PATH";
    let cargo_ndk_sysroot_path = ndk_home.join(sysroot_suffix(ARCH));
    let cargo_ndk_sysroot_target_key = "CARGO_NDK_SYSROOT_TARGET";
    let cargo_ndk_sysroot_target = sysroot_target(triple);
    let cargo_ndk_sysroot_libs_path_key = "CARGO_NDK_SYSROOT_LIBS_PATH";
    let cargo_ndk_sysroot_libs_path = cargo_ndk_sysroot_path
        .join("usr")
        .join("lib")
        .join(cargo_ndk_sysroot_target);
    let target_ar = ndk_home.join(ndk_tool(ARCH, "llvm-ar"));
    let target_ranlib = ndk_home.join(ndk_tool(ARCH, "llvm-ranlib"));
    let target_linker = self_path;

    let extra_include = format!(
        "{}/usr/include/{}",
        &cargo_ndk_sysroot_path.display(),
        &cargo_ndk_sysroot_target
    );

    let mut envs = [
        (cc_key, target_cc.clone().into_os_string()),
        (cflags_key, target_cflags.into()),
        (cxx_key, target_cxx.into_os_string()),
        (cxxflags_key, target_cxxflags.into()),
        (ar_key, target_ar.clone().into()),
        (ranlib_key, target_ranlib.into_os_string()),
        (cargo_ar_key, target_ar.into_os_string()),
        (cargo_linker_key, target_linker.into_os_string()),
        (
            cargo_ndk_sysroot_path_key.to_string(),
            cargo_ndk_sysroot_path.clone().into_os_string(),
        ),
        (
            cargo_ndk_sysroot_libs_path_key.to_string(),
            cargo_ndk_sysroot_libs_path.into_os_string(),
        ),
        (
            cargo_ndk_sysroot_target_key.to_string(),
            cargo_ndk_sysroot_target.into(),
        ),
        // Found this through a comment related to bindgen using the wrong clang for cross compiles
        //
        // https://github.com/rust-lang/rust-bindgen/issues/2962#issuecomment-2438297124
        //
        // https://github.com/KyleMayes/clang-sys?tab=readme-ov-file#environment-variables
        ("CLANG_PATH".into(), target_cc.with_extension("exe").into()),

        ("_CARGO_NDK_LINK_TARGET".into(), clang_target.into()), // Recognized by main() so we know when we're acting as a wrapper
        ("_CARGO_NDK_LINK_CLANG".into(), target_cc.into()),
    ]
    .into_iter()
    .collect::<BTreeMap<String, OsString>>();

    if env::var("MSYSTEM").is_ok() || env::var("CYGWIN").is_ok() {
        envs = envs
            .into_iter()
            .map(|(k, v)| {
                (
                    k,
                    OsString::from(v.into_string().unwrap().replace('\\', "/")),
                )
            })
            .collect();
    }

    if bindgen {
        let bindgen_args = format!(
            "--sysroot={} -I{}",
            &cargo_ndk_sysroot_path.display(),
            extra_include
        );
        let bindgen_clang_args = bindgen_args.replace('\\', "/");
        // log::debug!("{bindgen_clang_args_key}={bindgen_clang_args:?}");
        envs.insert(
            bindgen_clang_args_key.to_string(),
            bindgen_clang_args.into(),
        );
    }

    envs
}

/// Note: considering that there is an upstream quoting bug in the clang .cmd
/// wrappers on Windows we intentionally avoid any wrapper scripts and
/// instead pass a `--target=<triple><api_level>` argument to clang by using
/// cargo-ndk itself as a linker wrapper.
///
/// See: https://github.com/android/ndk/issues/1856
///
/// Note: it's not possible to pass `-Clink-arg=` arguments via
/// CARGO_ENCODED_RUSTFLAGS because that could trample rustflags that are
/// configured for the project and there's no practical way to read all
/// user-configured rustflags from outside of cargo itself.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run(
    shell: &mut Shell,
    dir: &Path,
    ndk_home: &Path,
    version: &Version,
    triple: &str,
    platform: u8,
    cargo_args: &[String],
    cargo_manifest: &Path,
    bindgen: bool,
    #[allow(unused_variables)] out_dir: &Utf8PathBuf,
) -> Result<(std::process::ExitStatus, Vec<Artifact>)> {
    if version.major < 23 {
        shell.error("NDK versions less than r23 are not supported. Install an up-to-date version of the NDK.").unwrap();
        std::process::exit(1);
    }

    // Insert Cargo arguments before any `--` arguments.
    let arg_insertion_position = cargo_args
        .iter()
        .enumerate()
        .find(|e| e.1.trim() == "--")
        .map_or(cargo_args.len(), |e| e.0);

    let mut cargo_args: Vec<OsString> = cargo_args.iter().map(Into::into).collect();

    let clang_target = clang_target(triple, platform);
    let cargo_bin = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut cargo_cmd = Command::new(&cargo_bin);
    let envs = build_env(triple, ndk_home, &clang_target, bindgen);

    shell
        .very_verbose(|shell| {
            for (k, v) in envs.iter() {
                shell.status_with_color(
                    "Exporting",
                    format!("{k}={v:?}"),
                    termcolor::Color::Cyan,
                )?;
            }

            shell.status_with_color(
                "Invoking",
                format!("cargo ({cargo_bin}) with args: {cargo_args:?}"),
                termcolor::Color::Cyan,
            )
        })
        .unwrap();

    cargo_cmd.current_dir(dir).envs(envs);

    match dir.parent() {
        Some(parent) => {
            if parent != dir {
                // log::debug!("Working directory does not match manifest-path");
                cargo_args.insert(arg_insertion_position, cargo_manifest.into());
                cargo_args.insert(arg_insertion_position, "--manifest-path".into());
            }
        }
        _ => {
            // log::warn!("Parent of current working directory does not exist");
        }
    }

    cargo_args.insert(arg_insertion_position, triple.into());
    cargo_args.insert(arg_insertion_position, "--target".into());

    cargo_args.insert(arg_insertion_position, "json-render-diagnostics".into());
    cargo_args.insert(arg_insertion_position, "--message-format".into());

    let mut child = cargo_cmd
        .args(cargo_args)
        .stdin(Stdio::inherit())
        .stderr(Stdio::inherit())
        .stdout(Stdio::piped())
        .spawn()
        .context("failed spawning cargo process")?;

    let reader = BufReader::new(child.stdout.take().context("no stdout available")?);
    let mut artifacts = Vec::new();

    for msg in Message::parse_stream(reader) {
        match msg? {
            Message::CompilerArtifact(artifact) => artifacts.push(artifact),
            Message::CompilerMessage(msg) => println!("{msg}"),
            Message::TextLine(line) => println!("{line}"),
            _ => {}
        }
    }

    let status = child.wait().context("cargo crashed")?;

    Ok((status, artifacts))
}

pub(crate) fn strip(ndk_home: &Path, bin_path: &Path) -> std::process::ExitStatus {
    let target_strip = ndk_home.join(ndk_tool(ARCH, "llvm-strip"));

    // log::debug!("strip: {}", &target_strip.display());

    Command::new(target_strip)
        .arg(bin_path)
        .status()
        .expect("strip crashed")
}
