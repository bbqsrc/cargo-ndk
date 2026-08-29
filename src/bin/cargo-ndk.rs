use std::env;
use std::process::exit;

fn main() -> anyhow::Result<()> {
    if env::var_os("_CARGO_NDK_LINK_TARGET").is_some() {
        let status = match cargo_ndk::run_linker_wrapper() {
            Ok(status) => status,
            Err(error) => {
                eprintln!("{error:#}");
                exit(1);
            }
        };
        exit(status.code().unwrap_or(1));
    }

    if env::var("CARGO").is_err() {
        eprintln!("This binary may only be called via `cargo ndk`.");
        exit(1);
    }

    let args = env::args().skip(1).collect::<Vec<_>>();

    cargo_ndk::cli::run(args)
}
