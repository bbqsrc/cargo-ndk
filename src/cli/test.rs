use std::{
    env,
    path::PathBuf,
    process::{Command, Stdio},
};

use clap::{CommandFactory, Parser};

use crate::{
    cli::{
        CommandExt as _, derive_adb_path, derive_ndk_path, derive_ndk_version,
        is_supported_rustc_version, panic_hook,
    },
    meta::Target,
    shell::{Shell, Verbosity},
};

#[derive(Debug, Parser, Clone)]
struct TestArgs {
    /// Triples for the target. Can be Rust or Android target names (i.e. arm64-v8a)
    #[arg(short, long, env = "CARGO_NDK_TARGET")]
    target: Target,

    /// Platform (also known as API level)
    #[arg(long, default_value_t = 21, env = "CARGO_NDK_PLATFORM")]
    platform: u8,

    /// Links Clang builtins library
    #[arg(long, default_value_t = false, env = "CARGO_NDK_LINK_BUILTINS")]
    link_builtins: bool,

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

pub struct TestUnit {
    executable: PathBuf,
    name: String,
    rel_path: String,
}

pub fn run(args: Vec<String>) -> anyhow::Result<()> {
    // Check for help/version before parsing to avoid required arg errors
    let valid_args = args.split(|x| x == "--").next().unwrap_or(&args);

    if valid_args.contains(&"--help".to_string()) {
        TestArgs::command().print_long_help().unwrap();
        std::process::exit(0);
    }

    if valid_args.contains(&"-h".to_string()) {
        TestArgs::command().print_help().unwrap();
        std::process::exit(0);
    }

    if args.contains(&"--version".to_string()) || args.contains(&"-V".to_string()) {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }

    let verbosity = if valid_args.contains(&"-q".into()) {
        Verbosity::Quiet
    } else if valid_args.contains(&"-vv".into()) {
        Verbosity::VeryVerbose
    } else if valid_args.contains(&"-v".into()) || valid_args.contains(&"--verbose".into()) {
        Verbosity::Verbose
    } else {
        Verbosity::Normal
    };

    let color = args
        .iter()
        .position(|x| x == "--color")
        .and_then(|p| args.get(p + 1))
        .map(|x| &**x);

    let mut shell = Shell::new();
    shell.set_verbosity(verbosity);
    shell.set_color_choice(color)?;

    if std::env::var_os("CARGO_NDK_NO_PANIC_HOOK").is_none() {
        std::panic::set_hook(Box::new(panic_hook));
    }

    shell.verbose(|shell| {
        shell.status_with_color(
            "Using",
            format!("cargo-ndk v{} (test mode)", env!("CARGO_PKG_VERSION"),),
            termcolor::Color::Cyan,
        )
    })?;

    if !is_supported_rustc_version() {
        shell.error("Rust compiler is too old and not supported by cargo-ndk.")?;
        shell.note("Upgrade Rust to at least v1.68.0.")?;
        std::process::exit(1);
    }

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
    let clang_target = crate::cargo::clang_target(triple, platform);

    let env_vars = crate::cargo::build_env(triple, &ndk_home, &clang_target, args.link_builtins);

    shell.verbose(|shell| {
        shell.status_with_color(
            "Building",
            format!("test binary for {} ({})", &target, &triple),
            termcolor::Color::Cyan,
        )
    })?;

    // Build test binary with --no-run
    let mut test_cmd = Command::new("cargo");
    test_cmd
        .args([
            "test",
            "--no-run",
            "--message-format",
            "json",
            "--target",
            triple,
        ])
        .args(&args.cargo_args)
        .envs(env_vars)
        .stderr(Stdio::inherit())
        .current_dir(&working_dir);

    if let Some(manifest_path) = &args.manifest_path {
        test_cmd.arg("--manifest-path").arg(manifest_path);
    }

    let output = test_cmd.output()?;

    let test_binary_paths = output
        .stdout
        .split(|c| *c == b'\n')
        .filter_map(|x| serde_json::from_slice::<serde_json::Value>(x).ok())
        .filter_map(|blob| {
            let artifact = blob.as_object()?;

            let Some(serde_json::Value::String(reason)) = artifact.get("reason") else {
                return None;
            };

            if reason == "compiler-artifact" {
                let executable = artifact
                    .get("executable")
                    .and_then(|v| v.as_str())
                    .map(PathBuf::from)?;

                let manifest_path = artifact
                    .get("manifest_path")
                    .and_then(|v| v.as_str())
                    .map(PathBuf::from)?;

                let src_path = artifact
                    .get("target")
                    .and_then(|v| v.get("src_path"))
                    .and_then(|v| v.as_str())
                    .map(PathBuf::from)?;

                let working_path = manifest_path.parent().unwrap();

                let rel_path = executable
                    .strip_prefix(working_path)
                    .unwrap_or(&executable)
                    .to_string_lossy()
                    .to_string();

                let src_path = src_path
                    .strip_prefix(working_path)
                    .unwrap_or(&src_path)
                    .to_string_lossy()
                    .to_string();

                Some(TestUnit {
                    executable,
                    rel_path,
                    name: src_path,
                })
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    if !output.status.success() {
        shell.error("Failed to build test binary")?;
        std::process::exit(output.status.code().unwrap_or(1));
    }

    if test_binary_paths.is_empty() {
        shell.error("No test binary found in the build output")?;
        std::process::exit(1);
    }

    let mut failed = false;

    for test_binary_path in test_binary_paths {
        // Push binary to device
        let device_path = format!(
            "/data/local/tmp/{}",
            test_binary_path
                .executable
                .file_name()
                .unwrap()
                .to_string_lossy()
        );

        // Ugly but works
        shell.verbose(|shell| {
            shell.status_header("Pushing")?;
            shell.reset_err()?;
            shell
                .err()
                .write_fmt(format_args!("test binary to device: {device_path}\r"))?;
            shell.set_needs_clear(true);
            Ok(())
        })?;

        let push_status = Command::new(&adb_path)
            .with_serial(args.adb_serial.as_deref())
            .arg("push")
            .arg(&test_binary_path.executable)
            .arg(&device_path)
            .output()?;

        if !push_status.status.success() {
            shell.error("Failed to push test binary to device")?;
            eprintln!("{}", std::str::from_utf8(&push_status.stderr)?.trim());
            shell.note("If multiple devices, use --adb-serial to specify one.")?;
            shell.note("Run `adb devices` to see connected devices.")?;
            std::process::exit(push_status.status.code().unwrap_or(1));
        }

        shell.verbose(|shell| {
            shell.status("Pushing", format!("test binary to device ({device_path})"))
        })?;

        // Make binary executable
        let chmod_status = Command::new(&adb_path)
            .with_serial(args.adb_serial.as_deref())
            .arg("shell")
            .arg("chmod")
            .arg("755")
            .arg(&device_path)
            .status()?;

        if !chmod_status.success() {
            shell.error("Failed to make test binary executable")?;
            std::process::exit(chmod_status.code().unwrap_or(1));
        }

        // Run the test binary on device
        shell.status(
            "Running",
            format!(
                "unittests {} ({})",
                test_binary_path.name, test_binary_path.rel_path
            ),
        )?;
        shell.reset_err()?;

        let verbosity_arg = match verbosity {
            Verbosity::Quiet => "-q",
            _ => "",
        };

        let run_status = Command::new(&adb_path)
            .with_serial(args.adb_serial.as_deref())
            .arg("shell")
            .arg(&device_path)
            .arg(verbosity_arg)
            .args(&args.test_args)
            .status()?;

        // Clean up the binary from device
        let _ = Command::new(&adb_path)
            .with_serial(args.adb_serial.as_deref())
            .arg("shell")
            .arg("rm")
            .arg(&device_path)
            .status();

        if !run_status.success() {
            failed = true;
        }
    }

    shell.note("No doctests can currently be run on Android devices. Please run them on your host machine.")?;

    std::process::exit(if failed { 1 } else { 0 });
}
