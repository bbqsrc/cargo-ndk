use std::env;
use std::process::exit;

fn main() -> anyhow::Result<()> {
    if env::var("CARGO").is_err() {
        eprintln!("This binary may only be called via `cargo ndk-runner`.");
        exit(1);
    }

    let args = std::env::args().collect::<Vec<_>>();

    cargo_ndk::cli::runner::run(args)
}
