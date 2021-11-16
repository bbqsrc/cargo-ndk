use std::{
    env,
    ffi::OsStr,
    path::{Path, PathBuf},
};

use cargo_metadata::MetadataCommand;
use gumdrop::Options;
use semver::Version;

use crate::meta::Target;

#[derive(Debug, Options)]
struct Args {
    #[options(help = "show help information")]
    help: bool,

    #[options(help = "print version")]
    version: bool,

    #[options(free, help = "args to be passed to cargo")]
    cargo_args: Vec<String>,

    #[options(
        help = "triple for the target(s)\n                           Supported: armeabi-v7a arm64-v8a x86 x86_64."
    )]
    target: Vec<Target>,

    #[options(
        meta = "DIR",
        help = "output to a jniLibs directory in the correct sub-directories"
    )]
    output_dir: Option<PathBuf>,

    #[options(help = "platform (also known as API level)")]
    platform: Option<u8>,

    #[options(
        no_short,
        help = "disable stripping debug symbols",
        default = "false"
    )]
    no_strip: bool,

    #[options(
        no_short,
        meta = "PATH",
        help = "path to Cargo.toml\n                           (limitations: https://github.com/rust-lang/cargo/issues/7856)"
    )]
    manifest_path: Option<PathBuf>,
}

fn highest_version_ndk_in_path(ndk_dir: &Path) -> Option<PathBuf> {
    if ndk_dir.exists() {
        std::fs::read_dir(&ndk_dir)
            .ok()?
            .flat_map(Result::ok)
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

fn derive_ndk_path() -> Option<PathBuf> {
    if let Some(path) = env::var_os("ANDROID_NDK").or_else(|| env::var_os("ANDROID_NDK_ROOT")) {
        return Some(PathBuf::from(path));
    }

    if let Some(path) = env::var_os("ANDROID_NDK_HOME").or_else(|| env::var_os("NDK_HOME")) {
        let path = PathBuf::from(path);
        return highest_version_ndk_in_path(&path).or(Some(path));
    };

    if let Some(sdk_path) = env::var_os("ANDROID_SDK_HOME") {
        let ndk_path = PathBuf::from(&sdk_path).join("ndk");
        if let Some(v) = highest_version_ndk_in_path(&ndk_path) {
            return Some(v);
        }
    };

    // Check Android Studio installed directories
    #[cfg(windows)]
    let base_dir = pathos::user::local_dir().unwrap();
    #[cfg(target_os = "linux")]
    let base_dir = pathos::user::data_dir().unwrap();
    #[cfg(target_os = "macos")]
    let base_dir = pathos::user::home_dir().unwrap().join("Library");

    let ndk_dir = base_dir.join("Android").join("sdk").join("ndk");
    log::trace!("Default NDK dir: {:?}", &ndk_dir);
    highest_version_ndk_in_path(&ndk_dir)
}

fn print_usage() {
    println!("cargo-ndk -- Brendan Molloy <https://github.com/bbqsrc/cargo-ndk>\n\nUsage: cargo ndk [OPTIONS] <CARGO_ARGS>\n");
    println!("{}", Args::usage());
}

pub(crate) fn run(args: Vec<String>) {
    log::trace!("Args: {:?}", args);

    if args.is_empty() || args.contains(&"-h".into()) || args.contains(&"--help".into()) {
        print_usage();

        std::process::exit(0);
    }

    let is_release = args.contains(&"--release".into());
    log::trace!("is_release: {}", is_release);

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
            log::error!("{}", e);
            std::process::exit(2);
        }
    };

    if args.cargo_args.is_empty() {
        log::error!("No args found to pass to cargo!");
        log::error!("You still need to specify build arguments to cargo to achieve anything. :)");
        std::process::exit(1);
    }

    let metadata = match MetadataCommand::new().exec() {
        Ok(v) => v,
        Err(e) => {
            log::error!("Failed to load Cargo.toml in current directory.");
            log::error!("{}", e);
            std::process::exit(1);
        }
    };

    // We used to check for NDK_HOME, so we'll keep doing that. But we'll also try ANDROID_NDK_HOME
    // and $ANDROID_SDK_HOME/ndk as this is how Android Studio configures the world
    let ndk_home = match derive_ndk_path() {
        Some(v) => {
            log::info!("Using NDK at path: {}", v.display());
            v
        }
        None => {
            log::error!("Could not find any NDK.");
            log::error!(
                "Set the environment ANDROID_NDK_HOME to your NDK installation's root directory,\nor install the NDK using Android Studio."
            );
            return;
        }
    };
    let working_dir = std::env::current_dir().expect("current directory could not be resolved");
    let working_dir_cargo = working_dir.join("Cargo.toml");
    let cargo_manifest = args.manifest_path.as_ref().unwrap_or(&working_dir_cargo);
    let config = match crate::meta::config(cargo_manifest, is_release) {
        Ok(v) => v,
        Err(e) => {
            log::error!("Failed loading manifest: {}", e);
            std::process::exit(1);
        }
    };

    let cmake_toolchain_path = ndk_home
        .join("build")
        .join("cmake")
        .join("android.toolchain.cmake");

    log::debug!(
        "Exporting CARGO_NDK_CMAKE_TOOLCHAIN_PATH = {:?}",
        &cmake_toolchain_path
    );
    std::env::set_var("CARGO_NDK_CMAKE_TOOLCHAIN_PATH", cmake_toolchain_path);

    // Try command line, then config. Config falls back to defaults in any case.
    let targets = if !args.target.is_empty() {
        args.target
    } else {
        config.targets
    };

    let platform = args.platform.unwrap_or(config.platform);

    if let Some(output_dir) = args.output_dir.as_ref() {
        std::fs::create_dir_all(output_dir).expect("failed to create output directory");
    }

    log::info!("NDK API level: {}", platform);
    std::env::set_var("CARGO_NDK_ANDROID_PLATFORM", platform.to_string());
    log::info!(
        "Building targets: {}",
        targets
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );

    for target in targets.iter() {
        let triple = target.triple();
        log::info!("Building {} ({})", &target, &triple);

        log::debug!(
            "Exporting CARGO_NDK_ANDROID_TARGET = {:?}",
            &target.to_string()
        );
        std::env::set_var("CARGO_NDK_ANDROID_TARGET", target.to_string());

        let status = crate::cargo::run(
            &working_dir,
            &ndk_home,
            triple,
            platform,
            &args.cargo_args,
            cargo_manifest,
        );
        let code = status.code().unwrap_or(-1);

        if code != 0 {
            log::info!("If the build failed due to a missing target, you can run this command:");
            log::info!("");
            log::info!("    rustup target install {}", triple);
            std::process::exit(code);
        }
    }

    let out_dir = metadata.target_directory;

    if let Some(output_dir) = args.output_dir.as_ref() {
        log::info!("Copying libraries to {}...", &output_dir.display());

        for target in targets {
            log::trace!("Target: {:?}", &target);
            let arch_output_dir = output_dir.join(target.to_string());
            std::fs::create_dir_all(&arch_output_dir).unwrap();

            let dir =
                out_dir
                    .join(target.triple())
                    .join(if is_release { "release" } else { "debug" });

            log::trace!("Target path: {}", dir);

            let so_files = std::fs::read_dir(&dir)
                .ok()
                .unwrap()
                .flat_map(Result::ok)
                .map(|x| x.path())
                .filter(|x| x.extension() == Some(OsStr::new("so")))
                .collect::<Vec<_>>();

            if so_files.is_empty() {
                log::error!("No .so files found in path {:?}", dir);
                log::error!("Did you set the crate-type in Cargo.toml to include 'cdylib'?");
                log::error!("For more info, see <https://doc.rust-lang.org/cargo/reference/cargo-targets.html#library>.");
                std::process::exit(1);
            }

            for so_file in so_files {
                let dest = arch_output_dir.join(so_file.file_name().unwrap());
                log::info!("{} -> {}", &so_file.display(), dest.display());
                std::fs::copy(so_file, &dest).unwrap();

                if !args.no_strip {
                    let _ = crate::cargo::strip(&ndk_home, target.triple(), &dest);
                }
            }
        }
    }
}
