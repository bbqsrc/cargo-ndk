use std::process::Command;

fn cargo_ndk_env_help(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-ndk-env"))
        .env("CARGO", "cargo")
        .args(args)
        .output()
        .expect("cargo-ndk-env should run");

    assert!(
        output.status.success(),
        "cargo-ndk-env exited with {:?}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout).expect("help output should be valid UTF-8")
}

#[test]
fn cargo_ndk_env_help_shows_env_format_options() {
    for args in [["--help"].as_slice(), ["--", "--help"].as_slice()] {
        let help = cargo_ndk_env_help(args);

        assert!(help.contains("Usage: cargo-ndk-env"));
        assert!(help.contains("--powershell"));
        assert!(help.contains("--json"));
        assert!(!help.contains("--output-dir"));
    }
}
