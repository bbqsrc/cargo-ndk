extern crate clap;

use std::env;
use clap::{App, AppSettings, Arg, SubCommand};
use std::process::{Command, exit};
use std::path::Path;

#[cfg(target_os = "macos")]
const ARCH: &'static str = "darwin-x86_64";
#[cfg(target_os = "linux")]
const ARCH: &'static str = "linux-x86_64";

fn triples(triple: &str) -> (String, String) {
    let toolchain_triple = match triple {
        "armv7-linux-androideabi" => "arm-linux-androideabi",
        "i686-linux-android" => "x86",
        "x86_64-linux-android" => "x86_64",
        _ => triple
    };

    let tool_triple = match triple {
        "armv7-linux-androideabi" => "arm-linux-androideabi",
        _ => triple
    };

    (toolchain_triple.to_string(), tool_triple.to_string())
}

fn toolchain_sysroot(triple: &str, arch: &str) -> String {
    let (toolchain_triple, tool_triple) = triples(triple);
    format!("toolchains/{}-4.9/prebuilt/{}/{}/bin", toolchain_triple, arch, tool_triple)
}

fn toolchain_suffix(triple: &str, arch: &str, bin: &str) -> String {
    let (toolchain_triple, tool_triple) = triples(triple);
    format!("toolchains/{}-4.9/prebuilt/{}/bin/{}-{}", toolchain_triple, arch, tool_triple, bin)
}

fn toolchain_libs_path(triple: &str, arch: &str) -> String {
    let (toolchain_triple, tool_triple) = triples(triple);
    format!("toolchains/{}-4.9/prebuilt/{}/lib/gcc/{}/4.9.x", toolchain_triple, arch, tool_triple)
}

fn platform_suffix(triple: &str, platform: &str) -> String {
    let arch: &str = triple.split("-").collect::<Vec<&str>>()[0];
    let toolchain_arch = match arch {
        "armv7" => "arm",
        "i686" => "x86",
        "aarch64" => "arm64",
        _ => arch
    };
    format!("platforms/android-{}/arch-{}", platform, toolchain_arch)
}

fn cargo_env_target_cfg(triple: &str, key: &str) -> String {
    format!("CARGO_TARGET_{}_{}", &triple.replace("-", "_"), key).to_uppercase()
}

fn main() {
    let app_matches = App::new("cargo-ndk")
        .version(env!("CARGO_PKG_VERSION"))
        .author("Brendan Molloy <brendan@bbqsrc.net>")
        .about("Automatically interfaces with the NDK to build Rust libraries.")
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

    let matches = app_matches
        .subcommand_matches("ndk")
        .expect("ndk matches must be found");

    let ndk_home = match env::var_os("NDK_HOME") {
        Some(v) => v,
        None => {
            eprintln!("No NDK_HOME set.");
            exit(1);
        }
    };

    let triple = matches.value_of("target").expect("Target not to be null");
    let platform = matches.value_of("platform").expect("Platform not to be null");
    let cargo_args: Vec<&str> = matches.values_of("cargo-args")
        .expect("Cargo-args to not be null")
        .collect();

    let target_ar = Path::new(&ndk_home)
        .join(toolchain_suffix(&triple, &ARCH, "ar"));
    let target_linker = Path::new(&ndk_home)
        .join(toolchain_suffix(&triple, &ARCH, "gcc"));
    let target_sysroot = Path::new(&ndk_home)
        .join(platform_suffix(&triple, &platform));
    let target_path = format!("{}:{}",
        Path::new(&ndk_home).join(toolchain_sysroot(&triple, &ARCH)).to_str().unwrap(),
        env::var_os("PATH").unwrap().to_str().unwrap());
    let target_tool_libs = Path::new(&ndk_home).join(toolchain_libs_path(&triple, &ARCH));
    let target_rustflags = format!("-Clink-arg=--sysroot={} -Clink-arg=-L{}",
        target_sysroot.to_str().unwrap(),
        target_tool_libs.to_str().unwrap());

    let status = Command::new("cargo")
        .env("PATH", &target_path)
        .env(cargo_env_target_cfg(&triple, "ar"), &target_ar)
        .env(cargo_env_target_cfg(&triple, "linker"), &target_linker)
        .env(cargo_env_target_cfg(&triple, "rustflags"), &target_rustflags)
        .args(cargo_args)
        .arg("--target")
        .arg(&triple)
        .status()
        .expect("Success");

    exit(status.code().unwrap_or(-1));
}
