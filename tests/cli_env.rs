use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn temp_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("cargo-ndk-cli-env-{name}-{nanos}"))
}

#[test]
fn ndk_env_includes_the_cmake_toolchain_path() {
    let ndk_path = temp_path("cmake");
    fs::create_dir_all(&ndk_path).expect("failed to create fake NDK");
    fs::write(
        ndk_path.join("source.properties"),
        "Pkg.Revision = 28.0.12433566\n",
    )
    .expect("failed to write fake NDK metadata");

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-ndk-env"))
        .args(["ndk-env", "--target", "arm64-v8a", "--json"])
        .env("CARGO", "cargo")
        .env("ANDROID_NDK_HOME", &ndk_path)
        .output()
        .expect("cargo-ndk-env should run");

    assert!(
        output.status.success(),
        "cargo-ndk-env exited with {:?}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("JSON output should be UTF-8");
    assert!(stdout.contains("CARGO_NDK_CMAKE_TOOLCHAIN_PATH"));
    assert!(stdout.contains("android.toolchain.cmake"));

    fs::remove_dir_all(ndk_path).ok();
}
