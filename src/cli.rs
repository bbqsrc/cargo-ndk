use std::{
    env,
    ffi::{OsStr, OsString},
    fmt::Display,
    fs,
    io::{self, ErrorKind},
    panic::{self, PanicInfo},
    path::{Path, PathBuf},
    time::Instant,
};

use cargo_metadata::{semver::Version, MetadataCommand};
use gumdrop::Options;

use crate::{
    meta::Target,
    shell::{Shell, Verbosity},
};

#[derive(Debug, Options)]
struct Args {
    #[options(help = "show help information")]
    help: bool,

    #[options(long = "version", help = "print version")]
    version: bool,

    #[options(free, help = "args to be passed to cargo")]
    cargo_args: Vec<String>,

    #[options(
        meta = "DIR",
        help = "output to a jniLibs directory in the correct sub-directories"
    )]
    output_dir: Option<PathBuf>,

    #[options(help = "platform (also known as API level)")]
    platform: Option<u8>,

    #[options(no_short, help = "disable stripping debug symbols", default = "false")]
    no_strip: bool,

    #[options(no_short, meta = "PATH", help = "path to Cargo.toml")]
    manifest_path: Option<PathBuf>,

    #[options(
        no_short,
        help = "set bindgen-specific environment variables (BINDGEN_EXTRA_CLANG_ARGS_*) when building",
        default = "false"
    )]
    bindgen: bool,

    #[options(
        help = "Triples for the target(s). Additionally, Android target names are supported: armeabi-v7a arm64-v8a x86 x86_64"
    )]
    target: Vec<Target>,
}

fn highest_version_ndk_in_path(ndk_dir: &Path) -> Option<PathBuf> {
    if ndk_dir.exists() {
        fs::read_dir(ndk_dir)
            .ok()?
            .filter_map(Result::ok)
            .filter_map(|x| {
                let path = x.path();
                path.components()
                    .last()
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
                            "Environment variable `{} = {:#?}` doesn't match `{} = {:#?}`",
                            first_var, first_path, var, path
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

    // Check Android Studio installed directories
    let base_dir = find_base_dir();

    let ndk_dir = base_dir.join("Android").join("sdk").join("ndk");
    highest_version_ndk_in_path(&ndk_dir).map(|path| (path, "standard location".to_string()))
}

fn print_usage() {
    println!("cargo-ndk <https://github.com/bbqsrc/cargo-ndk>\n\nUsage: cargo ndk [OPTIONS] <CARGO_ARGS>\n");
    println!("{}", Args::usage());
}

fn find_base_dir() -> PathBuf {
    #[cfg(windows)]
    let base_dir = pathos::user::local_dir().unwrap().to_path_buf();
    #[cfg(target_os = "linux")]
    let base_dir = pathos::user::data_dir().unwrap().to_path_buf();
    #[cfg(target_os = "macos")]
    let base_dir = pathos::user::home_dir().unwrap().join("Library");

    base_dir
}

fn derive_ndk_version(path: &Path) -> anyhow::Result<Version> {
    let data = fs::read_to_string(path.join("source.properties"))?;
    for line in data.split('\n') {
        if line.starts_with("Pkg.Revision") {
            let mut chunks = line.split(" = ");
            let _ = chunks
                .next()
                .ok_or_else(|| io::Error::new(ErrorKind::Other, "No chunk"))?;
            let version = chunks
                .next()
                .ok_or_else(|| io::Error::new(ErrorKind::Other, "No chunk"))?;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BuildMode {
    Debug,
    Release,
    Profile(String),
}

impl Display for BuildMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            BuildMode::Debug => "debug",
            BuildMode::Release => "release",
            BuildMode::Profile(x) => x,
        })
    }
}

impl From<&str> for BuildMode {
    fn from(profile: &str) -> Self {
        match profile {
            "dev" => BuildMode::Debug,
            "release" => BuildMode::Release,
            _ => BuildMode::Profile(profile.to_string()),
        }
    }
}

fn is_supported_rustc_version() -> bool {
    version_check::is_min_version("1.68.0").unwrap_or_default()
}

fn panic_hook(info: &PanicInfo<'_>) {
    fn _attempt_shell(lines: &[String]) -> Result<(), anyhow::Error> {
        let mut shell = Shell::new();
        shell.error("cargo-ndk panicked! Generating report...")?;
        for line in lines {
            println!("{}", line);
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
        .map(|(x, y)| format!("{}={:?}", x, y))
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
            eprintln!("{}", line);
        }
    }
}

pub(crate) fn run(args: Vec<String>) -> anyhow::Result<()> {
    if args.is_empty() || args.contains(&"-h".into()) || args.contains(&"--help".into()) {
        print_usage();
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
        panic::set_hook(Box::new(panic_hook));
    }

    if !is_supported_rustc_version() {
        shell.error("Rust compiler is too old and not supported by cargo-ndk.")?;
        shell.note("Upgrade Rust to at least v1.68.0.")?;
        std::process::exit(1);
    }

    let build_mode = if args.contains(&"--release".into()) {
        BuildMode::Release
    } else if let Some(i) = args.iter().position(|x| x == "--profile") {
        args.get(i + 1)
            .map(|p| BuildMode::from(p.as_str()))
            .unwrap_or(BuildMode::Debug)
    } else {
        args.iter()
            .find_map(|a| a.strip_prefix("--profile=").map(BuildMode::from))
            .unwrap_or(BuildMode::Debug)
    };

    let args = match Args::parse_args(&args, gumdrop::ParsingStyle::StopAtFirstFree) {
        Ok(args) if args.help => {
            print_usage();
            std::process::exit(0);
        }
        Ok(args) if args.version => {
            println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        }
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
                .position(|arg| arg == "-p")
                .and_then(|idx| cargo_args.get(idx + 1))
            {
                let selected_package = metadata
                    .packages
                    .iter()
                    .find(|p| &p.name == selected_package)
                    .unwrap_or_else(|| panic!("unknown package: {selected_package}"));

                Some(selected_package.manifest_path.as_std_path().to_path_buf())
            } else {
                None
            }
        })
        .unwrap_or_else(|| working_dir.join("Cargo.toml"));

    let config = match crate::meta::config(&cargo_manifest, &build_mode) {
        Ok(v) => v,
        Err(e) => {
            shell.error("Failed loading manifest")?;
            shell.error(e)?;
            std::process::exit(1);
        }
    };

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
    env::set_var("CARGO_NDK_CMAKE_TOOLCHAIN_PATH", cmake_toolchain_path);

    // Try command line, then config. Config falls back to defaults in any case.
    let targets = if !args.target.is_empty() {
        args.target
    } else {
        config.targets
    };

    let platform = args.platform.unwrap_or(config.platform);

    if let Some(output_dir) = args.output_dir.as_ref() {
        fs::create_dir_all(output_dir).expect("failed to create output directory");
    }

    shell.verbose(|shell| {
        shell.status_with_color(
            "Setting",
            format!("Android SDK platform level to {}", platform),
            termcolor::Color::Cyan,
        )
    })?;

    env::set_var("CARGO_NDK_ANDROID_PLATFORM", platform.to_string());
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

    for target in &targets {
        let triple = target.triple();
        shell.status("Building", format!("{} ({})", &target, &triple))?;

        shell.very_verbose(|shell| {
            shell.status_with_color(
                "Exporting",
                format!("CARGO_NDK_ANDROID_TARGET={:?}", &target.to_string()),
                termcolor::Color::Cyan,
            )
        })?;

        env::set_var("CARGO_NDK_ANDROID_TARGET", target.to_string());

        let status = crate::cargo::run(
            &mut shell,
            &working_dir,
            &ndk_home,
            &ndk_version,
            triple,
            platform,
            &args.cargo_args,
            &cargo_manifest,
            args.bindgen,
            &out_dir,
        );
        let code = status.code().unwrap_or(-1);

        if code != 0 {
            shell.note("If the build failed due to a missing target, you can run this command:")?;
            shell.note("")?;
            shell.note(format!("    rustup target install {}", triple))?;
            std::process::exit(code);
        }
    }

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

        for target in targets.iter() {
            let arch_output_dir = output_dir.join(target.to_string());
            fs::create_dir_all(&arch_output_dir).unwrap();

            let dir = out_dir.join(target.triple()).join(build_mode.to_string());

            let so_files = match fs::read_dir(&dir) {
                Ok(dir) => dir
                    .filter_map(Result::ok)
                    .map(|x| x.path())
                    .filter(|x| x.extension() == Some(OsStr::new("so")))
                    .collect::<Vec<_>>(),
                Err(e) => {
                    shell.error(format!("Could not read directory: {:?}", dir))?;
                    shell.error(e)?;
                    std::process::exit(1);
                }
            };

            if so_files.is_empty() {
                shell.error(format!("No .so files found in path {:?}", dir))?;
                shell.error("Did you set the crate-type in Cargo.toml to include 'cdylib'?")?;
                shell.error("For more info, see <https://doc.rust-lang.org/cargo/reference/cargo-targets.html#library>.")?;
                std::process::exit(1);
            }

            for so_file in so_files {
                let dest = arch_output_dir.join(so_file.file_name().unwrap());
                shell.verbose(|shell| {
                    shell.status(
                        "Copying",
                        format!(
                            "{} -> {}",
                            &dunce::canonicalize(&so_file).unwrap().display(),
                            &dest.display()
                        ),
                    )
                })?;
                fs::copy(so_file, &dest).unwrap();

                if !args.no_strip {
                    shell.verbose(|shell| {
                        shell.status(
                            "Stripping",
                            format!("{}", &dunce::canonicalize(&dest).unwrap().display()),
                        )
                    })?;
                    let _ = crate::cargo::strip(&ndk_home, &dest);
                }
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
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");

        shell.status("Finished", format!("targets ({t}) in {d}",))
    })?;

    Ok(())
}
