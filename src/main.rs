use clap::{App, AppSettings, Arg, SubCommand};
use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{exit, Command};

#[cfg(target_os = "macos")]
const ARCH: &'static str = "darwin-x86_64";
#[cfg(target_os = "linux")]
const ARCH: &'static str = "linux-x86_64";
#[cfg(target_os = "windows")]
const ARCH: &'static str = "windows-x86_64";

#[cfg(target_os = "windows")]
const EXT: &'static str = ".cmd";
#[cfg(not(target_os = "windows"))]
const EXT: &'static str = "";

fn clang_suffix(triple: &str, arch: &str, platform: &str) -> PathBuf {
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
        &format!("{}{}-clang{}", tool_triple, platform, EXT),
    ]
    .iter()
    .collect()
}

fn toolchain_triple<'a>(triple: &'a str) -> &'a str {
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
    libs: Vec<PathBuf>,
) -> std::process::ExitStatus {
    let target_ar = Path::new(&ndk_home).join(toolchain_suffix(&triple, &ARCH, "ar"));
    let target_linker = Path::new(&ndk_home).join(clang_suffix(&triple, &ARCH, &platform));

    let cc_key = format!("CC_{}", &triple);
    let ar_key = format!("AR_{}", &triple);

    log::debug!("ar: {:?}", &target_ar);
    log::debug!("linker: {:?}", &target_linker);

    Command::new("cargo")
        .current_dir(dir)
        .env(ar_key, &target_ar)
        .env(cc_key, &target_linker)
        .env(cargo_env_target_cfg(&triple, "ar"), &target_ar)
        .env(cargo_env_target_cfg(&triple, "linker"), &target_linker)
        .args(cargo_args)
        .arg("--target")
        .arg(&triple)
        .status()
        .expect("Success")
}

fn main() {
    env_logger::init();

    let app_matches = App::new("cargo-ndk")
        .bin_name("cargo")
        .subcommand(SubCommand::with_name("ndk")
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
                .long("android-platform")
                .value_name("PLATFORM")
                .takes_value(true)
                .required(true)
                .help("The platform to target (example: 16)"))
            .arg(Arg::with_name("cargo-args")
                .value_name("CARGO_ARGS")
                .required(true)
                .takes_value(true)
                .multiple(true)
            )
        )
        .get_matches();

    let matches = match app_matches.subcommand_matches("ndk") {
        Some(v) => v,
        None => {
            eprintln!("This binary may only be called via `cargo ndk`.");
            exit(1);
        }
    };

    let ndk_home = match env::var_os("NDK_HOME") {
        Some(v) => v,
        None => {
            eprintln!("No NDK_HOME set.");
            exit(1);
        }
    };

    let triple = matches.value_of("target").expect("Target not to be null");
    let platform = matches
        .value_of("platform")
        .expect("Platform not to be null");
    let cargo_args: Vec<&str> = matches
        .values_of("cargo-args")
        .expect("Cargo-args to not be null")
        .collect();

    let status = run(
        &std::env::current_dir().unwrap(),
        &ndk_home,
        triple,
        platform,
        cargo_args,
        vec![],
    );

    exit(status.code().unwrap_or(-1));
}
