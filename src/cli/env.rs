use std::collections::BTreeMap;

use clap::Parser;

use crate::{
    cargo::{build_env, clang_target},
    cli::{derive_ndk_path, init},
    meta::Target,
};

#[derive(Debug, Parser, Clone)]
struct EnvArgs {
    /// Triples for the target. Can be Rust or Android target names (i.e. arm64-v8a)
    #[arg(short, long, env = "CARGO_NDK_TARGET")]
    target: Target,

    /// Platform (also known as API level)
    #[arg(long, default_value_t = 21, env = "CARGO_NDK_PLATFORM")]
    platform: u8,

    /// Links Clang builtins library
    #[arg(long, default_value_t = false, env = "CARGO_NDK_LINK_BUILTINS")]
    link_builtins: bool,

    /// Use PowerShell syntax
    #[arg(long)]
    powershell: bool,

    /// Print output in JSON format
    #[arg(long)]
    json: bool,
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

    let clang_target = clang_target(args.target.triple(), args.platform);

    // Try command line, then config. Config falls back to defaults in any case.
    let env = build_env(
        args.target.triple(),
        &ndk_home,
        &clang_target,
        args.link_builtins,
    )
    .into_iter()
    .filter(|(k, _)| !k.starts_with('_'))
    .collect::<BTreeMap<_, _>>();

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(
                &env.into_iter()
                    .map(|(k, v)| (k, v.to_str().unwrap().to_string()))
                    .collect::<BTreeMap<_, _>>()
            )
            .unwrap()
        );
    } else if args.powershell {
        for (k, v) in env {
            println!("${{env:{k}}}={v:?}");
        }
        println!();
        println!("# To import with PowerShell:");
        println!("#     cargo ndk-env --powershell | Invoke-Expression");
    } else {
        for (k, v) in env {
            println!("export {}={:?}", k.to_uppercase().replace('-', "_"), v);
        }
        println!();
        println!("# To import with bash/zsh/etc:");
        println!("#     source <(cargo ndk-env)");
    }

    Ok(())
}
