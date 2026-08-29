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
    std::env::temp_dir().join(format!("cargo-ndk-wrapper-{name}-{nanos}"))
}

#[test]
fn embedding_executable_dispatches_to_wrapper_without_cargo_ndk_binary() {
    let directory = temp_path("dispatch");
    fs::create_dir_all(&directory).expect("failed to create wrapper test directory");

    #[cfg(windows)]
    let (clang, linker_args) = {
        let script = directory.join("fake-clang.cmd");
        fs::write(&script, "@echo off\r\nexit /b 0\r\n")
            .expect("failed to write fake clang script");
        ("cmd.exe".to_string(), vec![script.into_os_string()])
    };

    #[cfg(not(windows))]
    let (clang, linker_args) = {
        use std::os::unix::fs::PermissionsExt;

        let script = directory.join("fake-clang");
        fs::write(&script, "#!/bin/sh\nexit 0\n").expect("failed to write fake clang script");
        let mut permissions = fs::metadata(&script)
            .expect("fake clang metadata should be available")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).expect("failed to make fake clang executable");
        (
            script.to_string_lossy().into_owned(),
            vec!["output.o".into()],
        )
    };

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("embedding")
        .join("Cargo.toml");
    let fixture_directory = manifest
        .parent()
        .expect("embedding fixture should have a parent directory");
    let fixture_lockfile = fixture_directory.join("Cargo.lock");
    let fixture_lockfile_existed = fixture_lockfile.exists();
    let target_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("embedding-fixture");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args(["--offline", "run", "--quiet", "--manifest-path"])
        .arg(&manifest)
        .args(["--target-dir"])
        .arg(&target_dir)
        .arg("--")
        .args(linker_args)
        .env_remove("CARGO")
        .env(
            "_CARGO_NDK_LINK_TARGET",
            if cfg!(windows) { "/C" } else { "--target=test" },
        )
        .env("_CARGO_NDK_LINK_CLANG", clang)
        .output()
        .expect("cargo-ndk wrapper process should start");

    if !fixture_lockfile_existed {
        fs::remove_file(&fixture_lockfile).ok();
    }
    fs::remove_dir_all(&target_dir).ok();

    assert!(
        output.status.success(),
        "embedded linker wrapper failed with {:?}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    fs::remove_dir_all(directory).ok();
}
