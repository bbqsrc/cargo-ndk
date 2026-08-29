use std::{
    collections::BTreeMap,
    env,
    ffi::{OsStr, OsString},
    io::BufReader,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, Result};
use cargo_metadata::{Artifact, Message};

use crate::{ApiLevel, Target, ndk::Ndk, shell::Shell};

fn cargo_env_target_cfg(triple: &str, key: &str) -> String {
    format!("CARGO_TARGET_{}_{}", triple.replace('-', "_"), key).to_uppercase()
}

#[inline]
fn env_var_with_key(key: String) -> Option<(String, String)> {
    env::var(&key).map(|value| (key, value)).ok()
}

// Derived from getenv_with_target_prefixes in `cc` crate.
fn cc_env(var_base: &str, triple: &str) -> (String, Option<String>) {
    let triple_u = triple.replace('-', "_");
    let most_specific_key = format!("{var_base}_{triple}");

    env_var_with_key(most_specific_key.to_string())
        .or_else(|| env_var_with_key(format!("{var_base}_{triple_u}")))
        .or_else(|| env_var_with_key(format!("TARGET_{var_base}")))
        .or_else(|| env_var_with_key(var_base.to_string()))
        .map(|(key, value)| (key, Some(value)))
        .unwrap_or_else(|| (most_specific_key, None))
}

// {}/toolchains/llvm/prebuilt/{ARCH}/lib/clang/{clang_version}/lib/linux
#[inline]
fn clang_lib_path(ndk_home: &Path) -> Result<PathBuf> {
    let clang_folder: PathBuf = ndk_home
        .join("toolchains")
        .join("llvm")
        .join("prebuilt")
        .join(crate::ARCH)
        .join("lib")
        .join("clang");

    let clang_lib_version = std::fs::read_dir(&clang_folder)
        .context("unable to get clang target directory")?
        .filter_map(|a| a.ok())
        .max_by(|a, b| a.file_name().cmp(&b.file_name()))
        .context("unable to get clang target")?
        .path();

    Ok(clang_folder
        .join(clang_lib_version)
        .join("lib")
        .join("linux"))
}

#[inline]
fn contains_libclang(path: &Path) -> bool {
    std::fs::read_dir(path)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(|entry| entry.ok()))
        .any(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("libclang."))
        })
}

#[inline]
fn libclang_search_suffixes() -> &'static [&'static str] {
    if cfg!(target_env = "musl") {
        &["musl/lib", "lib", "lib64", "bin"]
    } else {
        &["lib", "lib64", "bin", "musl/lib"]
    }
}

#[inline]
fn libclang_path_with_suffixes(ndk_home: &Path, suffixes: &[&str]) -> Option<PathBuf> {
    let prebuilt_path = ndk_home
        .join("toolchains")
        .join("llvm")
        .join("prebuilt")
        .join(crate::ARCH);

    suffixes
        .iter()
        .map(|suffix| prebuilt_path.join(suffix))
        .find(|path| contains_libclang(path))
}

#[inline]
fn libclang_path(ndk_home: &Path) -> Option<PathBuf> {
    libclang_path_with_suffixes(ndk_home, libclang_search_suffixes())
}

const CARGO_NDK_SYSROOT_PATH_KEY: &str = "CARGO_NDK_SYSROOT_PATH";
const CARGO_NDK_SYSROOT_TARGET_KEY: &str = "CARGO_NDK_SYSROOT_TARGET";
const CARGO_NDK_SYSROOT_LIBS_PATH_KEY: &str = "CARGO_NDK_SYSROOT_LIBS_PATH";
const CARGO_NDK_NDK_VERSION_KEY: &str = "CARGO_NDK_NDK_VERSION";
const CARGO_NDK_CMAKE_TOOLCHAIN_PATH_KEY: &str = "CARGO_NDK_CMAKE_TOOLCHAIN_PATH";

/// Configuration for adapting a Cargo command to an Android NDK toolchain.
///
/// The default linker is the current executable. This makes an executable
/// embedding `cargo-ndk` able to act as its own linker wrapper by dispatching
/// linker invocations to [`crate::run_linker_wrapper`]. Use [`Self::with_linker`]
/// when the wrapper lives in a different executable.
#[derive(Debug, Clone)]
pub struct BuildConfig {
    ndk: Ndk,
    target: Target,
    api_level: ApiLevel,
    linker: Option<PathBuf>,
    runner: Option<PathBuf>,
    link_builtins: bool,
    link_cxx_shared: bool,
}

impl BuildConfig {
    /// Creates a build configuration for one Android target.
    pub fn new(ndk: Ndk, target: Target, api_level: impl Into<ApiLevel>) -> Self {
        Self {
            ndk,
            target,
            api_level: api_level.into(),
            linker: None,
            runner: None,
            link_builtins: false,
            link_cxx_shared: false,
        }
    }

    /// Sets the executable Cargo should invoke as the target linker.
    pub fn with_linker(mut self, linker: impl Into<PathBuf>) -> Self {
        self.linker = Some(linker.into());
        self
    }

    /// Sets an optional Cargo target runner, for example an adb-based runner.
    pub fn with_runner(mut self, runner: impl Into<PathBuf>) -> Self {
        self.runner = Some(runner.into());
        self
    }

    /// Enables linking compiler builtins from the NDK.
    pub fn with_link_builtins(mut self, enabled: bool) -> Self {
        self.link_builtins = enabled;
        self
    }

    /// Enables linking `libc++_shared`.
    pub fn with_link_cxx_shared(mut self, enabled: bool) -> Self {
        self.link_cxx_shared = enabled;
        self
    }

    /// Returns the generated environment without starting Cargo.
    pub fn environment(&self) -> Result<BuildEnvironment> {
        build_environment(self)
    }

    /// Applies the NDK environment to a Cargo command.
    pub fn configure_command(&self, command: &mut Command) -> Result<()> {
        self.environment()?.apply_to(command);
        Ok(())
    }

    /// Returns the target selected by this configuration.
    pub fn target(&self) -> Target {
        self.target
    }

    /// Returns the Android API level selected by this configuration.
    pub fn api_level(&self) -> ApiLevel {
        self.api_level
    }

    /// Returns the NDK used by this configuration.
    pub fn ndk(&self) -> &Ndk {
        &self.ndk
    }
}

/// Environment variables generated for one Android Cargo build.
#[derive(Debug, Clone)]
pub struct BuildEnvironment {
    variables: BTreeMap<String, OsString>,
}

impl BuildEnvironment {
    /// Gets a generated value by environment variable name.
    pub fn get(&self, key: &str) -> Option<&OsStr> {
        self.variables.get(key).map(OsString::as_os_str)
    }

    /// Iterates over generated environment variables without exposing storage ownership.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &OsStr)> {
        self.variables
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_os_str()))
    }

    /// Applies all generated variables to a command.
    pub fn apply_to(&self, command: &mut Command) {
        for (key, value) in &self.variables {
            command.env(key, value);
        }
    }

    pub(crate) fn into_map(self) -> BTreeMap<String, OsString> {
        self.variables
    }
}

fn build_environment(config: &BuildConfig) -> Result<BuildEnvironment> {
    config.ndk.ensure_supported()?;

    let ndk_home = config.ndk.path();
    let ndk_version = config.ndk.version();
    let target = config.target;
    let triple = target.triple();
    let clang_target = target.clang_target(config.api_level);
    let linker = match &config.linker {
        Some(linker) => linker.clone(),
        None => env::current_exe().context("failed to resolve the current executable")?,
    };

    // Environment variables for the `cc` crate
    let (cc_key, _) = cc_env("CC", triple);
    let (cflags_key, cflags_value) = cc_env("CFLAGS", triple);
    let (cxx_key, _) = cc_env("CXX", triple);
    let (cxxflags_key, cxxflags_value) = cc_env("CXXFLAGS", triple);
    let (ar_key, _) = cc_env("AR", triple);
    let (ranlib_key, _) = cc_env("RANLIB", triple);

    // Environment variables for cargo
    let cargo_ar_key = cargo_env_target_cfg(triple, "ar");
    let cargo_linker_key = cargo_env_target_cfg(triple, "linker");
    let bindgen_clang_args_key = format!("BINDGEN_EXTRA_CLANG_ARGS_{}", triple.replace('-', "_"));

    let target_cc = config.ndk.tool("clang");
    let target_cflags = match cflags_value {
        Some(v) => format!("{clang_target} {v}"),
        None => clang_target.to_string(),
    };
    let target_cxx = config.ndk.tool("clang++");
    let target_cxxflags = match cxxflags_value {
        Some(v) => format!("{clang_target} {v}"),
        None => clang_target.to_string(),
    };
    let cargo_ndk_sysroot_path = config.ndk.sysroot();
    let cargo_ndk_sysroot_target = target.sysroot_target();
    let cargo_ndk_sysroot_libs_path = cargo_ndk_sysroot_path
        .join("usr")
        .join("lib")
        .join(cargo_ndk_sysroot_target);
    let target_ar = config.ndk.tool("llvm-ar");
    let target_ranlib = config.ndk.tool("llvm-ranlib");

    let extra_include = format!(
        "{}/usr/include/{}",
        cargo_ndk_sysroot_path.display(),
        cargo_ndk_sysroot_target
    );

    let mut envs = [
        (cc_key, target_cc.clone().into_os_string()),
        (cflags_key, target_cflags.into()),
        (cxx_key, target_cxx.into_os_string()),
        (cxxflags_key, target_cxxflags.into()),
        (ar_key, target_ar.clone().into()),
        (ranlib_key, target_ranlib.into_os_string()),
        (cargo_ar_key, target_ar.into_os_string()),
        (cargo_linker_key, linker.into_os_string()),
        (
            CARGO_NDK_SYSROOT_PATH_KEY.to_string(),
            cargo_ndk_sysroot_path.clone().into_os_string(),
        ),
        (
            CARGO_NDK_SYSROOT_LIBS_PATH_KEY.to_string(),
            cargo_ndk_sysroot_libs_path.into_os_string(),
        ),
        (
            CARGO_NDK_SYSROOT_TARGET_KEY.to_string(),
            cargo_ndk_sysroot_target.into(),
        ),
        (
            "CARGO_NDK_ANDROID_PLATFORM".to_string(),
            config.api_level.to_string().into(),
        ),
        (
            CARGO_NDK_NDK_VERSION_KEY.to_string(),
            ndk_version.to_string().into(),
        ),
        (
            CARGO_NDK_CMAKE_TOOLCHAIN_PATH_KEY.to_string(),
            config.ndk.cmake_toolchain_path().into_os_string(),
        ),
        (
            "ANDROID_PLATFORM".to_string(),
            config.api_level.to_string().into(),
        ),
        ("ANDROID_ABI".to_string(), target.abi().into()),
        // https://github.com/KyleMayes/clang-sys?tab=readme-ov-file#environment-variables
        ("CLANG_PATH".into(), target_cc.clone().into()),
        ("_CARGO_NDK_LINK_TARGET".into(), clang_target.into()), // Recognized by main() so we know when we're acting as a wrapper
        ("_CARGO_NDK_LINK_CLANG".into(), target_cc.into()),
    ]
    .into_iter()
    .collect::<BTreeMap<String, OsString>>();

    if let Some(runner) = &config.runner {
        envs.insert(
            cargo_env_target_cfg(triple, "runner"),
            runner.clone().into_os_string(),
        );
    }

    if let Some(path) = libclang_path(ndk_home) {
        envs.insert("LIBCLANG_PATH".to_string(), path.into_os_string());
    }

    // cmd.arg(format!("-L{builtins_path}"));

    // // Extract arch from target triple (e.g., "aarch64-linux-android21" -> "aarch64")
    // let arch = std::env::var("CARGO_NDK_SYSROOT_TARGET")
    //     .expect("cargo-ndk rustc linker: didn't find CARGO_NDK_SYSROOT_TARGET");
    // let arch = arch.split('-').next().unwrap();
    // cmd.arg(format!("-lclang_rt.builtins-{arch}-android"));

    let mut ldflags = vec![];

    if config.link_builtins {
        let builtins_path = clang_lib_path(ndk_home)?;

        ldflags.push(format!("-L{}", builtins_path.display()));
        ldflags.push(format!(
            "-lclang_rt.builtins-{}-android",
            target.sysroot_target().split('-').next().unwrap()
        ));
    }

    if config.link_cxx_shared {
        ldflags.push("-lc++_shared".to_string());
    }

    // 64-bit android requires 16kb pages now. It is the default for NDK r28+.
    // https://developer.android.com/guide/practices/page-sizes
    if target.is_64_bit() {
        if ndk_version.major() <= 27 {
            ldflags.push("-Wl,-z,max-page-size=16384".to_string());
        }

        if ndk_version.major() <= 22 {
            ldflags.push("-Wl,-z,common-page-size=16384".to_string());
        }
    }

    if !ldflags.is_empty() {
        let builtins_path = ldflags.join("\x1f");
        envs.insert("_CARGO_NDK_LDFLAGS".to_string(), builtins_path.into());
    }

    if env::var("MSYSTEM").is_ok() || env::var("CYGWIN").is_ok() {
        envs = envs
            .into_iter()
            .map(|(k, v)| {
                (
                    k,
                    OsString::from(v.into_string().unwrap().replace('\\', "/")),
                )
            })
            .collect();
    }

    let bindgen_args = format!(
        "--sysroot={} -I{}",
        cargo_ndk_sysroot_path.display(),
        extra_include
    );
    let bindgen_clang_args = bindgen_args.replace('\\', "/");

    envs.insert(
        bindgen_clang_args_key.to_string(),
        bindgen_clang_args.into(),
    );

    Ok(BuildEnvironment { variables: envs })
}

/// Note: considering that there is an upstream quoting bug in the clang .cmd
/// wrappers on Windows we intentionally avoid any wrapper scripts and
/// instead pass a `--target=<triple><api_level>` argument to clang by using
/// the configured executable as a linker wrapper.
///
/// See: https://github.com/android/ndk/issues/1856
///
/// Note: it's not possible to pass `-Clink-arg=` arguments via
/// CARGO_ENCODED_RUSTFLAGS because that could trample rustflags that are
/// configured for the project and there's no practical way to read all
/// user-configured rustflags from outside of cargo itself.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run(
    shell: &mut Shell,
    dir: &Path,
    ndk: &Ndk,
    target: Target,
    platform: u8,
    linker: &Path,
    runner: &Path,
    link_builtins: bool,
    link_cxx_shared: bool,
    cargo_args: &[String],
    cargo_manifest: &Path,
) -> Result<(std::process::ExitStatus, Vec<Artifact>)> {
    let cargo_args: Vec<OsString> = cargo_args.iter().map(Into::into).collect();
    let triple = target.triple();
    let cargo_bin = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut cargo_cmd = Command::new(&cargo_bin);
    let config = BuildConfig::new(ndk.clone(), target, ApiLevel::new(platform))
        .with_linker(linker.to_path_buf())
        .with_runner(runner.to_path_buf())
        .with_link_builtins(link_builtins)
        .with_link_cxx_shared(link_cxx_shared);
    let envs = match config.environment() {
        Ok(envs) => envs,
        Err(error) => {
            shell.error(error)?;
            std::process::exit(1);
        }
    };

    shell
        .very_verbose(|shell| {
            for (k, v) in envs.iter() {
                shell.status_with_color(
                    "Exporting",
                    format!("{k}={v:?}"),
                    termcolor::Color::Cyan,
                )?;
            }

            shell.status_with_color(
                "Invoking",
                format!("cargo ({cargo_bin}) with args: {cargo_args:?}"),
                termcolor::Color::Cyan,
            )
        })
        .unwrap();

    cargo_cmd.current_dir(dir);
    envs.apply_to(&mut cargo_cmd);

    let cargo_args = if !cargo_args.is_empty() && cargo_args[0].as_os_str() == "--" {
        &cargo_args[1..]
    } else {
        &cargo_args[..]
    };

    // Handle the case where `--` is sent to the subcommand
    let mid = cargo_args
        .iter()
        .position(|arg| arg == "--")
        .unwrap_or(cargo_args.len());
    let (cargo_args, subcommand_args) = cargo_args.split_at(mid);
    let mut cargo_args = cargo_args.to_vec();

    append_target_args(&mut cargo_args, dir, cargo_manifest, triple);

    cargo_cmd.args(&cargo_args);

    if !subcommand_args.is_empty() {
        cargo_cmd.args(subcommand_args);
    }

    let mut child = cargo_cmd
        .stdin(Stdio::inherit())
        .stderr(Stdio::inherit())
        .stdout(Stdio::piped())
        .spawn()
        .context("failed spawning cargo process")?;

    let reader = BufReader::new(child.stdout.take().context("no stdout available")?);
    let mut artifacts = Vec::new();

    for msg in Message::parse_stream(reader) {
        match msg? {
            Message::CompilerArtifact(artifact) => artifacts.push(artifact),
            Message::CompilerMessage(msg) => println!("{msg}"),
            Message::TextLine(line) => println!("{line}"),
            _ => {}
        }
    }

    let status = child.wait().context("cargo crashed")?;

    Ok((status, artifacts))
}

fn append_target_args(
    cargo_args: &mut Vec<OsString>,
    dir: &Path,
    cargo_manifest: &Path,
    triple: &str,
) {
    if cargo_manifest
        .parent()
        .is_some_and(|manifest_dir| manifest_dir != dir)
    {
        cargo_args.push("--manifest-path".into());
        cargo_args.push(cargo_manifest.into());
    }

    cargo_args.push("--target".into());
    cargo_args.push(triple.into());

    cargo_args.push("--message-format".into());
    cargo_args.push("json-render-diagnostics".into());
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::{OsStr, OsString},
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::{ARCH, ApiLevel, BuildConfig, Ndk, Target};

    use super::{
        append_target_args, libclang_path, libclang_path_with_suffixes, libclang_search_suffixes,
    };

    fn temp_ndk_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("cargo-ndk-{name}-{nanos}"));
        fs::remove_dir_all(&path).ok();
        path
    }

    fn create_libclang_file(ndk_home: &Path, suffix: &str) -> PathBuf {
        let path = ndk_home
            .join("toolchains")
            .join("llvm")
            .join("prebuilt")
            .join(ARCH)
            .join(suffix);
        fs::create_dir_all(&path).expect("failed to create fake libclang directory");
        fs::write(path.join("libclang.so"), "").expect("failed to write fake libclang");
        path
    }

    fn create_ndk(ndk_home: &Path) -> Ndk {
        fs::create_dir_all(ndk_home).expect("failed to create fake NDK directory");
        fs::write(
            ndk_home.join("source.properties"),
            "Pkg.Revision = 28.0.12433566\n",
        )
        .expect("failed to write fake NDK metadata");
        Ndk::from_path(ndk_home).expect("fake NDK should load")
    }

    #[test]
    fn build_env_exports_android_platform_and_abi() {
        let ndk_home = temp_ndk_path("build-env-platform");
        let env = BuildConfig::new(create_ndk(&ndk_home), Target::Arm64V8a, ApiLevel::new(28))
            .with_linker("/cargo-ndk")
            .environment()
            .expect("environment should be generated");

        assert_eq!(
            env.get("CARGO_NDK_ANDROID_PLATFORM"),
            Some(OsStr::new("28"))
        );
        assert_eq!(
            env.get("CARGO_NDK_NDK_VERSION"),
            Some(OsStr::new("28.0.12433566"))
        );
        assert_eq!(env.get("ANDROID_PLATFORM"), Some(OsStr::new("28")));
        assert_eq!(env.get("ANDROID_ABI"), Some(OsStr::new("arm64-v8a")));

        fs::remove_dir_all(ndk_home).ok();
    }

    #[test]
    fn target_args_skip_manifest_path_for_current_package() {
        let mut args = vec![OsString::from("build")];

        append_target_args(
            &mut args,
            Path::new("/workspace/app"),
            Path::new("/workspace/app/Cargo.toml"),
            "aarch64-linux-android",
        );

        assert_eq!(
            args,
            vec![
                "build",
                "--target",
                "aarch64-linux-android",
                "--message-format",
                "json-render-diagnostics",
            ]
        );
    }

    #[test]
    fn target_args_include_manifest_path_for_different_package() {
        let mut args = vec![OsString::from("build")];

        append_target_args(
            &mut args,
            Path::new("/workspace"),
            Path::new("/workspace/member/Cargo.toml"),
            "aarch64-linux-android",
        );

        assert_eq!(
            args,
            vec![
                "build",
                "--manifest-path",
                "/workspace/member/Cargo.toml",
                "--target",
                "aarch64-linux-android",
                "--message-format",
                "json-render-diagnostics",
            ]
        );
    }

    #[test]
    fn libclang_path_prefers_host_ndk_layout() {
        let ndk_home = temp_ndk_path("host-libclang");
        let host_path = create_libclang_file(&ndk_home, "lib");
        let _fallback_path = create_libclang_file(&ndk_home, "musl/lib");

        assert_eq!(libclang_path(&ndk_home).unwrap(), host_path);

        fs::remove_dir_all(ndk_home).ok();
    }

    #[test]
    fn libclang_path_prefers_musl_layout_for_musl_hosts() {
        let ndk_home = temp_ndk_path("musl-host-libclang");
        let _host_path = create_libclang_file(&ndk_home, "lib");
        let musl_path = create_libclang_file(&ndk_home, "musl/lib");

        assert_eq!(
            libclang_path_with_suffixes(&ndk_home, &["musl/lib", "lib", "lib64", "bin"]),
            Some(musl_path)
        );

        fs::remove_dir_all(ndk_home).ok();
    }

    #[test]
    fn libclang_path_falls_back_to_lib64_ndk_layout() {
        let ndk_home = temp_ndk_path("lib64-libclang");
        let fallback_path = create_libclang_file(&ndk_home, "lib64");

        assert_eq!(libclang_path(&ndk_home).unwrap(), fallback_path);

        fs::remove_dir_all(ndk_home).ok();
    }

    #[test]
    fn libclang_path_uses_musl_layout_as_last_resort() {
        let ndk_home = temp_ndk_path("musl-libclang");
        let fallback_path = create_libclang_file(&ndk_home, "musl/lib");

        assert_eq!(libclang_path(&ndk_home).unwrap(), fallback_path);

        fs::remove_dir_all(ndk_home).ok();
    }

    #[test]
    fn libclang_search_order_matches_host_abi() {
        let suffixes = libclang_search_suffixes();

        if cfg!(target_env = "musl") {
            assert_eq!(suffixes[0], "musl/lib");
        } else {
            assert_eq!(suffixes[0], "lib");
        }
    }

    #[test]
    fn build_env_exports_libclang_path_when_detected() {
        let ndk_home = temp_ndk_path("build-env-libclang");
        let ndk = create_ndk(&ndk_home);
        let libclang_path = create_libclang_file(&ndk_home, "musl/lib");

        let env = BuildConfig::new(ndk, Target::Arm64V8a, ApiLevel::new(28))
            .with_linker("/cargo-ndk")
            .environment()
            .expect("environment should be generated");

        assert_eq!(
            PathBuf::from(env.get("LIBCLANG_PATH").unwrap()),
            libclang_path
        );

        fs::remove_dir_all(ndk_home).ok();
    }
}
