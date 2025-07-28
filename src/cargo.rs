use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    io::BufReader,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, Result};
use cargo_metadata::{Artifact, Message, semver::Version};

use crate::{ARCH, shell::Shell};

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
    let most_specific_key = format!("{var_base}_{triple}");

    env_var_with_key(most_specific_key.to_string())
        .or_else(|| env_var_with_key(format!("{var_base}_{triple_u}")))
        .or_else(|| env_var_with_key(format!("TARGET_{var_base}")))
        .or_else(|| env_var_with_key(var_base.to_string()))
        .map(|(key, value)| (key, Some(value)))
        .unwrap_or_else(|| (most_specific_key, None))
}

// {}/toolchains/llvm/prebuilt/{ARCH}/lib/clang/{clang_version}/lib/linux
#[inline]
fn clang_lib_path(ndk_home: &Path) -> PathBuf {
    let clang_folder: PathBuf = ndk_home
        .join("toolchains")
        .join("llvm")
        .join("prebuilt")
        .join(ARCH)
        .join("lib")
        .join("clang");

    let clang_lib_version = std::fs::read_dir(&clang_folder)
        .expect("Unable to get clang target directory")
        .filter_map(|a| a.ok())
        .max_by(|a, b| a.file_name().cmp(&b.file_name()))
        .expect("Unable to get clang target")
        .path();

    clang_folder
        .join(clang_lib_version)
        .join("lib")
        .join("linux")
}

const CARGO_NDK_SYSROOT_PATH_KEY: &'static str = "CARGO_NDK_SYSROOT_PATH";
const CARGO_NDK_SYSROOT_TARGET_KEY: &'static str = "CARGO_NDK_SYSROOT_TARGET";
const CARGO_NDK_SYSROOT_LIBS_PATH_KEY: &'static str = "CARGO_NDK_SYSROOT_LIBS_PATH";

pub(crate) fn build_env(
    triple: &str,
    ndk_home: &Path,
    clang_target: &str,
    link_builtins: bool,
) -> BTreeMap<String, OsString> {
    let self_path = dunce::canonicalize(env::args().next().unwrap())
        .expect("Failed to canonicalize absolute path to cargo-ndk")
        .parent()
        .unwrap()
        .join("cargo-ndk");

    // Environment variables for the `cc` crate
    let (cc_key, _) = cc_env("CC", triple);
    let (cflags_key, cflags_value) = cc_env("CFLAGS", triple);
    let (cxx_key, _) = cc_env("CXX", triple);
    let (cxxflags_key, cxxflags_value) = cc_env("CXXFLAGS", triple);
    let (ar_key, _) = cc_env("AR", triple);
    let (ranlib_key, _) = cc_env("RANLIB", triple);

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
    let cargo_ndk_sysroot_path = ndk_home.join(sysroot_suffix(ARCH));
    let cargo_ndk_sysroot_target = sysroot_target(triple);
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
            CARGO_NDK_SYSROOT_PATH_KEY.to_string(),
            cargo_ndk_sysroot_path.clone().into_os_string(),
        ),
        (
            CARGO_NDK_SYSROOT_LIBS_PATH_KEY.to_string(),
            cargo_ndk_sysroot_libs_path.into_os_string(),
        ),
        (
            CARGO_NDK_SYSROOT_TARGET_KEY.to_string(),
            cargo_ndk_sysroot_target.into(),
        ),
        // https://github.com/KyleMayes/clang-sys?tab=readme-ov-file#environment-variables
        ("CLANG_PATH".into(), target_cc.clone().into()),
        ("_CARGO_NDK_LINK_TARGET".into(), clang_target.into()), // Recognized by main() so we know when we're acting as a wrapper
        ("_CARGO_NDK_LINK_CLANG".into(), target_cc.into()),
    ]
    .into_iter()
    .collect::<BTreeMap<String, OsString>>();

    if link_builtins {
        let builtins_path = clang_lib_path(ndk_home);
        envs.insert("_CARGO_NDK_LINK_BUILTINS".to_string(), builtins_path.into());
    }

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

    let bindgen_args = format!(
        "--sysroot={} -I{}",
        &cargo_ndk_sysroot_path.display(),
        extra_include
    );
    let bindgen_clang_args = bindgen_args.replace('\\', "/");

    envs.insert(
        bindgen_clang_args_key.to_string(),
        bindgen_clang_args.into(),
    );

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
    link_builtins: bool,
    cargo_args: &[String],
    cargo_manifest: &Path,
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
    let envs = build_env(triple, ndk_home, &clang_target, link_builtins);

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
