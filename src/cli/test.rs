use std::{env, path::PathBuf, process::Command};

use clap::Parser;

use crate::{
    clang_target,
    cli::{HasCargoArgs, derive_adb_path, derive_ndk_path, derive_ndk_version, init},
    meta::Target,
};

#[derive(Debug, Parser, Clone)]
struct TestArgs {
    /// Triples for the target. Can be Rust or Android target names (i.e. arm64-v8a)
    #[arg(short, long, env = "CARGO_NDK_TARGET")]
    target: Target,

    /// Platform (also known as API level)
    #[arg(short = 'P', long, default_value_t = 21, env = "CARGO_NDK_PLATFORM")]
    platform: u8,

    /// Links Clang builtins library
    #[arg(long, default_value_t = false, env = "CARGO_NDK_LINK_BUILTINS")]
    link_builtins: bool,

    /// Links libc++_shared library
    #[arg(long, default_value_t = false, env = "CARGO_NDK_LINK_CXX_SHARED")]
    link_cxx_shared: bool,

    /// Path to Cargo.toml
    #[arg(long, value_name = "PATH")]
    manifest_path: Option<PathBuf>,

    #[arg(long, env = "CARGO_NDK_ADB_SERIAL")]
    /// "Serial number" of the device to use for testing (e.g. "emulator-5554" or "0123456789ABCDEF")
    ///
    /// You can find the serial number of your device by running `adb devices`.
    ///
    /// If not set, the first available device will be used.
    adb_serial: Option<String>,

    /// Arguments to be passed to cargo test
    #[arg(allow_hyphen_values = true)]
    cargo_args: Vec<String>,

    #[arg(last = true)]
    /// Additional arguments to pass to the test binary on device
    test_args: Vec<String>,
}

impl HasCargoArgs for TestArgs {
    fn set_cargo_args(&mut self, args: Vec<String>) {
        self.cargo_args = args;
    }
}

pub fn run(args: Vec<String>) -> anyhow::Result<()> {
    // Check for help/version before parsing to avoid required arg errors
    let valid_args = args.split(|x| x == "--").next().unwrap_or(&args).to_vec();
    let (mut shell, _) = init(valid_args)?;

    let mut args = match TestArgs::try_parse_from(&args) {
        Ok(args) => args,
        Err(e) => {
            shell.error(e)?;
            std::process::exit(2);
        }
    };

    // Workaround for -- capturing being weird in clap
    if let Some(idx) = args.cargo_args.iter().rposition(|x| x == "--") {
        args.test_args.extend(args.cargo_args.split_off(idx + 1));
        args.cargo_args.truncate(idx);
    }

    // Get adb path
    let adb_path = match derive_adb_path(&mut shell) {
        Ok(path) => path,
        Err(e) => {
            shell.error(e)?;
            std::process::exit(1);
        }
    };

    shell.verbose(|shell| {
        shell.status_with_color(
            "Found",
            format!("adb at {}", adb_path.display()),
            termcolor::Color::Cyan,
        )
    })?;

    // Get NDK path for building
    let (ndk_home, ndk_detection_method) = match derive_ndk_path(&mut shell) {
        Some((path, method)) => (path, method),
        None => {
            shell.error("Could not find any NDK.")?;
            shell.note(
                "Set the environment ANDROID_NDK_HOME to your NDK installation's root directory,\nor install the NDK using Android Studio."
            )?;
            std::process::exit(1);
        }
    };

    let ndk_version = match derive_ndk_version(&ndk_home) {
        Ok(v) => v,
        Err(e) => {
            shell.error(format!(
                "Error detecting NDK version for path {}",
                ndk_home.display()
            ))?;
            shell.error(e)?;
            std::process::exit(1);
        }
    };

    shell.verbose(|shell| {
        shell.status_with_color(
            "Detected",
            format!(
                "NDK v{} ({}) [{}]",
                ndk_version,
                ndk_home.display(),
                ndk_detection_method
            ),
            termcolor::Color::Cyan,
        )
    })?;

    let working_dir = env::current_dir().expect("current directory could not be resolved");
    let target = args.target;
    let platform = args.platform;

    // Set up environment for cargo test build
    let triple = target.triple();
    let clang_target = clang_target(triple, platform);

    let env_vars = crate::cargo::build_env(
        triple,
        &ndk_home,
        &ndk_version,
        &clang_target,
        args.link_builtins,
        args.link_cxx_shared,
    );

    shell.verbose(|shell| {
        shell.status_with_color(
            "Building",
            format!("test binary for {} ({})", &target, &triple),
            termcolor::Color::Cyan,
        )
    })?;

    // Now we have the test binary built, we can find it in the output
    let mut test_cmd = Command::new("cargo");
    test_cmd
        .args(["test", "--target", triple])
        .args(&args.cargo_args)
        .arg("--")
        .args(&args.test_args)
        .envs(env_vars)
        .current_dir(&working_dir);

    if let Some(manifest_path) = &args.manifest_path {
        test_cmd.arg("--manifest-path").arg(manifest_path);
    }

    test_cmd.status().unwrap();

    std::process::exit(0);
}
