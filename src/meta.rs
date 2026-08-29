use std::fmt::Display;
use std::str::FromStr;

use clap::ValueEnum;
use clap::builder::PossibleValue;
use serde::Deserialize;

pub(crate) fn default_targets() -> &'static [Target] {
    &[Target::ArmeabiV7a, Target::Arm64V8a]
}

/// An Android API level used when selecting the NDK platform libraries.
#[derive(Debug, Deserialize, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ApiLevel(u8);

impl ApiLevel {
    /// Creates an API level from its numeric representation.
    pub const fn new(value: u8) -> Self {
        Self(value)
    }

    /// Returns the numeric API level.
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl Default for ApiLevel {
    fn default() -> Self {
        Self::new(21)
    }
}

impl From<u8> for ApiLevel {
    fn from(value: u8) -> Self {
        Self::new(value)
    }
}

impl Display for ApiLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for ApiLevel {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(Self::new)
    }
}

/// A Rust-supported Android target and its corresponding Android ABI.
#[derive(Debug, Deserialize, Copy, Clone, PartialEq, Eq, Hash)]
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
        f.write_str(self.abi())
    }
}

impl Target {
    /// Returns the Rust target triple used by Cargo.
    pub fn triple(&self) -> &'static str {
        match self {
            Target::ArmeabiV7a => "armv7-linux-androideabi",
            Target::Arm64V8a => "aarch64-linux-android",
            Target::X86 => "i686-linux-android",
            Target::X86_64 => "x86_64-linux-android",
        }
    }

    /// Returns the Android ABI directory name for this target.
    pub fn abi(&self) -> &'static str {
        match self {
            Target::ArmeabiV7a => "armeabi-v7a",
            Target::Arm64V8a => "arm64-v8a",
            Target::X86 => "x86",
            Target::X86_64 => "x86_64",
        }
    }

    /// Returns the Clang target triple used by the NDK linker wrapper.
    pub fn clang_triple(&self) -> &'static str {
        match self {
            Target::ArmeabiV7a => "armv7a-linux-androideabi",
            Target::Arm64V8a => "aarch64-linux-android",
            Target::X86 => "i686-linux-android",
            Target::X86_64 => "x86_64-linux-android",
        }
    }

    /// Returns the `--target` argument passed to the NDK Clang executable.
    pub fn clang_target(&self, api_level: ApiLevel) -> String {
        format!("--target={}{}", self.clang_triple(), api_level)
    }

    /// Returns the target directory name used inside the NDK sysroot.
    pub fn sysroot_target(&self) -> &'static str {
        match self {
            Target::ArmeabiV7a => "arm-linux-androideabi",
            Target::Arm64V8a => "aarch64-linux-android",
            Target::X86 => "i686-linux-android",
            Target::X86_64 => "x86_64-linux-android",
        }
    }

    /// Returns whether this target uses a 64-bit Android ABI.
    pub fn is_64_bit(&self) -> bool {
        matches!(self, Target::Arm64V8a | Target::X86_64)
    }
}

/// A descriptive alias for [`Target`] when used outside the CLI layer.
pub type AndroidTarget = Target;

#[cfg(test)]
mod tests {
    use super::{ApiLevel, Target};

    #[test]
    fn target_mapping_covers_abi_clang_and_sysroot_names() {
        let cases = [
            (
                Target::ArmeabiV7a,
                "armeabi-v7a",
                "armv7-linux-androideabi",
                "armv7a-linux-androideabi",
                "arm-linux-androideabi",
            ),
            (
                Target::Arm64V8a,
                "arm64-v8a",
                "aarch64-linux-android",
                "aarch64-linux-android",
                "aarch64-linux-android",
            ),
            (
                Target::X86,
                "x86",
                "i686-linux-android",
                "i686-linux-android",
                "i686-linux-android",
            ),
            (
                Target::X86_64,
                "x86_64",
                "x86_64-linux-android",
                "x86_64-linux-android",
                "x86_64-linux-android",
            ),
        ];

        for (target, abi, triple, clang_triple, sysroot_target) in cases {
            assert_eq!(target.abi(), abi);
            assert_eq!(target.triple(), triple);
            assert_eq!(target.clang_triple(), clang_triple);
            assert_eq!(target.sysroot_target(), sysroot_target);
            assert_eq!(
                target.clang_target(ApiLevel::new(28)),
                format!("--target={clang_triple}28")
            );
        }
    }

    #[test]
    fn api_level_round_trips_through_its_numeric_value() {
        let api_level = ApiLevel::new(29);

        assert_eq!(api_level.get(), 29);
        assert_eq!(api_level.to_string(), "29");
        assert_eq!("29".parse::<ApiLevel>().unwrap(), api_level);
    }
}
