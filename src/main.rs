use std::env;
use std::process::exit;

mod cargo;
mod cli;
mod meta;

/// We are avoiding using the Clang wrapper scripts in the NDK because they have
/// a quoting bug on Windows (https://github.com/android/ndk/issues/1856) and
/// for consistency on other platforms, considering it's now generally
/// recommended to avoid relying on these wrappers:
/// https://android-review.googlesource.com/c/platform/ndk/+/2134712
///
/// Instead; we set cargo-ndk up as a RUSTC_WRAPPER as a way to be able to pass
/// "-Clink-arg=--target=<triple>"
///
/// We do it this way because we can't modify rustflags before running `cargo
/// build` without potentially trampling over flags that are configured via
/// Cargo.
fn rustc_wrapper() -> ! {
    let is_cross = std::env::args().any(|arg| arg.starts_with("--target"));

    let mut args = std::env::args_os();
    let _first = args
        .next()
        .expect("cargo-ndk rustc wrapper: expected at least two argument"); // ignore arg[0]
    let rustc = args
        .next()
        .expect("cargo-ndk rustc wrapper: expected at least one argument"); // The first argument is the "real" rustc, followed by arguments
    let target_arg = std::env::var("_CARGO_NDK_RUSTC_TARGET")
        .expect("cargo-ndk rustc wrapper didn't find _CARGO_NDK_RUSTC_TARGET env var");

    // If RUSTC_WRAPPER was already set in the environment then we daisy chain to the original
    // wrapper, otherwise we run rustc specified via args[1]
    let mut cmd = if let Ok(wrapper_rustc) = std::env::var("_CARGO_NDK_WRAPPED_RUSTC") {
        let mut cmd = std::process::Command::new(wrapper_rustc);
        cmd.arg(&rustc);
        cmd
    } else {
        std::process::Command::new(&rustc)
    };

    if is_cross {
        cmd.arg(format!("-Clink-arg={target_arg}"));
    }

    let mut child = cmd.args(args).spawn().unwrap_or_else(|err| {
        eprintln!("cargo-ndk: Failed to spawn {rustc:?} as rustc wrapper: {err}");
        std::process::exit(1)
    });
    let status = child.wait().unwrap_or_else(|err| {
        eprintln!("cargo-ndk (as rustc wrapper): Failed to wait for {rustc:?} to complete: {err}");
        std::process::exit(1);
    });

    std::process::exit(status.code().unwrap_or(1))
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    if env::var("CARGO").is_err() {
        eprintln!("This binary may only be called via `cargo ndk`.");
        exit(1);
    }

    if std::env::var("_CARGO_NDK_RUSTC_TARGET").is_ok() {
        rustc_wrapper();
    }

    let args = std::env::args().skip(2).collect::<Vec<_>>();

    cli::run(args);
    Ok(())
}
