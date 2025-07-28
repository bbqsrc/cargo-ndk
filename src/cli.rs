use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    fs, io,
    panic::PanicHookInfo,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Instant,
};

use anyhow::Context;
use cargo_metadata::{Artifact, CrateType, MetadataCommand, camino::Utf8Path, semver::Version};
use clap::{CommandFactory, Parser};
use filetime::FileTime;

use crate::{
    cargo::{build_env, clang_target},
    meta::{Target, default_targets},
    shell::{Shell, Verbosity},
};

trait CommandExt {
    fn with_serial(self, serial: Option<&str>) -> Self;
}

impl CommandExt for Command {
    fn with_serial(mut self, serial: Option<&str>) -> Self {
        if let Some(serial) = serial {
            self.arg("-s").arg(serial);
        }
        self
    }
}

#[derive(Debug, Parser)]
struct EnvArgs {
    /// Triples for the target. Can be Rust or Android target names (i.e. arm64-v8a)
    #[arg(short, long, env = "CARGO_NDK_TARGET")]
    target: Target,

    /// Platform (also known as API level)
    #[arg(long, default_value_t = 21, env = "CARGO_NDK_PLATFORM")]
    platform: u8,

    /// Links Clang builtins library
    #[arg(long, default_value_t = false, env = "CARGO_NDK_LINK_BUILTINS")]
    link_builtins: bool,

    /// Use PowerShell syntax
    #[arg(long)]
    powershell: bool,

    /// Print output in JSON format
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Parser, Clone)]
struct BuildArgs {
    /// Triples for the target. Can be Rust or Android target names (i.e. arm64-v8a)
    #[arg(short, long, env = "CARGO_NDK_TARGET", value_delimiter = ',')]
    target: Vec<Target>,

    /// Platform (also known as API level)
    #[arg(long, default_value_t = 21, env = "CARGO_NDK_PLATFORM")]
    platform: u8,

    /// Links Clang builtins library
    #[arg(long, default_value_t = false, env = "CARGO_NDK_LINK_BUILTINS")]
    link_builtins: bool,

    /// Output to a `jniLibs` directory in the correct sub-directories
    #[arg(short, long, value_name = "DIR", env = "CARGO_NDK_OUTPUT_DIR")]
    output_dir: Option<PathBuf>,

    /// Path to Cargo.toml
    #[arg(long, value_name = "PATH")]
    manifest_path: Option<PathBuf>,

    /// Args to be passed to cargo
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    cargo_args: Vec<String>,
}

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

fn highest_version_ndk_in_path(ndk_dir: &Path) -> Option<PathBuf> {
    if ndk_dir.exists() {
        fs::read_dir(ndk_dir)
            .ok()?
            .filter_map(Result::ok)
            .filter_map(|x| {
                let path = x.path();
                path.components()
                    .next_back()
                    .and_then(|comp| comp.as_os_str().to_str())
                    .and_then(|name| Version::parse(name).ok())
                    .map(|version| (version, path))
            })
            .max_by(|(a, _), (b, _)| a.cmp(b))
            .map(|(_, path)| path)
    } else {
        None
    }
}

/// Return the name and value of the first environment variable that is set
///
/// Additionally checks that if any other variables are set then they should
/// be consistent with the first variable, otherwise a warning is printed.
fn find_first_consistent_var_set<'a>(
    vars: &'a [&str],
    shell: &mut Shell,
) -> Option<(&'a str, OsString)> {
    let mut first_var_set = None;
    for var in vars {
        if let Some(path) = env::var_os(var) {
            if let Some((first_var, first_path)) = first_var_set.as_ref() {
                if *first_path != path {
                    shell
                        .warn(format!(
                            "Environment variable `{first_var} = {first_path:#?}` doesn't match `{var} = {path:#?}`"
                        ))
                        .unwrap();
                }
                continue;
            }
            first_var_set = Some((*var, path));
        }
    }

    first_var_set
}

/// Return a path to a discovered NDK and string describing how it was found
fn derive_ndk_path(shell: &mut Shell) -> Option<(PathBuf, String)> {
    let ndk_vars = [
        "ANDROID_NDK_HOME",
        "ANDROID_NDK_ROOT",
        "ANDROID_NDK_PATH",
        "NDK_HOME",
    ];
    if let Some((var_name, path)) = find_first_consistent_var_set(&ndk_vars, shell) {
        let path = PathBuf::from(path);
        return highest_version_ndk_in_path(&path)
            .or(Some(path))
            .map(|path| (path, var_name.to_string()));
    }

    let sdk_vars = ["ANDROID_HOME", "ANDROID_SDK_ROOT", "ANDROID_SDK_HOME"];
    if let Some((var_name, sdk_path)) = find_first_consistent_var_set(&sdk_vars, shell) {
        let ndk_path = PathBuf::from(&sdk_path).join("ndk");
        if let Some(v) = highest_version_ndk_in_path(&ndk_path) {
            return Some((v, var_name.to_string()));
        }
    }

    let ndk_dir = default_ndk_dir();
    highest_version_ndk_in_path(&ndk_dir).map(|path| (path, "standard location".to_string()))
}

fn default_ndk_dir() -> PathBuf {
    #[cfg(windows)]
    let dir = pathos::user::local_dir()
        .unwrap()
        .to_path_buf()
        .join("Android")
        .join("sdk")
        .join("ndk");

    #[cfg(target_os = "linux")]
    let dir = pathos::xdg::home_dir()
        .unwrap()
        .join("Android")
        .join("Sdk")
        .join("ndk");

    #[cfg(target_os = "macos")]
    let dir = pathos::user::home_dir()
        .unwrap()
        .join("Library")
        .join("Android")
        .join("sdk")
        .join("ndk");

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    let dir = PathBuf::new();

    dir
}

/// Return a path to adb executable, resolving from ANDROID_HOME or ANDROID_SDK_ROOT
fn derive_adb_path(shell: &mut Shell) -> anyhow::Result<PathBuf> {
    let sdk_vars = ["ANDROID_HOME", "ANDROID_SDK_ROOT", "ANDROID_SDK_HOME"];
    if let Some((_, sdk_path)) = find_first_consistent_var_set(&sdk_vars, shell) {
        let adb_path = PathBuf::from(&sdk_path).join("platform-tools").join("adb");
        #[cfg(windows)]
        let adb_path = adb_path.with_extension("exe");

        if adb_path.exists() {
            return Ok(adb_path);
        }
    }

    // Fallback to system PATH
    #[cfg(windows)]
    let adb_name = "adb.exe";
    #[cfg(not(windows))]
    let adb_name = "adb";

    if let Ok(output) = Command::new("which").arg(adb_name).output() {
        if output.status.success() {
            let path_str = String::from_utf8_lossy(&output.stdout);
            let path_str = path_str.trim();
            return Ok(PathBuf::from(path_str));
        }
    }

    Err(anyhow::anyhow!(
        "Could not find adb. Please set ANDROID_HOME or ensure adb is in your PATH."
    ))
}

fn derive_ndk_version(path: &Path) -> anyhow::Result<Version> {
    let data = fs::read_to_string(path.join("source.properties"))?;
    for line in data.split('\n') {
        if line.starts_with("Pkg.Revision") {
            let mut chunks = line.split(" = ");
            let _ = chunks.next().ok_or_else(|| io::Error::other("No chunk"))?;
            let version = chunks.next().ok_or_else(|| io::Error::other("No chunk"))?;
            let version = match Version::parse(version) {
                Ok(v) => v,
                Err(_e) => {
                    return Err(anyhow::anyhow!(format!(
                        "Could not parse NDK version. Got: '{}'",
                        version
                    )));
                }
            };
            return Ok(version);
        }
    }

    Err(anyhow::anyhow!("Could not find Pkg.Revision in given path"))
}

fn is_supported_rustc_version() -> bool {
    version_check::is_min_version("1.68.0").unwrap_or_default()
}

fn panic_hook(info: &PanicHookInfo<'_>) {
    fn _attempt_shell(lines: &[String]) -> Result<(), anyhow::Error> {
        let mut shell = Shell::new();
        shell.error("cargo-ndk panicked! Generating report...")?;
        for line in lines {
            println!("{line}");
        }
        shell.error("end of panic report. Please report the above to: <https://github.com/bbqsrc/cargo-ndk/issues>")?;
        Ok(())
    }

    let location = info.location().unwrap();
    let msg = match info.payload().downcast_ref::<&'static str>() {
        Some(s) => *s,
        None => match info.payload().downcast_ref::<String>() {
            Some(s) => &s[..],
            None => "Box<dyn Any>",
        },
    };

    let env = std::env::vars()
        .map(|(x, y)| format!("{x}={y:?}"))
        .collect::<Vec<_>>();
    let args = std::env::args().collect::<Vec<_>>();

    let lines = vec![
        format!("location: {location}"),
        format!("message: {msg}"),
        format!("args: {args:?}"),
        format!(
            "pwd: {}",
            std::env::current_dir()
                .map(|x| x.display().to_string())
                .unwrap_or_else(|_| "<unknown>".to_string())
        ),
        format!("env:\n  {}", env.join("\n  ")),
    ];

    if _attempt_shell(&lines).is_err() {
        // Last ditch attempt
        for line in lines {
            eprintln!("{line}");
        }
    }
}

pub fn run_env(args: Vec<String>) -> anyhow::Result<()> {
    // Check for help/version before parsing to avoid required arg errors
    if args.contains(&"--help".to_string()) {
        EnvArgs::command().print_long_help().unwrap();
        std::process::exit(0);
    }

    if args.contains(&"-h".to_string()) {
        EnvArgs::command().print_help().unwrap();
        std::process::exit(0);
    }

    if args.contains(&"--version".to_string()) || args.contains(&"-V".to_string()) {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }

    let color = args
        .iter()
        .position(|x| x == "--color")
        .and_then(|p| args.get(p + 1))
        .map(|x| &**x);

    let verbosity = if args.contains(&"-q".into()) {
        Verbosity::Quiet
    } else if args.contains(&"-vv".into()) {
        Verbosity::VeryVerbose
    } else if args.contains(&"-v".into()) || args.contains(&"--verbose".into()) {
        Verbosity::Verbose
    } else {
        Verbosity::Normal
    };

    let mut shell = Shell::new();
    shell.set_verbosity(verbosity);
    shell.set_color_choice(color)?;

    let args = match EnvArgs::try_parse_from(&args) {
        Ok(args) => args,
        Err(e) => {
            shell.error(e)?;
            std::process::exit(2);
        }
    };

    let (ndk_home, _ndk_detection_method) = match derive_ndk_path(&mut shell) {
        Some((path, method)) => (path, method),
        None => {
            shell.error("Could not find any NDK.")?;
            shell.note(
                "Set the environment ANDROID_NDK_HOME to your NDK installation's root directory,\nor install the NDK using Android Studio."
            )?;
            std::process::exit(1);
        }
    };

    let clang_target = clang_target(args.target.triple(), args.platform);

    // Try command line, then config. Config falls back to defaults in any case.
    let env = build_env(
        args.target.triple(),
        &ndk_home,
        &clang_target,
        args.link_builtins,
    )
    .into_iter()
    .filter(|(k, _)| !k.starts_with('_'))
    .collect::<BTreeMap<_, _>>();

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(
                &env.into_iter()
                    .map(|(k, v)| (k, v.to_str().unwrap().to_string()))
                    .collect::<BTreeMap<_, _>>()
            )
            .unwrap()
        );
    } else if args.powershell {
        for (k, v) in env {
            println!("${{env:{k}}}={v:?}");
        }
        println!();
        println!("# To import with PowerShell:");
        println!("#     cargo ndk-env --powershell | Out-String | Invoke-Expression");
    } else {
        for (k, v) in env {
            println!("export {}={:?}", k.to_uppercase().replace('-', "_"), v);
        }
        println!();
        println!("# To import with bash/zsh/etc:");
        println!("#     source <(cargo ndk-env)");
    }

    Ok(())
}

/// Parse arguments that can appear both before and after the cargo subcommand
fn parse_mixed_args<T>(args: Vec<String>) -> anyhow::Result<T>
where
    T: clap::Parser + Clone + clap::CommandFactory,
    T: HasCargoArgs,
{
    let mut global_args = vec!["cargo-ndk".to_string()];
    let mut cargo_args = Vec::new();

    // Skip the "ndk" subcommand name (always the first argument)
    let mut i = 1;

    // Get all flags from the Args struct programmatically
    let cmd = T::command();
    let mut global_flags = Vec::new();
    let mut value_flags = Vec::new();

    for arg in cmd.get_arguments() {
        // Skip the cargo_args field since it's not a real flag
        if arg.get_id() == "cargo_args" {
            continue;
        }

        if let Some(long) = arg.get_long() {
            let long_flag = format!("--{long}");
            global_flags.push(long_flag.clone());

            // Check if this flag takes a value (not a boolean flag)
            if arg.get_action().takes_values() {
                value_flags.push(long_flag);
            }
        }
        if let Some(short) = arg.get_short() {
            let short_flag = format!("-{short}");
            global_flags.push(short_flag.clone());

            // Check if this flag takes a value (not a boolean flag)
            if arg.get_action().takes_values() {
                value_flags.push(short_flag);
            }
        }
    }

    while i < args.len() {
        let arg = &args[i];

        // Check if this is a global flag
        if global_flags.contains(&arg.to_string()) {
            global_args.push(arg.clone());

            // Check if this flag takes a value
            if value_flags.contains(&arg.to_string()) && i + 1 < args.len() {
                i += 1;
                global_args.push(args[i].clone());
            }
        } else if arg.starts_with("--") && arg.contains('=') {
            // Handle --flag=value format
            let flag_name = arg.split('=').next().unwrap();
            if global_flags.contains(&flag_name.to_string()) {
                global_args.push(arg.clone());
            } else {
                cargo_args.push(arg.clone());
            }
        } else {
            // This is a cargo arg
            cargo_args.push(arg.clone());
        }

        i += 1;
    }

    // Parse the extracted global args
    let mut parsed_args = T::try_parse_from(&global_args)?;

    // Set the cleaned cargo_args directly
    parsed_args.set_cargo_args(cargo_args);

    Ok(parsed_args)
}

trait HasCargoArgs {
    fn set_cargo_args(&mut self, args: Vec<String>);
}

impl HasCargoArgs for BuildArgs {
    fn set_cargo_args(&mut self, args: Vec<String>) {
        self.cargo_args = args;
    }
}

pub fn run(args: Vec<String>) -> anyhow::Result<()> {
    // Check for help/version before parsing to avoid required arg errors
    if args.contains(&"--help".to_string()) {
        BuildArgs::command().print_long_help().unwrap();
        std::process::exit(0);
    }

    if args.contains(&"-h".to_string()) {
        BuildArgs::command().print_help().unwrap();
        std::process::exit(0);
    }

    if args.contains(&"--version".to_string()) || args.contains(&"-V".to_string()) {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }

    let verbosity = if args.contains(&"-q".into()) {
        Verbosity::Quiet
    } else if args.contains(&"-vv".into()) {
        Verbosity::VeryVerbose
    } else if args.contains(&"-v".into()) || args.contains(&"--verbose".into()) {
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
            format!("cargo-ndk v{}", env!("CARGO_PKG_VERSION"),),
            termcolor::Color::Cyan,
        )
    })?;

    if !is_supported_rustc_version() {
        shell.error("Rust compiler is too old and not supported by cargo-ndk.")?;
        shell.note("Upgrade Rust to at least v1.68.0.")?;
        std::process::exit(1);
    }

    let args = match parse_mixed_args::<BuildArgs>(args) {
        Ok(args) => args,
        Err(e) => {
            shell.error(e)?;
            std::process::exit(2);
        }
    };

    if args.cargo_args.is_empty() {
        shell.error("No args found to pass to cargo!")?;
        shell.note("You still need to specify build arguments to cargo to achieve anything. :)")?;
        std::process::exit(1);
    }

    let metadata = match MetadataCommand::new().no_deps().exec() {
        Ok(v) => v,
        Err(e) => {
            shell.error("Failed to load Cargo.toml in current directory.")?;
            shell.error(e)?;
            std::process::exit(1);
        }
    };

    let out_dir = metadata.target_directory;

    // We used to check for NDK_HOME, so we'll keep doing that. But we'll also try ANDROID_NDK_HOME
    // and $ANDROID_SDK_HOME/ndk as this is how Android Studio configures the world
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

    // Attempt to smartly determine exactly what package is being worked with. The following is the manifest priority:
    //
    // 1. --manifest-path in the command-line arguments
    // 2. The manifest path of the package specified with `-p` for cargo.
    // 3. The manifest path in the current working dir
    let cargo_args = &args.cargo_args;
    let cargo_manifest = args
        .manifest_path
        .or_else(|| {
            if let Some(selected_package) = cargo_args
                .iter()
                .position(|arg| arg == "-p" || arg == "--package")
                .and_then(|idx| cargo_args.get(idx + 1))
            {
                let selected_package = metadata
                    .packages
                    .iter()
                    .find(|p| p.name.as_str() == selected_package)
                    .unwrap_or_else(|| panic!("unknown package: {selected_package}"));

                Some(selected_package.manifest_path.as_std_path().to_path_buf())
            } else {
                None
            }
        })
        .unwrap_or_else(|| working_dir.join("Cargo.toml"));

    let cmake_toolchain_path = ndk_home
        .join("build")
        .join("cmake")
        .join("android.toolchain.cmake");

    shell.very_verbose(|shell| {
        shell.status_with_color(
            "Exporting",
            format!("CARGO_NDK_CMAKE_TOOLCHAIN_PATH={:?}", &cmake_toolchain_path),
            termcolor::Color::Cyan,
        )
    })?;
    unsafe {
        env::set_var("CARGO_NDK_CMAKE_TOOLCHAIN_PATH", cmake_toolchain_path);
    }

    let platform = args.platform;

    // Try command line, then config. Config falls back to defaults in any case.
    let targets = if !args.target.is_empty() {
        args.target
    } else {
        default_targets().to_vec()
    };

    if let Some(output_dir) = args.output_dir.as_ref() {
        if let Err(e) = fs::create_dir_all(output_dir) {
            shell.error(format!("failed to create output dir, {e}"))?;
            std::process::exit(1);
        }

        // Canonicalize because path is shared with build scripts that can run in a different current_dir.
        let output_dir = match dunce::canonicalize(output_dir) {
            Ok(p) => p,
            Err(e) => {
                shell.error(format!("failed to canonicalize output dir, {e}"))?;
                if out_dir.is_absolute() {
                    output_dir.clone()
                } else {
                    std::process::exit(1)
                }
            }
        };

        shell.verbose(|shell| {
            shell.status_with_color(
                "Exporting",
                format!("CARGO_NDK_OUTPUT_PATH={output_dir:?}"),
                termcolor::Color::Cyan,
            )
        })?;

        unsafe {
            std::env::set_var("CARGO_NDK_OUTPUT_PATH", output_dir);
        }
    }

    shell.verbose(|shell| {
        shell.status_with_color(
            "Setting",
            format!("Android SDK platform level to {platform}"),
            termcolor::Color::Cyan,
        )
    })?;

    unsafe {
        env::set_var("CARGO_NDK_ANDROID_PLATFORM", platform.to_string());
    }
    shell.verbose(|shell| {
        shell.status_with_color(
            "Building",
            format!(
                "targets ({})",
                targets
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            termcolor::Color::Cyan,
        )
    })?;

    let start_time = Instant::now();

    let targets = targets
        .into_iter()
        .map(|target| {
            let triple = target.triple();
            shell.status("Building", format!("{} ({})", &target, &triple))?;

            shell.very_verbose(|shell| {
                shell.status_with_color(
                    "Exporting",
                    format!("CARGO_NDK_ANDROID_PLATFORM={:?}", &target.to_string()),
                    termcolor::Color::Cyan,
                )
            })?;
            unsafe {
                env::set_var("CARGO_NDK_ANDROID_PLATFORM", target.to_string());
            }

            // Set ANDROID_PLATFORM (API level)
            shell.very_verbose(|shell| {
                shell.status_with_color(
                    "Exporting",
                    format!("ANDROID_PLATFORM={platform}"),
                    termcolor::Color::Cyan,
                )
            })?;
            unsafe {
                env::set_var("ANDROID_PLATFORM", platform.to_string());
            }

            // Set ANDROID_ABI using the Android-specific target name
            let android_abi = target.to_string();
            shell.very_verbose(|shell| {
                shell.status_with_color(
                    "Exporting",
                    format!("ANDROID_ABI={:?}", &android_abi),
                    termcolor::Color::Cyan,
                )
            })?;
            unsafe {
                env::set_var("ANDROID_ABI", android_abi);
            }

            let (status, artifacts) = crate::cargo::run(
                &mut shell,
                &working_dir,
                &ndk_home,
                &ndk_version,
                triple,
                platform,
                args.link_builtins,
                &args.cargo_args,
                &cargo_manifest,
            )?;
            let code = status.code().unwrap_or(-1);

            if code != 0 {
                shell.note(
                    "If the build failed due to a missing target, you can run this command:",
                )?;
                shell.note("")?;
                shell.note(format!("    rustup target install {triple}"))?;
                std::process::exit(code);
            }

            Ok((target, artifacts))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    if let Some(output_dir) = args.output_dir.as_ref() {
        shell.concise(|shell| {
            shell.status(
                "Copying",
                format!(
                    "libraries to {}",
                    dunce::canonicalize(output_dir).unwrap().display()
                ),
            )
        })?;

        for (target, artifacts) in targets.iter() {
            shell.very_verbose(|shell| {
                shell.note(format!("artifacts for {target}: {artifacts:?}"))
            })?;

            let arch_output_dir = output_dir.join(target.to_string());
            fs::create_dir_all(&arch_output_dir).unwrap();

            if artifacts.is_empty() || !artifacts.iter().any(artifact_is_cdylib) {
                shell.error("No usable artifacts produced by cargo")?;
                shell.error("Did you set the crate-type in Cargo.toml to include 'cdylib'?")?;
                shell.error("For more info, see <https://doc.rust-lang.org/cargo/reference/cargo-targets.html#library>.")?;
                std::process::exit(1);
            }

            for artifact in artifacts.iter().filter(|a| artifact_is_cdylib(a)) {
                let Some(file) = artifact
                    .filenames
                    .iter()
                    .find(|name| name.extension() == Some("so"))
                else {
                    // This should never happen because we filter for cdylib outputs above but you
                    // never know... and it still feels better than just unwrapping
                    shell.error("No cdylib file found to copy")?;
                    std::process::exit(1);
                };

                let dest = arch_output_dir.join(file.file_name().unwrap());

                if is_fresh(file, &dest)? {
                    shell.status("Fresh", file)?;
                    continue;
                }

                shell.verbose(|shell| {
                    shell.status("Copying", format!("{file} -> {}", &dest.display()))
                })?;

                fs::copy(file, &dest)
                    .with_context(|| format!("failed to copy {file:?} over to {dest:?}"))?;

                filetime::set_file_mtime(
                    &dest,
                    FileTime::from_last_modification_time(
                        &dest
                            .metadata()
                            .with_context(|| format!("failed getting metadata for {dest:?}"))?,
                    ),
                )
                .with_context(|| format!("unable to update the modification time of {dest:?}"))?;
            }
        }
    }

    shell.verbose(|shell| {
        let duration = start_time.elapsed();
        let secs = duration.as_secs();
        let d = if secs >= 60 {
            format!("{}m {:02}s", secs / 60, secs % 60)
        } else {
            format!("{}.{:02}s", secs, duration.subsec_nanos() / 10_000_000)
        };
        let t = targets
            .iter()
            .map(|(target, _)| target.to_string())
            .collect::<Vec<_>>()
            .join(", ");

        shell.status("Finished", format!("targets ({t}) in {d}",))
    })?;

    Ok(())
}

pub struct TestUnit {
    executable: PathBuf,
    name: String,
    rel_path: String,
}

pub fn run_test(args: Vec<String>) -> anyhow::Result<()> {
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

/// Check whether the produced artifact is of use to use (has to be of type `cdylib`).
fn artifact_is_cdylib(artifact: &Artifact) -> bool {
    artifact.target.crate_types.contains(&CrateType::CDyLib)
}

// Check if the source file has changed and should be copied over to the destination path.
fn is_fresh(src: &Utf8Path, dest: &Path) -> anyhow::Result<bool> {
    if !dest.exists() {
        return Ok(false);
    }

    let src = src
        .metadata()
        .with_context(|| format!("failed getting metadata for {src:?}"))?;
    let dest = dest
        .metadata()
        .with_context(|| format!("failed getting metadata for {dest:?}"))?;

    // Only errors if modification time isn't available on the OS. Therefore,
    // we can't check it and always assume the file changed.
    let Some((src, dest)) = src.modified().ok().zip(dest.modified().ok()) else {
        return Ok(false);
    };

    Ok(src <= dest)
}
