pub mod env;
pub mod runner;
pub mod test;

use std::{
    ffi::OsString,
    fs,
    panic::PanicHookInfo,
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

use anyhow::Context;
use cargo_metadata::{Artifact, CrateType, MetadataCommand};
use clap::Parser;
use filetime::FileTime;

use crate::{
    Ndk,
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

#[derive(Debug, Parser, Clone)]
#[command(name = "cargo-ndk")]
struct BuildArgs {
    /// Triples for the target. Can be Rust or Android target names (i.e. arm64-v8a)
    #[arg(short, long, env = "CARGO_NDK_TARGET", value_delimiter = ',')]
    target: Vec<Target>,

    /// Platform (also known as API level)
    #[arg(short = 'P', long, default_value_t = 21, env = "CARGO_NDK_PLATFORM")]
    platform: u8,

    /// Links Clang builtins library
    #[arg(long, default_value_t = false, env = "CARGO_NDK_LINK_BUILTINS")]
    link_builtins: bool,

    /// Links libc++_shared library
    #[arg(long, default_value_t = false, env = "CARGO_NDK_LINK_LIBCXX_SHARED")]
    link_libcxx_shared: bool,

    /// Output to a `jniLibs` directory in the correct sub-directories
    #[arg(short, long, value_name = "DIR", env = "CARGO_NDK_OUTPUT_PATH")]
    output_dir: Option<PathBuf>,

    /// Path to Cargo.toml
    #[arg(long, value_name = "PATH")]
    manifest_path: Option<PathBuf>,

    /// Args to be passed to cargo
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    cargo_args: Vec<String>,
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
        if let Some(path) = std::env::var_os(var) {
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

pub(crate) fn discover_ndk(shell: &mut Shell) -> anyhow::Result<Option<Ndk>> {
    let mut warning_error = None;
    let ndk = Ndk::discover_with_warning(|warning| {
        if warning_error.is_none() {
            warning_error = shell.warn(warning).err();
        }
    })?;

    if let Some(error) = warning_error {
        return Err(error);
    }

    Ok(ndk)
}

pub(crate) fn cargo_ndk_executable_path() -> anyhow::Result<PathBuf> {
    sibling_executable("cargo-ndk")
}

pub(crate) fn cargo_ndk_runner_path() -> anyhow::Result<PathBuf> {
    sibling_executable("cargo-ndk-runner")
}

fn sibling_executable(name: &str) -> anyhow::Result<PathBuf> {
    let current_executable = dunce::canonicalize(std::env::current_exe()?)?;
    let parent = current_executable
        .parent()
        .context("current executable has no parent directory")?;
    Ok(parent.join(name))
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

/// Parse arguments that can appear both before and after the cargo subcommand
fn parse_mixed_args<T>(args: Vec<String>) -> anyhow::Result<T>
where
    T: clap::Parser + Clone + clap::CommandFactory + HasCargoArgs,
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

trait StringVecExt {
    fn contains_str(&self, value: &str) -> bool;
}

impl StringVecExt for Vec<String> {
    fn contains_str(&self, value: &str) -> bool {
        self.iter().any(|s| s == value)
    }
}

fn init<T>(args: Vec<String>) -> anyhow::Result<(Shell, Vec<String>)>
where
    T: clap::CommandFactory,
{
    if std::env::var_os("CARGO_NDK_NO_PANIC_HOOK").is_none() {
        std::panic::set_hook(Box::new(panic_hook));
    }

    if args.contains_str("--help") {
        T::command().print_long_help().unwrap();
        std::process::exit(0);
    }

    if args.contains_str("-h") {
        T::command().print_help().unwrap();
        std::process::exit(0);
    }

    if args.contains_str("--version") || args.contains_str("-V") {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }

    let verbosity = if args.contains_str("-q") {
        Verbosity::Quiet
    } else if args.contains_str("-vv") {
        Verbosity::VeryVerbose
    } else if args.contains_str("-v") || args.contains_str("--verbose") {
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

    shell.verbose(|shell| {
        shell.status_with_color(
            "Using",
            format!("cargo-ndk v{}", env!("CARGO_PKG_VERSION")),
            termcolor::Color::Cyan,
        )
    })?;

    if !is_supported_rustc_version() {
        shell.error("Rust compiler is too old and not supported by cargo-ndk.")?;
        shell.note("Upgrade Rust to at least v1.68.0.")?;
        std::process::exit(1);
    }

    Ok((shell, args))
}

pub fn run(args: Vec<String>) -> anyhow::Result<()> {
    // Check for help/version before parsing to avoid required arg errors
    let (mut shell, args) = init::<BuildArgs>(args)?;

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
    let ndk = match discover_ndk(&mut shell) {
        Ok(Some(ndk)) => ndk,
        Ok(None) => {
            shell.error("Could not find any NDK.")?;
            shell.note(
                "Set the environment ANDROID_NDK_HOME to your NDK installation's root directory,\nor install the NDK using Android Studio."
            )?;
            std::process::exit(1);
        }
        Err(error) => {
            shell.error("Failed to detect the Android NDK.")?;
            shell.error(error)?;
            std::process::exit(1);
        }
    };

    shell.verbose(|shell| {
        shell.status_with_color(
            "Detected",
            format!(
                "NDK v{} ({}) [{}]",
                ndk.version(),
                ndk.path().display(),
                ndk.source()
            ),
            termcolor::Color::Cyan,
        )
    })?;

    let working_dir = std::env::current_dir().expect("current directory could not be resolved");

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

    let cargo_ndk_path = cargo_ndk_executable_path()?;
    let cargo_ndk_runner_path = cargo_ndk_runner_path()?;
    let cmake_toolchain_path = ndk.cmake_toolchain_path();

    shell.very_verbose(|shell| {
        shell.status_with_color(
            "Exporting",
            format!("CARGO_NDK_CMAKE_TOOLCHAIN_PATH={:?}", cmake_toolchain_path),
            termcolor::Color::Cyan,
        )
    })?;
    unsafe {
        std::env::set_var("CARGO_NDK_CMAKE_TOOLCHAIN_PATH", cmake_toolchain_path);
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
        std::env::set_var("CARGO_NDK_ANDROID_PLATFORM", platform.to_string());
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
            shell.status("Building", format!("{} ({})", target, triple))?;

            shell.very_verbose(|shell| {
                shell.status_with_color(
                    "Exporting",
                    format!("CARGO_NDK_ANDROID_PLATFORM={:?}", target.to_string()),
                    termcolor::Color::Cyan,
                )
            })?;
            unsafe {
                std::env::set_var("CARGO_NDK_ANDROID_PLATFORM", target.to_string());
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
                std::env::set_var("ANDROID_PLATFORM", platform.to_string());
            }

            // Set ANDROID_ABI using the Android-specific target name
            let android_abi = target.to_string();
            shell.very_verbose(|shell| {
                shell.status_with_color(
                    "Exporting",
                    format!("ANDROID_ABI={:?}", android_abi),
                    termcolor::Color::Cyan,
                )
            })?;
            unsafe {
                std::env::set_var("ANDROID_ABI", &android_abi);
            }

            let (status, artifacts) = crate::cargo::run(
                &mut shell,
                &working_dir,
                &ndk,
                target,
                platform,
                &cargo_ndk_path,
                &cargo_ndk_runner_path,
                args.link_builtins,
                args.link_libcxx_shared,
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

    // Determine the package being built so we only copy its artifacts.
    let current_package_id = match metadata
        .packages
        .iter()
        .find(|p| p.manifest_path.as_std_path() == cargo_manifest)
    {
        Some(p) => p.id.clone(),
        None => {
            shell.error("Could not determine current package from manifest")?;
            std::process::exit(1);
        }
    };

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

            if args.link_libcxx_shared {
                let cargo_ndk_sysroot_path = ndk.sysroot();
                let cargo_ndk_sysroot_target = target.sysroot_target();
                let cargo_ndk_sysroot_libs_path = cargo_ndk_sysroot_path
                    .join("usr")
                    .join("lib")
                    .join(cargo_ndk_sysroot_target);
                let dest = arch_output_dir.join("libc++_shared.so");

                if is_fresh(&cargo_ndk_sysroot_libs_path, &dest)? {
                    shell.verbose(|shell| shell.status("Fresh", "libc++_shared.so"))?;
                } else {
                    shell.verbose(|shell| {
                        shell.status("Copying", format!("libc++_shared.so -> {}", dest.display()))
                    })?;

                    fs::copy(cargo_ndk_sysroot_libs_path.join("libc++_shared.so"), &dest)
                        .with_context(|| {
                            format!(
                                "failed to copy libc++_shared.so from {} to {}",
                                cargo_ndk_sysroot_libs_path.display(),
                                output_dir.display()
                            )
                        })?;
                }
            }

            for artifact in copyable_cdylib_artifacts(artifacts, &current_package_id) {
                let Some(file) = artifact
                    .filenames
                    .iter()
                    .find(|name| name.extension() == Some("so"))
                else {
                    shell.error(format!(
                        "No cdylib file found to copy in\n{:#?}",
                        artifact.filenames
                    ))?;
                    std::process::exit(1);
                };
                let dest = arch_output_dir.join(file.file_name().unwrap());

                if is_fresh(file.as_std_path(), &dest)? {
                    shell.verbose(|shell| shell.status("Fresh", file))?;
                    continue;
                }

                shell.verbose(|shell| {
                    shell.status("Copying", format!("{file} -> {}", dest.display()))
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

/// Check whether the produced artifact is of use to use (has to be of type `cdylib`).
#[inline]
fn artifact_is_cdylib(artifact: &Artifact) -> bool {
    artifact.target.crate_types.contains(&CrateType::CDyLib)
}

fn copyable_cdylib_artifacts<'a>(
    artifacts: &'a [Artifact],
    current_package_id: &'a cargo_metadata::PackageId,
) -> impl Iterator<Item = &'a Artifact> + 'a {
    artifacts
        .iter()
        .filter(|a| artifact_is_cdylib(a))
        .filter(move |a| &a.package_id == current_package_id)
}

/// Check if the source file has changed and should be copied over to the destination path.
#[inline]
fn is_fresh(src: &Path, dest: &Path) -> anyhow::Result<bool> {
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

#[cfg(test)]
mod tests {
    use cargo_metadata::Artifact;
    use serde_json::json;

    use super::copyable_cdylib_artifacts;

    fn artifact(package_id: &str, crate_types: &[&str], filenames: &[&str]) -> Artifact {
        serde_json::from_value(json!({
            "package_id": package_id,
            "manifest_path": format!("/workspace/{package_id}/Cargo.toml"),
            "target": {
                "kind": ["lib"],
                "crate_types": crate_types,
                "name": package_id,
                "src_path": format!("/workspace/{package_id}/src/lib.rs"),
                "edition": "2024",
                "doc": true,
                "doctest": true,
                "test": true
            },
            "profile": {
                "opt_level": "0",
                "debuginfo": 0,
                "debug_assertions": true,
                "overflow_checks": true,
                "test": false
            },
            "features": [],
            "filenames": filenames,
            "executable": null,
            "fresh": false
        }))
        .expect("test artifact should deserialize")
    }

    #[test]
    fn copyable_cdylib_artifacts_ignores_workspace_cdylibs_from_other_packages() {
        let current = artifact(
            "path+file:///workspace/app#0.1.0",
            &["cdylib"],
            &["target/libapp.so"],
        );
        let sibling_without_so = artifact(
            "path+file:///workspace/helper#0.1.0",
            &["cdylib"],
            &["target/helper.dll"],
        );
        let sibling_with_so = artifact(
            "path+file:///workspace/other#0.1.0",
            &["cdylib"],
            &["target/libother.so"],
        );
        let current_package_id = current.package_id.clone();
        let artifacts = vec![sibling_without_so, sibling_with_so, current];

        let selected = copyable_cdylib_artifacts(&artifacts, &current_package_id)
            .map(|artifact| artifact.package_id.repr.as_str())
            .collect::<Vec<_>>();

        assert_eq!(selected, vec![current_package_id.repr.as_str()]);
    }
}
