use std::env;
use std::process::exit;

fn main() -> anyhow::Result<()> {
    if env::var("CARGO").is_err() {
        eprintln!("This binary may only be called via `cargo ndk-env`.");
        exit(1);
    }

    let args = std::env::args().skip(1).collect::<Vec<_>>();

    cargo_ndk::cli::run_env(args)
}
