use std::env;
use std::process::exit;

/// We are avoiding using the Clang wrapper scripts in the NDK because they have
/// a quoting bug on Windows (https://github.com/android/ndk/issues/1856) and
/// for consistency on other platforms, considering it's now generally
/// recommended to avoid relying on these wrappers:
/// https://android-review.googlesource.com/c/platform/ndk/+/2134712
///
/// Instead; we set cargo-ndk up as our rustc `_LINKER` as a way to be able to pass
/// --target=<triple><api-level>
///
/// We do it this way because we can't modify rustflags before running `cargo
/// build` without potentially trampling over flags that are configured via
/// Cargo.
fn clang_linker_wrapper() -> ! {
    let args = std::env::args_os().skip(1);
    let clang = std::env::var("_CARGO_NDK_LINK_CLANG")
        .expect("cargo-ndk rustc linker: didn't find _CARGO_NDK_LINK_CLANG env var");
    let target = std::env::var("_CARGO_NDK_LINK_TARGET")
        .expect("cargo-ndk rustc linker: didn't find _CARGO_NDK_LINK_TARGET env var");

    let mut child = std::process::Command::new(&clang)
        .arg(target)
        .args(args)
        .spawn()
        .unwrap_or_else(|err| {
            eprintln!("cargo-ndk: Failed to spawn {clang:?} as linker: {err}");
            std::process::exit(1)
        });
    let status = child.wait().unwrap_or_else(|err| {
        eprintln!("cargo-ndk (as linker): Failed to wait for {clang:?} to complete: {err}");
        std::process::exit(1);
    });

    std::process::exit(status.code().unwrap_or(1))
}

fn main() -> anyhow::Result<()> {
    if env::var("CARGO").is_err() {
        eprintln!("This binary may only be called via `cargo ndk`.");
        exit(1);
    }

    if std::env::var("_CARGO_NDK_LINK_TARGET").is_ok() {
        clang_linker_wrapper();
    }

    let args = std::env::args().skip(1).collect::<Vec<_>>();

    cargo_ndk::cli::run(args)
}
