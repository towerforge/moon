//! Config, data and state paths. XDG on macOS and Linux alike: a terminal
//! tool has no business in `~/Library/Application Support`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub state_dir: PathBuf,
}

const APP: &str = "moon";

/// The home directory: `HOME`, or `USERPROFILE` on Windows, where `HOME` is
/// usually unset. An empty variable counts as unset.
pub fn home_dir() -> Option<PathBuf> {
    home_from(std::env::var_os("HOME"), std::env::var_os("USERPROFILE"))
}

fn home_from(home: Option<OsString>, userprofile: Option<OsString>) -> Option<PathBuf> {
    [home, userprofile]
        .into_iter()
        .flatten()
        .find(|v| !v.is_empty())
        .map(PathBuf::from)
}

impl Paths {
    /// Reads the home directory and the `XDG_*` variables from the environment.
    pub fn from_env() -> Self {
        let home = home_dir().unwrap_or_else(|| PathBuf::from("."));
        Self::resolve(
            &home,
            std::env::var_os("XDG_CONFIG_HOME"),
            std::env::var_os("XDG_DATA_HOME"),
            std::env::var_os("XDG_STATE_HOME"),
        )
    }

    /// Resolves the paths from explicit values. An empty variable counts as
    /// unset.
    pub fn resolve(
        home: &Path,
        xdg_config: Option<OsString>,
        xdg_data: Option<OsString>,
        xdg_state: Option<OsString>,
    ) -> Self {
        fn pick(var: Option<OsString>, home: &Path, fallback: &str) -> PathBuf {
            match var {
                Some(v) if !v.is_empty() => PathBuf::from(v),
                _ => home.join(fallback),
            }
        }
        Self {
            config_dir: pick(xdg_config, home, ".config").join(APP),
            data_dir: pick(xdg_data, home, ".local/share").join(APP),
            state_dir: pick(xdg_state, home, ".local/state").join(APP),
        }
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn sessions_dir(&self) -> PathBuf {
        self.data_dir.join("sessions")
    }

    pub fn log_file(&self) -> PathBuf {
        self.state_dir.join("moon.log")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_falls_back_to_userprofile() {
        let os = |s: &str| Some(OsString::from(s));
        assert_eq!(
            home_from(os("/home/a"), os("C:\\Users\\a")),
            Some(PathBuf::from("/home/a"))
        );
        assert_eq!(
            home_from(None, os("C:\\Users\\a")),
            Some(PathBuf::from("C:\\Users\\a"))
        );
        assert_eq!(
            home_from(os(""), os("C:\\Users\\a")),
            Some(PathBuf::from("C:\\Users\\a"))
        );
        assert_eq!(home_from(None, None), None);
    }

    #[test]
    fn by_default_under_home() {
        let p = Paths::resolve(Path::new("/home/j"), None, None, None);
        assert_eq!(
            p.config_file(),
            PathBuf::from("/home/j/.config/moon/config.toml")
        );
        assert_eq!(
            p.sessions_dir(),
            PathBuf::from("/home/j/.local/share/moon/sessions")
        );
        assert_eq!(
            p.log_file(),
            PathBuf::from("/home/j/.local/state/moon/moon.log")
        );
    }

    #[test]
    fn honors_xdg_and_ignores_empty_ones() {
        let p = Paths::resolve(
            Path::new("/home/j"),
            Some("/etc/x".into()),
            Some("".into()),
            None,
        );
        assert_eq!(p.config_dir, PathBuf::from("/etc/x/moon"));
        assert_eq!(p.data_dir, PathBuf::from("/home/j/.local/share/moon"));
    }
}
