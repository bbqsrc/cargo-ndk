use std::{
    env,
    ffi::OsString,
    fs::File,
    io::{self, BufRead, Write},
    path::{Path, PathBuf},
    process::Command,
};

use cargo_metadata::{camino::Utf8PathBuf, semver::Version};

#[cfg(target_os = "windows")]
const NDK_CMD_QUOTE_ISSUE: &str = "https://github.com/android/ndk/issues/1856";

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

#[cfg(windows)]
fn read_lines<P>(filename: P) -> io::Result<io::Lines<io::BufReader<File>>>
where
    P: AsRef<Path>,
{
    let file = File::open(filename)?;
    Ok(io::BufReader::new(file).lines())
}

#[cfg(windows)]
fn write_lines<P>(filename: P, lines: &[String]) -> io::Result<()>
where
    P: AsRef<Path>,
{
    let file = File::create(filename)?;
    for line in lines {
        writeln!(&file, "{line}")?;
    }
    Ok(())
}

/// This fixes a quoting bug in the r25 NDK .cmd wrapper scripts for
/// Windows, ref: https://github.com/android/ndk/issues/1856
///
/// Returns `true` if the workaround is required, else `false`
#[cfg(windows)]
fn ndk_r25_workaround_patch_cmd_script<P>(filename: P, apply: bool) -> bool
where
    P: AsRef<Path> + std::fmt::Debug,
{
    let mut patched = false;
    let lines = read_lines(&filename).unwrap_or_else(|err| {
        log::error!("Failed to read {filename:?} to check if quoting workaround required: {err:?}");
        std::process::exit(1);
    });

    let lines: Vec<String> = lines
        .filter_map(|line| {
            if let Ok(line) = line {
                if line == r#"if "%1" == "-cc1" goto :L"# {
                    patched = true;
                    Some(r#"if "%~1" == "-cc1" goto :L"#.to_string())
                } else {
                    Some(line)
                }
            } else {
                None
            }
        })
        .collect();

    if patched {
        if !apply {
            log::error!(
                "NDK .cmd wrapper needs quoting workaround ({NDK_CMD_QUOTE_ISSUE}): {filename:?}"
            );
            patched
        } else {
            if let Err(err) = write_lines(&filename, &lines) {
                log::error!("Failed to patch {filename:?} to workaround quoting bug ({NDK_CMD_QUOTE_ISSUE}): {err:?}");
                std::process::exit(1);
            }
            log::info!(
                "Applied NDK .cmd wrapper quoting workaround ({NDK_CMD_QUOTE_ISSUE}): {filename:?}"
            );
            false
        }
    } else {
        false
    }
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
    apply_ndk_quote_workaround: bool,
    #[allow(unused_variables)] out_dir: &Utf8PathBuf,
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
    let target_ranlib = ndk_home.join(ndk23_tool(ARCH, "llvm-ranlib"));

    let cc_key = format!("CC_{}", &triple);
    let ar_key = format!("AR_{}", &triple);
    let cxx_key = format!("CXX_{}", &triple);
    let ranlib_key = format!("RANLIB_{}", &triple);
    let bindgen_clang_args_key = format!("BINDGEN_EXTRA_CLANG_ARGS_{}", &triple.replace('-', "_"));
    let cargo_bin = env::var("CARGO").unwrap_or_else(|_| "cargo".into());

    log::debug!("cargo: {}", &cargo_bin);
    log::debug!("{}={}", &ar_key, &target_ar.display());
    log::debug!("{}={}", &cc_key, &target_linker.display());
    log::debug!("{}={}", &cxx_key, &target_cxx.display());
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

    #[cfg(windows)]
    {
        if version.major == 25 {
            let needs_workaround =
                ndk_r25_workaround_patch_cmd_script(target_linker, apply_ndk_quote_workaround)
                    | ndk_r25_workaround_patch_cmd_script(target_cxx, true);
            if needs_workaround {
                log::warn!("Re-run with --apply-ndk-quote-workaround to patch NDK with quoting workaround ({NDK_CMD_QUOTE_ISSUE})");
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
    let target_strip = ndk_home.join(ndk23_tool(ARCH, "llvm-strip"));

    log::debug!("strip: {}", &target_strip.display());

    Command::new(target_strip)
        .arg(bin_path)
        .status()
        .expect("strip crashed")
}
