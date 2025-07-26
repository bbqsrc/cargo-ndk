use std::fmt::Display;
use std::str::FromStr;

use clap::ValueEnum;
use clap::builder::PossibleValue;
use serde::Deserialize;

pub(crate) fn default_targets() -> &'static [Target] {
    &[Target::ArmeabiV7a, Target::Arm64V8a]
}

#[derive(Debug, Deserialize, Copy, Clone)]
pub enum Target {
    #[serde(rename = "armeabi-v7a")]
    ArmeabiV7a,
    #[serde(rename = "arm64-v8a")]
    Arm64V8a,
    #[serde(rename = "x86")]
    X86,
    #[serde(rename = "x86_64")]
    X86_64,
}

impl ValueEnum for Target {
    fn value_variants<'a>() -> &'a [Self] {
        &[Self::ArmeabiV7a, Self::Arm64V8a, Self::X86, Self::X86_64]
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(match self {
            Self::ArmeabiV7a => PossibleValue::new("armeabi-v7a").alias("armv7-linux-androideabi"),
            Self::Arm64V8a => PossibleValue::new("arm64-v8a").alias("aarch64-linux-android"),
            Self::X86 => PossibleValue::new("x86").alias("i686-linux-android"),
            Self::X86_64 => PossibleValue::new("x86_64").alias("x86_64-linux-android"),
        })
    }
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
