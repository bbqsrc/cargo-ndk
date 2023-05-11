use std::env;
use std::process::exit;

mod cargo;
mod cli;
mod meta;

#[cfg(windows)]
fn args_hack(cmd: &str) -> anyhow::Result<()> {
    use std::os::windows::process::CommandExt;

    let args = wargs::command_line_to_argv(None)
        .skip(1)
        .collect::<Vec<_>>();

    let mut process = std::process::Command::new(cmd)
        .raw_arg(args.join(" "))
        .spawn()?;

    let status = process.wait()?;

    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Errored with code {}",
            status.code().unwrap()
        ))
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    if env::var("CARGO").is_err() {
        eprintln!("This binary may only be called via `cargo ndk`.");
        exit(1);
    }

    #[cfg(windows)]
    {
        let main_arg = std::env::args().next().unwrap();
        let main_arg = std::path::Path::new(&main_arg)
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap();

        if main_arg != "cargo-ndk" {
            let maybe = main_arg.to_uppercase().replace('-', "_");
            let app = match std::env::var(format!("CARGO_NDK_{maybe}")) {
                Ok(cmd) => cmd,
                Err(err) => {
                    log::error!("{}", err);
                    panic!("{}", err);
                }
            };
            log::debug!("Running command: {app}");
            return args_hack(&app);
        }
    }

    let args = std::env::args().skip(2).collect::<Vec<_>>();

    cli::run(args);
    Ok(())
}
