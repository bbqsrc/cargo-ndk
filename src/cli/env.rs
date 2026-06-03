use std::{collections::BTreeMap, ffi::OsString};

use clap::Parser;

use crate::{
    cargo::build_env,
    clang_target,
    cli::{derive_ndk_path, derive_ndk_version, init},
    meta::Target,
};

#[derive(Debug, Parser, Clone)]
#[command(name = "cargo-ndk-env")]
struct EnvArgs {
    /// Triples for the target. Can be Rust or Android target names (i.e. arm64-v8a)
    #[arg(short, long, env = "CARGO_NDK_TARGET")]
    target: Target,

    /// Platform (also known as API level)
    #[arg(short = 'P', long, default_value_t = 21, env = "CARGO_NDK_PLATFORM")]
    platform: u8,

    /// Links Clang builtins library
    #[arg(long, default_value_t = false, env = "CARGO_NDK_LINK_BUILTINS")]
    link_builtins: bool,

    /// Links libc++_shared library
    #[arg(long, default_value_t = false, env = "CARGO_NDK_LINK_LIBCXX_SHARED")]
    link_libcxx_shared: bool,

    /// Use PowerShell syntax
    #[arg(long)]
    powershell: bool,

    /// Print output in JSON format
    #[arg(long)]
    json: bool,

    /// Include internal cargo-ndk linker wrapper variables
    #[arg(long)]
    include_internal: bool,
}

fn filter_env(
    env: BTreeMap<String, OsString>,
    include_internal: bool,
) -> BTreeMap<String, OsString> {
    if include_internal {
        env
    } else {
        env.into_iter()
            .filter(|(k, _)| !k.starts_with('_'))
            .collect()
    }
}

fn env_value(value: &OsString) -> &str {
    value
        .to_str()
        .expect("cargo-ndk environment values should be valid UTF-8")
}

fn shell_quote(value: &OsString) -> String {
    format!("'{}'", env_value(value).replace('\'', "'\\''"))
}

fn powershell_quote(value: &OsString) -> String {
    format!("'{}'", env_value(value).replace('\'', "''"))
}

pub fn run(args: Vec<String>) -> anyhow::Result<()> {
    let (mut shell, args) = init::<EnvArgs>(args)?;
    let args = EnvArgs::try_parse_from(args)?;

    let (ndk_home, _ndk_detection_method) = match derive_ndk_path(&mut shell) {
        Some((path, method)) => (path, method),
        None => {
            shell.error("Could not find any NDK.")?;
            shell.note(
                "Set the environment ANDROID_NDK_HOME to your NDK installation's root directory,\nor install the NDK using Android Studio."
            )?;
            std::process::exit(1);
        }
    };

    let ndk_version = derive_ndk_version(&ndk_home)?;
    let clang_target = clang_target(args.target.triple(), args.platform);

    // Try command line, then config. Config falls back to defaults in any case.
    let env = filter_env(
        build_env(
            args.target.triple(),
            &ndk_home,
            &ndk_version,
            &clang_target,
            args.platform,
            &args.target.to_string(),
            args.link_builtins,
            args.link_libcxx_shared,
        ),
        args.include_internal,
    );

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(
                &env.into_iter()
                    .map(|(k, v)| (k, env_value(&v).to_string()))
                    .collect::<BTreeMap<_, _>>()
            )
            .unwrap()
        );
    } else if args.powershell {
        for (k, v) in env {
            println!("${{env:{k}}}={}", powershell_quote(&v));
        }
        println!();
        println!("# To import with PowerShell:");
        println!("#     cargo ndk-env --powershell | Invoke-Expression");
    } else {
        for (k, v) in env {
            println!("export {}={}", k.replace('-', "_"), shell_quote(&v));
        }
        println!();
        println!("# To import with bash/zsh/etc:");
        println!("#     source <(cargo ndk-env)");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, ffi::OsString};

    use super::{filter_env, powershell_quote, shell_quote};

    fn sample_env() -> BTreeMap<String, OsString> {
        BTreeMap::from([
            ("ANDROID_PLATFORM".to_string(), OsString::from("28")),
            (
                "_CARGO_NDK_LINK_TARGET".to_string(),
                OsString::from("--target=aarch64-linux-android28"),
            ),
        ])
    }

    #[test]
    fn filter_env_hides_internal_values_by_default() {
        let env = filter_env(sample_env(), false);

        assert!(env.contains_key("ANDROID_PLATFORM"));
        assert!(!env.contains_key("_CARGO_NDK_LINK_TARGET"));
    }

    #[test]
    fn filter_env_keeps_internal_values_when_requested() {
        let env = filter_env(sample_env(), true);

        assert!(env.contains_key("ANDROID_PLATFORM"));
        assert_eq!(
            env["_CARGO_NDK_LINK_TARGET"],
            OsString::from("--target=aarch64-linux-android28")
        );
    }

    #[test]
    fn shell_quote_preserves_internal_ldflag_separator() {
        assert_eq!(
            shell_quote(&OsString::from("-L/path with spaces\x1f-lbuiltins's")),
            "'-L/path with spaces\x1f-lbuiltins'\\''s'"
        );
    }

    #[test]
    fn powershell_quote_preserves_internal_ldflag_separator() {
        assert_eq!(
            powershell_quote(&OsString::from("-L/path with spaces\x1f-lbuiltins's")),
            "'-L/path with spaces\x1f-lbuiltins''s'"
        );
    }
}
