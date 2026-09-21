//! The platform the running binary was built for, and the release asset that
//! matches it. The names are the ones `make package` writes and `install.sh`
//! downloads: `moon-<os>-<arch>[-musl].tar.gz`, `.zip` on Windows.

use std::fmt;

use crate::UpdateError;

pub const APP: &str = "moon";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub os: &'static str,
    pub arch: &'static str,
    /// Statically linked against musl. Only ever true on Linux.
    pub musl: bool,
}

impl Target {
    pub const fn new(os: &'static str, arch: &'static str, musl: bool) -> Self {
        Self { os, arch, musl }
    }

    /// What this binary was compiled for. The libc comes from the build, not
    /// from the machine: a musl binary stays on musl.
    pub fn current() -> Result<Self, UpdateError> {
        let os = match std::env::consts::OS {
            "macos" => "macos",
            "linux" => "linux",
            "windows" => "windows",
            other => return Err(UpdateError::Unsupported(other.to_string())),
        };
        let arch = match std::env::consts::ARCH {
            "x86_64" => "x86_64",
            "aarch64" => "aarch64",
            other => return Err(UpdateError::Unsupported(format!("{os} {other}"))),
        };
        Ok(Self::new(os, arch, cfg!(target_env = "musl")))
    }

    pub fn is_windows(&self) -> bool {
        self.os == "windows"
    }

    /// Name of the release asset for this platform.
    pub fn asset_name(&self) -> String {
        let musl = if self.musl && self.os == "linux" {
            "-musl"
        } else {
            ""
        };
        let ext = if self.is_windows() { "zip" } else { "tar.gz" };
        format!("{APP}-{}-{}{musl}.{ext}", self.os, self.arch)
    }

    /// Name of the binary inside that asset.
    pub fn binary_name(&self) -> String {
        if self.is_windows() {
            format!("{APP}.exe")
        } else {
            APP.to_string()
        }
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} · {}", self.os, self.arch)?;
        if self.musl && self.os == "linux" {
            write!(f, " · musl")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_names_are_the_ones_the_releases_use() {
        assert_eq!(
            Target::new("macos", "aarch64", false).asset_name(),
            "moon-macos-aarch64.tar.gz"
        );
        assert_eq!(
            Target::new("linux", "x86_64", false).asset_name(),
            "moon-linux-x86_64.tar.gz"
        );
        assert_eq!(
            Target::new("linux", "aarch64", true).asset_name(),
            "moon-linux-aarch64-musl.tar.gz"
        );
        assert_eq!(
            Target::new("windows", "x86_64", false).asset_name(),
            "moon-windows-x86_64.zip"
        );
        // musl is a Linux matter: it does not touch the other names
        assert_eq!(
            Target::new("macos", "aarch64", true).asset_name(),
            "moon-macos-aarch64.tar.gz"
        );
    }

    #[test]
    fn the_binary_carries_exe_on_windows() {
        assert_eq!(Target::new("linux", "x86_64", false).binary_name(), "moon");
        assert_eq!(
            Target::new("windows", "x86_64", false).binary_name(),
            "moon.exe"
        );
    }

    #[test]
    fn this_platform_is_supported() {
        let t = Target::current().expect("moon builds for this platform");
        assert!(t.asset_name().starts_with("moon-"));
    }
}
