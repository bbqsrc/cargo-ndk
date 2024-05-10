use std::fmt::Display;
use std::path::Path;
use std::str::FromStr;

use serde::Deserialize;

use crate::cli::BuildMode;

const fn default_platform() -> u8 {
    21
}

fn default_targets() -> Vec<Target> {
    vec![Target::ArmeabiV7a, Target::Arm64V8a]
}

#[derive(Debug, Deserialize)]
struct CargoToml {
    package: Package,
    lib: Option<Lib>,
}

#[derive(Debug, Deserialize)]
struct Package {
    name: String,
    metadata: Option<Metadata>,
}

#[derive(Debug, Deserialize)]
struct Metadata {
    ndk: Option<Ndk>,
}

#[derive(Debug, Deserialize)]
struct Lib {
    name: Option<String>
}

#[derive(Debug, Deserialize)]
pub(crate) struct Ndk {
    #[serde(default = "default_platform")]
    pub platform: u8,

    #[serde(default = "default_targets")]
    targets: Vec<Target>,

    release: Option<NdkTarget>,
    debug: Option<NdkTarget>,
}

impl Default for Ndk {
    fn default() -> Self {
        Self {
            platform: default_platform(),
            targets: default_targets(),
            release: None,
            debug: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct NdkTarget {
    targets: Vec<Target>,
}

#[derive(Debug)]
pub struct Config {
    pub lib_name: String,
    pub platform: u8,
    pub targets: Vec<Target>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            platform: Ndk::default().platform,
            targets: default_targets(),
            lib_name: String::new(),
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub enum Target {
    #[serde(rename = "armeabi-v7a")]
    ArmeabiV7a,
    #[default]
    #[serde(rename = "arm64-v8a")]
    Arm64V8a,
    #[serde(rename = "x86")]
    X86,
    #[serde(rename = "x86_64")]
    X86_64,
}

impl FromStr for Target {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            // match android style architectures
            "armeabi-v7a" => Target::ArmeabiV7a,
            "arm64-v8a" => Target::Arm64V8a,
            "x86" => Target::X86,
            "x86_64" => Target::X86_64,
            // match rust triple architectures
            "armv7-linux-androideabi" => Target::ArmeabiV7a,
            "aarch64-linux-android" => Target::Arm64V8a,
            "i686-linux-android" => Target::X86,
            "x86_64-linux-android" => Target::X86_64,
            _ => return Err(format!("Unsupported target: '{s}'")),
        })
    }
}

impl Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Target::ArmeabiV7a => "armeabi-v7a",
            Target::Arm64V8a => "arm64-v8a",
            Target::X86 => "x86",
            Target::X86_64 => "x86_64",
        })
    }
}

impl Target {
    pub fn triple(&self) -> &'static str {
        match self {
            Target::ArmeabiV7a => "armv7-linux-androideabi",
            Target::Arm64V8a => "aarch64-linux-android",
            Target::X86 => "i686-linux-android",
            Target::X86_64 => "x86_64-linux-android",
        }
    }
}

pub(crate) fn config(
    cargo_toml_path: &Path,
    build_mode: &BuildMode,
) -> Result<Config, anyhow::Error> {
    let toml_string = std::fs::read_to_string(cargo_toml_path)?;
    let cargo_toml: CargoToml = toml::from_str(&toml_string)?;

    let package = cargo_toml.package;

    let ndk = package.metadata.and_then(|x| x.ndk).unwrap_or_default();
    let base_targets = ndk.targets;

    let targets = if matches!(build_mode, BuildMode::Release) {
        ndk.release.map_or_else(|| base_targets, |x| x.targets)
    } else {
        ndk.debug.map_or_else(|| base_targets, |x| x.targets)
    };

    Ok(Config {
        lib_name: cargo_toml.lib.and_then(|x| x.name).unwrap_or(package.name).replace("-", "_"),
        platform: ndk.platform,
        targets,
    })
}
