use std::{collections::BTreeMap, env};

use clap::{CommandFactory, Parser};

use crate::{
    cargo::{build_env, clang_target},
    cli::derive_ndk_path,
    meta::Target,
    shell::{Shell, Verbosity},
};

#[derive(Debug, Parser)]
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
    // Check for help/version before parsing to avoid required arg errors
    if args.contains(&"--help".to_string()) {
        EnvArgs::command().print_long_help().unwrap();
        std::process::exit(0);
    }

    if args.contains(&"-h".to_string()) {
        EnvArgs::command().print_help().unwrap();
        std::process::exit(0);
    }

    if args.contains(&"--version".to_string()) || args.contains(&"-V".to_string()) {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }

    let color = args
        .iter()
        .position(|x| x == "--color")
        .and_then(|p| args.get(p + 1))
        .map(|x| &**x);

    let verbosity = if args.contains(&"-q".into()) {
        Verbosity::Quiet
    } else if args.contains(&"-vv".into()) {
        Verbosity::VeryVerbose
    } else if args.contains(&"-v".into()) || args.contains(&"--verbose".into()) {
        Verbosity::Verbose
    } else {
        Verbosity::Normal
    };

    let mut shell = Shell::new();
    shell.set_verbosity(verbosity);
    shell.set_color_choice(color)?;

    let args = match EnvArgs::try_parse_from(&args) {
        Ok(args) => args,
        Err(e) => {
            shell.error(e)?;
            std::process::exit(2);
        }
    };

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
        println!("#     cargo ndk-env --powershell | Out-String | Invoke-Expression");
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
