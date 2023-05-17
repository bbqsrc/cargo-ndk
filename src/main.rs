use std::env;
use std::process::exit;

mod cargo;
mod cli;
mod meta;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    if env::var("CARGO").is_err() {
        eprintln!("This binary may only be called via `cargo ndk`.");
        exit(1);
    }

    let args = std::env::args().skip(2).collect::<Vec<_>>();

    cli::run(args);
    Ok(())
}
