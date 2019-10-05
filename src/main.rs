use clap::{App, AppSettings, Arg, SubCommand};
use std::env;
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

fn toolchain_suffix(triple: &str, arch: &str, platform: &str, bin: &str) -> PathBuf {
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
        &format!("{}{}-{}{}", tool_triple, platform, bin, EXT),
    ]
    .iter()
    .collect()
}

fn platform_suffix(triple: &str, platform: &str) -> PathBuf {
    let arch: &str = triple.split("-").collect::<Vec<&str>>()[0];
    let toolchain_arch = match arch {
        "armv7" => "arm",
        "i686" => "x86",
        "aarch64" => "arm64",
        _ => arch,
    };
    [
        "platforms",
        &format!("android-{}", platform),
        &format!("arch-{}", toolchain_arch),
    ]
    .iter()
    .collect()
}

fn arm_unwind_sysroot(arch: &str) -> PathBuf {
    [
        "toolchains",
        "llvm",
        "prebuilt",
        arch,
        "sysroot",
        "usr",
        "lib",
        "arm-linux-androideabi",
    ]
    .iter()
    .collect()
}

fn cargo_env_target_cfg(triple: &str, key: &str) -> String {
    format!("CARGO_TARGET_{}_{}", &triple.replace("-", "_"), key).to_uppercase()
}

fn main() {
    let app_matches = App::new("cargo-ndk")
        .version(env!("CARGO_PKG_VERSION"))
        .author("Brendan Molloy <brendan@bbqsrc.net>")
        .about("Automatically interfaces with the NDK to build Rust libraries. Minimum compatible NDK version: r19c.")
        .setting(AppSettings::TrailingVarArg)
        .bin_name("cargo")
        .subcommand(SubCommand::with_name("ndk")
            .arg(Arg::with_name("target")
                .long("target")
                .value_name("TARGET")
                .takes_value(true)
                .required(true)
                .help("The triple for the target"))
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
                .multiple(true))
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

    let target_ar = Path::new(&ndk_home).join(toolchain_suffix(&triple, &ARCH, &platform, "ar"));
    let target_linker =
        Path::new(&ndk_home).join(toolchain_suffix(&triple, &ARCH, &platform, "clang"));
    let target_sysroot = Path::new(&ndk_home).join(platform_suffix(&triple, &platform));
    let mut target_rustflags = format!("-Clink-arg=--sysroot={}", target_sysroot.to_str().unwrap());

    if triple.starts_with("arm") {
        target_rustflags.push_str(" ");
        target_rustflags.push_str(&format!(
            "-Clink-arg=-L{}",
            Path::new(&ndk_home)
                .join(arm_unwind_sysroot(&ARCH))
                .to_str()
                .unwrap()
        ));
    }

    let status = Command::new("cargo")
        .env(cargo_env_target_cfg(&triple, "ar"), &target_ar)
        .env(cargo_env_target_cfg(&triple, "linker"), &target_linker)
        .env(
            cargo_env_target_cfg(&triple, "rustflags"),
            &target_rustflags,
        )
        .args(cargo_args)
        .arg("--target")
        .arg(&triple)
        .status()
        .expect("Success");

    exit(status.code().unwrap_or(-1));
}
