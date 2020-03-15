use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{exit, Command};

use clap::{App, AppSettings, Arg};

#[cfg(target_os = "macos")]
const ARCH: &str = "darwin-x86_64";
#[cfg(target_os = "linux")]
const ARCH: &str = "linux-x86_64";
#[cfg(target_os = "windows")]
const ARCH: &str = "windows-x86_64";

#[cfg(target_os = "windows")]
const EXT: &str = ".cmd";
#[cfg(not(target_os = "windows"))]
const EXT: &str = "";

fn clang_suffix(triple: &str, arch: &str, platform: &str, postfix: &str) -> PathBuf {
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
        &format!("{}{}-clang{}{}", tool_triple, platform, postfix, EXT),
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
        &format!("{}-{}{}", toolchain_triple(triple), bin, EXT),
    ]
    .iter()
    .collect()
}

fn cargo_env_target_cfg(triple: &str, key: &str) -> String {
    format!("CARGO_TARGET_{}_{}", &triple.replace("-", "_"), key).to_uppercase()
}

fn run(
    dir: &Path,
    ndk_home: &OsString,
    triple: &str,
    platform: &str,
    cargo_args: Vec<&str>,
) -> std::process::ExitStatus {
    let target_ar = Path::new(&ndk_home).join(toolchain_suffix(&triple, &ARCH, "ar"));
    let target_linker = Path::new(&ndk_home).join(clang_suffix(&triple, &ARCH, &platform, ""));
    let target_cxx = Path::new(&ndk_home).join(clang_suffix(&triple, &ARCH, &platform, "++"));

    let cc_key = format!("CC_{}", &triple);
    let ar_key = format!("AR_{}", &triple);
    let cxx_key = format!("CXX_{}", &triple);
    let cargo_bin = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());

    log::debug!("ar: {:?}", &target_ar);
    log::debug!("linker: {:?}", &target_linker);
    log::debug!("cargo: {:?}", &cargo_bin);

    Command::new(cargo_bin)
        .current_dir(dir)
        .env(ar_key, &target_ar)
        .env(cc_key, &target_linker)
        .env(cxx_key, &target_cxx)
        .env(cargo_env_target_cfg(&triple, "ar"), &target_ar)
        .env(cargo_env_target_cfg(&triple, "linker"), &target_linker)
        .args(cargo_args)
        .arg("--target")
        .arg(&triple)
        .status()
        .expect("cargo crashed")
}

fn main() {
    env_logger::init();

    if env::var("CARGO").is_err() {
        eprintln!("This binary may only be called via `cargo ndk`.");
        exit(1);
    }

    // We used to check for NDK_HOME, so we'll keep doing that. But we'll also try ANDROID_NDK_HOME
    // and $ANDROID_SDK_HOME/ndk-bundle as this is how Android Studio configures the world
    let ndk_home = env::var_os("ANDROID_NDK_HOME")
        .or_else(|| env::var_os("NDK_HOME"))
        .or_else(|| {
            env::var_os("ANDROID_SDK_HOME")
                .as_ref()
                .map(|x| Path::new(x).join("ndk-bundle").into())
        });

    let ndk_home = match ndk_home {
        Some(v) => v,
        None => {
            eprintln!("Could not find any NDK.");
            eprintln!(
                "Set the environment ANDROID_NDK_HOME to your NDK installation's root directory."
            );
            exit(1);
        }
    };

    let matches = App::new("cargo-ndk")
        .bin_name("cargo-ndk")
        .setting(AppSettings::TrailingVarArg)
        .version(env!("CARGO_PKG_VERSION"))
        .author("Brendan Molloy <brendan@bbqsrc.net>")
        .about("Automatically interfaces with the NDK to build Rust libraries. Minimum compatible NDK version: r19c.")
        .arg(Arg::with_name("target")
            .long("target")
            .value_name("TARGET")
            .takes_value(true)
            .required(true)
            .help("The triple for the target")
            .long_help("The following targets are supported:
  * aarch64-linux-android
  * armv7-linux-androideabi
  * i686-linux-android
  * x86_64-linux-android"))
        .arg(Arg::with_name("platform")
            .long("platform")
            .alias("android-platform")
            .value_name("PLATFORM")
            .required(true)
            .help("The platform to target"))
        .arg(Arg::with_name("cargo-args")
            .value_name("CARGO_ARGS")
            .required(true)
            .takes_value(true)
            .multiple(true)
        )
        .get_matches_from(env::args().skip(1));

    let triple = matches.value_of("target").expect("Target not to be null");
    let platform = matches
        .value_of("platform")
        .expect("Platform not to be null");
    let cargo_args: Vec<&str> = matches
        .values_of("cargo-args")
        .expect("Cargo-args to not be null")
        .collect();

    let status = run(
        &std::env::current_dir().expect("current directory could not be resolved"),
        &ndk_home,
        triple,
        platform,
        cargo_args,
    );

    exit(status.code().unwrap_or(-1));
}
