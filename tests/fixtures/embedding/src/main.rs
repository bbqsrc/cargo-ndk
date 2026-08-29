use std::process::exit;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("_CARGO_NDK_LINK_TARGET").is_some() {
        let status = cargo_ndk::run_linker_wrapper()?;
        exit(status.code().unwrap_or(1));
    }

    Ok(())
}
