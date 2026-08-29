use std::{
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use cargo_ndk::{AndroidTarget, ApiLevel, BuildConfig, BuildEnvironment, Ndk};

#[cfg(target_os = "macos")]
const HOST_ARCH: &str = "darwin-x86_64";
#[cfg(any(target_os = "linux", target_os = "android"))]
const HOST_ARCH: &str = "linux-x86_64";
#[cfg(target_os = "windows")]
const HOST_ARCH: &str = "windows-x86_64";

fn temp_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("cargo-ndk-library-{name}-{nanos}"))
}

fn fake_ndk(path: &Path, version: &str) -> Ndk {
    fs::create_dir_all(path).expect("failed to create fake NDK");
    fs::write(
        path.join("source.properties"),
        format!("Pkg.Revision = {version}\n"),
    )
    .expect("failed to write fake NDK metadata");
    Ndk::from_path(path).expect("fake NDK should load")
}

fn command_env(command: &Command, key: &str) -> Option<OsString> {
    command
        .get_envs()
        .find(|(name, _)| *name == OsStr::new(key))
        .and_then(|(_, value)| value.map(OsString::from))
}

fn assert_tool_env(environment: &BuildEnvironment, base: &str, tool: &Path) {
    let target_prefix = format!("{base}_");
    let target_env = format!("TARGET_{base}");
    assert!(environment.iter().any(|(key, value)| {
        (key == base || key == target_env || key.starts_with(&target_prefix))
            && value == tool.as_os_str()
    }));
}

#[test]
fn library_configures_a_cargo_command_for_an_embedding_executable() {
    let ndk_path = temp_path("command");
    let ndk = fake_ndk(&ndk_path, "28.0.12433566");
    let expected_linker = std::env::current_exe().expect("test executable should resolve");
    let expected_cmake = ndk_path
        .join("build")
        .join("cmake")
        .join("android.toolchain.cmake");
    let expected_bin = ndk_path
        .join("toolchains")
        .join("llvm")
        .join("prebuilt")
        .join(HOST_ARCH)
        .join("bin");
    let config = BuildConfig::new(ndk, AndroidTarget::Arm64V8a, ApiLevel::new(28))
        .with_link_cxx_shared(true);

    let environment = config
        .environment()
        .expect("library environment should be generated");

    assert_eq!(
        environment.get("CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER"),
        Some(expected_linker.as_os_str())
    );
    assert!(
        environment
            .get("CARGO_TARGET_AARCH64_LINUX_ANDROID_RUNNER")
            .is_none()
    );
    assert_eq!(
        environment.get("CARGO_NDK_SYSROOT_TARGET"),
        Some(OsStr::new("aarch64-linux-android"))
    );
    assert_tool_env(&environment, "CC", &expected_bin.join("clang"));
    assert_tool_env(&environment, "CXX", &expected_bin.join("clang++"));
    assert_tool_env(&environment, "AR", &expected_bin.join("llvm-ar"));
    assert_tool_env(&environment, "RANLIB", &expected_bin.join("llvm-ranlib"));
    assert_eq!(
        environment.get("CARGO_TARGET_AARCH64_LINUX_ANDROID_AR"),
        Some(expected_bin.join("llvm-ar").as_os_str())
    );
    assert_eq!(
        environment.get("CARGO_NDK_ANDROID_PLATFORM"),
        Some(OsStr::new("28"))
    );
    assert_eq!(
        environment.get("ANDROID_ABI"),
        Some(OsStr::new("arm64-v8a"))
    );
    assert_eq!(
        environment.get("_CARGO_NDK_LINK_TARGET"),
        Some(OsStr::new("--target=aarch64-linux-android28"))
    );
    assert_eq!(
        environment.get("CARGO_NDK_CMAKE_TOOLCHAIN_PATH"),
        Some(expected_cmake.as_os_str())
    );
    assert!(
        environment
            .get("_CARGO_NDK_LDFLAGS")
            .expect("libc++ flag should be exported")
            .to_string_lossy()
            .contains("-lc++_shared")
    );

    let mut command = Command::new("cargo");
    config
        .configure_command(&mut command)
        .expect("configuration should apply to Cargo");
    assert_eq!(
        command_env(&command, "CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER"),
        Some(expected_linker.into_os_string())
    );

    fs::remove_dir_all(ndk_path).ok();
}

#[test]
fn library_preserves_ndk_builtins_and_page_size_compatibility_flags() {
    let ndk_path = temp_path("flags");
    let ndk = fake_ndk(&ndk_path, "27.3.13750724");
    let builtins_path = ndk_path
        .join("toolchains")
        .join("llvm")
        .join("prebuilt")
        .join(HOST_ARCH)
        .join("lib")
        .join("clang")
        .join("18")
        .join("lib")
        .join("linux");
    fs::create_dir_all(&builtins_path).expect("failed to create fake builtins directory");

    let environment = BuildConfig::new(ndk, AndroidTarget::Arm64V8a, ApiLevel::new(28))
        .with_link_builtins(true)
        .with_linker("xtask")
        .environment()
        .expect("library environment should be generated");
    let flags = environment
        .get("_CARGO_NDK_LDFLAGS")
        .expect("builtins flags should be exported")
        .to_string_lossy();

    assert!(flags.contains(&format!("-L{}", builtins_path.display())));
    assert!(flags.contains("-lclang_rt.builtins-aarch64-android"));
    assert!(flags.contains("-Wl,-z,max-page-size=16384"));
    assert!(!flags.contains("common-page-size"));

    fs::remove_dir_all(ndk_path).ok();
}

#[test]
fn library_rejects_unsupported_ndk_versions_before_generating_environment() {
    let ndk_path = temp_path("unsupported-version");
    let ndk = fake_ndk(&ndk_path, "22.1.7171670");

    let error = BuildConfig::new(ndk, AndroidTarget::Arm64V8a, ApiLevel::new(28))
        .environment()
        .expect_err("NDK versions before r23 should be rejected");

    assert!(error.to_string().contains("less than r23"));
    fs::remove_dir_all(ndk_path).ok();
}
