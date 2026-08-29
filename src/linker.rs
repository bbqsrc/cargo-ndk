use std::{
    env,
    ffi::{OsStr, OsString},
    process::{Command, ExitStatus},
};

use anyhow::{Context, Result};

const LINK_CLANG: &str = "_CARGO_NDK_LINK_CLANG";
const LINK_TARGET: &str = "_CARGO_NDK_LINK_TARGET";
const LINK_LDFLAGS: &str = "_CARGO_NDK_LDFLAGS";

/// Runs the linker wrapper using the internal environment variables prepared
/// by [`crate::BuildConfig`].
///
/// The function does not terminate the process. An embedding executable can
/// return or translate the resulting status code according to its own CLI
/// conventions.
pub fn run_linker_wrapper() -> Result<ExitStatus> {
    let clang = required_env(LINK_CLANG)?;
    let target = required_env(LINK_TARGET)?;
    let ldflags = env::var_os(LINK_LDFLAGS);

    linker_command(env::args_os().skip(1), &clang, &target, ldflags.as_deref())
        .status()
        .with_context(|| {
            format!(
                "cargo-ndk linker: failed to execute {}",
                PathDisplay(&clang)
            )
        })
}

fn required_env(name: &str) -> Result<OsString> {
    env::var_os(name)
        .ok_or_else(|| anyhow::anyhow!("cargo-ndk rustc linker: didn't find {name} env var"))
}

fn linker_command<I>(args: I, clang: &OsStr, target: &OsStr, ldflags: Option<&OsStr>) -> Command
where
    I: IntoIterator<Item = OsString>,
{
    let mut command = Command::new(clang);
    command.arg(target);

    if let Some(ldflags) = ldflags {
        for flag in ldflags.to_string_lossy().split('\x1f') {
            command.arg(flag);
        }
    }

    command.args(args);
    command
}

struct PathDisplay<'a>(&'a OsStr);

impl std::fmt::Display for PathDisplay<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0.to_string_lossy())
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::{OsStr, OsString};

    use super::linker_command;

    #[test]
    fn linker_command_places_target_and_flags_before_cargo_arguments() {
        let command = linker_command(
            vec![OsString::from("-shared"), OsString::from("output.so")],
            OsStr::new("clang"),
            OsStr::new("--target=aarch64-linux-android28"),
            Some(OsStr::new(
                "-Lbuiltins\x1f-lclang_rt.builtins-aarch64-android",
            )),
        );

        assert_eq!(command.get_program(), OsStr::new("clang"));
        let args = command.get_args().collect::<Vec<_>>();
        assert_eq!(
            args,
            vec![
                OsStr::new("--target=aarch64-linux-android28"),
                OsStr::new("-Lbuiltins"),
                OsStr::new("-lclang_rt.builtins-aarch64-android"),
                OsStr::new("-shared"),
                OsStr::new("output.so"),
            ]
        );
    }
}
