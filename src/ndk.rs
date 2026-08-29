use std::{
    env,
    ffi::OsString,
    fmt, fs, io,
    path::{Path, PathBuf},
};

use anyhow::Result;
use cargo_metadata::semver::Version;

use crate::ARCH;

const MIN_SUPPORTED_NDK_MAJOR: u64 = 23;

/// A parsed Android NDK revision.
///
/// The representation is intentionally independent of the `cargo_metadata`
/// crate so callers do not need to depend on cargo metadata types just to
/// inspect the NDK version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NdkVersion(Version);

impl NdkVersion {
    /// Returns the major component of the NDK revision.
    pub fn major(&self) -> u64 {
        self.0.major
    }

    /// Returns the minor component of the NDK revision.
    pub fn minor(&self) -> u64 {
        self.0.minor
    }

    /// Returns the patch component of the NDK revision.
    pub fn patch(&self) -> u64 {
        self.0.patch
    }

    fn from_version(version: Version) -> Self {
        Self(version)
    }
}

impl fmt::Display for NdkVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl PartialOrd for NdkVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for NdkVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

/// Describes how an [`Ndk`] installation was discovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NdkSource {
    /// An NDK-specific environment variable, such as `ANDROID_NDK_HOME`.
    Environment(&'static str),
    /// An Android SDK environment variable, such as `ANDROID_HOME`.
    AndroidSdk(&'static str),
    /// The default Android SDK location for the host operating system.
    StandardLocation,
    /// A path supplied directly to [`Ndk::from_path`].
    Explicit,
}

impl fmt::Display for NdkSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Environment(variable) | Self::AndroidSdk(variable) => f.write_str(variable),
            Self::StandardLocation => f.write_str("standard location"),
            Self::Explicit => f.write_str("explicit path"),
        }
    }
}

/// An Android NDK installation and its parsed revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ndk {
    path: PathBuf,
    version: NdkVersion,
    source: NdkSource,
}

impl Ndk {
    /// Loads an NDK from a known installation directory.
    pub fn from_path(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let version = read_version(&path)?;

        Ok(Self {
            path,
            version,
            source: NdkSource::Explicit,
        })
    }

    /// Discovers an NDK using the same environment variables and standard
    /// locations as the `cargo ndk` command.
    ///
    /// `Ok(None)` means that no candidate installation could be found. A
    /// candidate with an unreadable or invalid `source.properties` file is
    /// returned as an error instead of silently trying a different one.
    pub fn discover() -> Result<Option<Self>> {
        Self::discover_with_warning(|_| {})
    }

    /// Discovers an NDK and reports conflicts between equivalent environment
    /// variables to the supplied callback. This is useful for CLI frontends
    /// that want to preserve their own warning/output policy.
    pub fn discover_with_warning<F>(mut warning: F) -> Result<Option<Self>>
    where
        F: FnMut(String),
    {
        let Some((path, source)) = discover_path(&mut warning) else {
            return Ok(None);
        };

        let mut ndk = Self::from_path(path)?;
        ndk.source = source;
        Ok(Some(ndk))
    }

    /// Returns the root directory of the NDK installation.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the parsed NDK revision from `source.properties`.
    pub fn version(&self) -> &NdkVersion {
        &self.version
    }

    /// Checks whether this NDK can be used for an Android build.
    ///
    /// NDK releases before r23 are not supported by cargo-ndk's toolchain
    /// configuration.
    pub fn ensure_supported(&self) -> Result<()> {
        if self.version.major() < MIN_SUPPORTED_NDK_MAJOR {
            anyhow::bail!(
                "NDK versions less than r23 are not supported. Install an up-to-date version of the NDK."
            );
        }

        Ok(())
    }

    /// Returns how this installation was obtained.
    pub fn source(&self) -> NdkSource {
        self.source
    }

    pub(crate) fn tool(&self, name: &str) -> PathBuf {
        self.path
            .join("toolchains")
            .join("llvm")
            .join("prebuilt")
            .join(ARCH)
            .join("bin")
            .join(name)
    }

    pub(crate) fn sysroot(&self) -> PathBuf {
        self.path
            .join("toolchains")
            .join("llvm")
            .join("prebuilt")
            .join(ARCH)
            .join("sysroot")
    }

    pub(crate) fn cmake_toolchain_path(&self) -> PathBuf {
        self.path
            .join("build")
            .join("cmake")
            .join("android.toolchain.cmake")
    }
}

fn discover_path<F>(warning: &mut F) -> Option<(PathBuf, NdkSource)>
where
    F: FnMut(String),
{
    let ndk_vars = [
        "ANDROID_NDK_HOME",
        "ANDROID_NDK_ROOT",
        "ANDROID_NDK_PATH",
        "NDK_HOME",
    ];
    if let Some((variable, path)) = find_first_consistent_var_set(&ndk_vars, warning) {
        let path = PathBuf::from(path);
        return highest_version_ndk_in_path(&path)
            .or(Some(path))
            .map(|path| (path, NdkSource::Environment(variable)));
    }

    let sdk_vars = ["ANDROID_HOME", "ANDROID_SDK_ROOT", "ANDROID_SDK_HOME"];
    if let Some((variable, sdk_path)) = find_first_consistent_var_set(&sdk_vars, warning) {
        let ndk_path = PathBuf::from(&sdk_path).join("ndk");
        if let Some(path) = highest_version_ndk_in_path(&ndk_path) {
            return Some((path, NdkSource::AndroidSdk(variable)));
        }
    }

    default_ndk_dir()
        .and_then(|path| highest_version_ndk_in_path(&path))
        .map(|path| (path, NdkSource::StandardLocation))
}

fn find_first_consistent_var_set<F>(
    vars: &[&'static str],
    warning: &mut F,
) -> Option<(&'static str, OsString)>
where
    F: FnMut(String),
{
    let mut first_var_set = None;
    for variable in vars {
        if let Some(path) = env::var_os(variable) {
            if let Some((first_variable, first_path)) = first_var_set.as_ref() {
                if *first_path != path {
                    warning(format!(
                        "Environment variable `{first_variable} = {first_path:#?}` doesn't match `{variable} = {path:#?}`"
                    ));
                }
                continue;
            }
            first_var_set = Some((*variable, path));
        }
    }

    first_var_set
}

fn highest_version_ndk_in_path(ndk_dir: &Path) -> Option<PathBuf> {
    if ndk_dir.exists() {
        fs::read_dir(ndk_dir)
            .ok()?
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let path = entry.path();
                path.components()
                    .next_back()
                    .and_then(|component| component.as_os_str().to_str())
                    .and_then(|name| Version::parse(name).ok())
                    .map(|version| (version, path))
            })
            .max_by(|(a, _), (b, _)| a.cmp(b))
            .map(|(_, path)| path)
    } else {
        None
    }
}

fn default_ndk_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    let dir = pathos::user::local_dir()
        .ok()?
        .to_path_buf()
        .join("Android")
        .join("sdk")
        .join("ndk");

    #[cfg(target_os = "linux")]
    let dir = pathos::xdg::home_dir()
        .ok()?
        .join("Android")
        .join("Sdk")
        .join("ndk");

    #[cfg(target_os = "macos")]
    let dir = pathos::user::home_dir()
        .ok()?
        .join("Library")
        .join("Android")
        .join("sdk")
        .join("ndk");

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    let dir = PathBuf::new();

    Some(dir)
}

fn read_version(path: &Path) -> Result<NdkVersion> {
    let data = fs::read_to_string(path.join("source.properties"))?;
    for line in data.split('\n') {
        if line.starts_with("Pkg.Revision") {
            let mut chunks = line.split(" = ");
            let _ = chunks.next().ok_or_else(|| io::Error::other("No chunk"))?;
            let version = chunks.next().ok_or_else(|| io::Error::other("No chunk"))?;
            let version = Version::parse(version).map_err(|_| {
                anyhow::anyhow!(format!("Could not parse NDK version. Got: '{version}'"))
            })?;
            return Ok(NdkVersion::from_version(version));
        }
    }

    Err(anyhow::anyhow!("Could not find Pkg.Revision in given path"))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{Ndk, NdkSource, highest_version_ndk_in_path};

    fn temp_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("cargo-ndk-{name}-{nanos}"))
    }

    fn write_ndk(path: &PathBuf, version: &str) {
        fs::create_dir_all(path).expect("failed to create fake NDK");
        fs::write(
            path.join("source.properties"),
            format!("Pkg.Desc = Android NDK\nPkg.Revision = {version}\n"),
        )
        .expect("failed to write fake NDK metadata");
    }

    #[test]
    fn loads_ndk_version_from_explicit_path() {
        let path = temp_path("explicit-ndk");
        write_ndk(&path, "28.0.12433566");

        let ndk = Ndk::from_path(&path).expect("fake NDK should load");

        assert_eq!(ndk.path(), path);
        assert_eq!(ndk.version().to_string(), "28.0.12433566");
        assert_eq!(ndk.version().major(), 28);
        assert_eq!(ndk.version().minor(), 0);
        assert_eq!(ndk.version().patch(), 12433566);
        assert_eq!(ndk.source(), NdkSource::Explicit);

        fs::remove_dir_all(path).ok();
    }

    #[test]
    fn discovery_selects_the_highest_semver_named_ndk() {
        let root = temp_path("versioned-ndk");
        write_ndk(&root.join("25.2.9519653"), "25.2.9519653");
        write_ndk(&root.join("28.0.12433566"), "28.0.12433566");
        write_ndk(&root.join("not-an-ndk"), "1.0.0");

        assert_eq!(
            highest_version_ndk_in_path(&root),
            Some(root.join("28.0.12433566"))
        );

        fs::remove_dir_all(root).ok();
    }
}
