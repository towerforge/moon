//! Putting the new binary in the place of the one that is running. The write
//! goes to a temporary file next to the target and a rename swaps it in, so a
//! download that dies half way leaves the old binary untouched.

use std::io;
use std::path::{Path, PathBuf};

use crate::UpdateError;

/// Where the running binary came from. Only a release binary is ours to
/// replace: the other two belong to cargo or to the build directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallKind {
    /// Installed by `install.sh`, by the PowerShell installer or by hand.
    Release,
    /// `cargo install`: cargo owns it and `cargo install` updates it.
    Cargo,
    /// `target/debug` or `target/release`: a build, not an installation.
    Dev,
}

impl InstallKind {
    /// Why the update stops because of where the binary lives, and what
    /// updates it instead.
    pub fn refusal(self) -> Option<Refusal> {
        match self {
            InstallKind::Release => None,
            InstallKind::Cargo => Some(Refusal {
                why: "this moon came from cargo",
                fix: "cargo install --git https://github.com/towerforge/moon moon-cli",
            }),
            InstallKind::Dev => Some(Refusal {
                why: "this moon is a local build under target/",
                fix: "make build",
            }),
        }
    }
}

/// An update that stops before downloading anything: the reason, and the
/// command that updates this moon instead. Two pieces so the panel can give
/// the command a line of its own instead of cutting it short; as text, one
/// sentence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refusal {
    pub why: &'static str,
    pub fix: &'static str,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: update it with `{}` (or pass --force to overwrite it anyway)",
            self.why, self.fix
        )
    }
}

/// The binary that is running, with symlinks resolved so the update lands on
/// the real file.
pub fn current_exe() -> Result<PathBuf, UpdateError> {
    let exe = std::env::current_exe().map_err(|source| UpdateError::Io {
        path: PathBuf::from("<current exe>"),
        source,
    })?;
    Ok(std::fs::canonicalize(&exe).unwrap_or(exe))
}

pub fn classify(exe: &Path) -> InstallKind {
    let parts: Vec<_> = exe.iter().map(|p| p.to_string_lossy()).collect();
    let after_target = parts
        .windows(2)
        .any(|w| w[0] == "target" && (w[1] == "debug" || w[1] == "release"));
    if after_target {
        return InstallKind::Dev;
    }
    let cargo_bin = parts
        .windows(2)
        .any(|w| (w[0] == ".cargo" || w[0] == "cargo") && w[1] == "bin");
    if cargo_bin {
        return InstallKind::Cargo;
    }
    InstallKind::Release
}

/// Whether the target can be replaced, asked before anything is downloaded:
/// the swap needs to write in the directory, not into the file.
pub fn check_writable(dest: &Path) -> Result<(), UpdateError> {
    let dir = dest.parent().unwrap_or(Path::new("."));
    let probe = dir.join(format!(".moon-update-probe-{}", std::process::id()));
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            Ok(())
        }
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            Err(UpdateError::Denied(dest.to_path_buf()))
        }
        Err(source) => Err(UpdateError::Io {
            path: dir.to_path_buf(),
            source,
        }),
    }
}

/// Writes `bytes` over `dest`. The old binary is kept until the new one is in
/// place; on Windows, where the running file cannot be overwritten, it is
/// moved aside first and deleted afterwards if the system lets go of it.
pub fn install_binary(bytes: &[u8], dest: &Path) -> Result<(), UpdateError> {
    let dir = dest.parent().unwrap_or(Path::new("."));
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "moon".to_string());
    let tmp = dir.join(format!(".{name}.new-{}", std::process::id()));
    write_executable(&tmp, bytes).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    let swap = swap_in(&tmp, dest, dir, &name);
    if swap.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    swap
}

fn write_executable(path: &Path, bytes: &[u8]) -> Result<(), UpdateError> {
    std::fs::write(path, bytes).map_err(|source| io_err(path, source))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .map_err(|source| io_err(path, source))?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn swap_in(tmp: &Path, dest: &Path, _dir: &Path, _name: &str) -> Result<(), UpdateError> {
    std::fs::rename(tmp, dest).map_err(|source| io_err(dest, source))
}

#[cfg(windows)]
fn swap_in(tmp: &Path, dest: &Path, dir: &Path, name: &str) -> Result<(), UpdateError> {
    let old = dir.join(format!(".{name}.old-{}", std::process::id()));
    let moved = std::fs::rename(dest, &old).is_ok();
    if let Err(source) = std::fs::rename(tmp, dest) {
        if moved {
            let _ = std::fs::rename(&old, dest);
        }
        return Err(io_err(dest, source));
    }
    // the running binary is still locked: it goes on the next run, or by hand
    if moved {
        let _ = std::fs::remove_file(&old);
    }
    Ok(())
}

fn io_err(path: &Path, source: io::Error) -> UpdateError {
    if source.kind() == io::ErrorKind::PermissionDenied {
        UpdateError::Denied(path.to_path_buf())
    } else {
        UpdateError::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_tells_where_the_binary_came_from() {
        assert_eq!(
            classify(Path::new("/usr/local/bin/moon")),
            InstallKind::Release
        );
        assert_eq!(
            classify(Path::new("/home/j/.local/bin/moon")),
            InstallKind::Release
        );
        assert_eq!(
            classify(Path::new("/home/j/.cargo/bin/moon")),
            InstallKind::Cargo
        );
        assert_eq!(
            classify(Path::new("/home/j/moon/target/release/moon")),
            InstallKind::Dev
        );
        assert_eq!(
            classify(Path::new("/home/j/moon/target/debug/moon")),
            InstallKind::Dev
        );
        // a directory called target that is not a build directory
        assert_eq!(
            classify(Path::new("/opt/target/bin/moon")),
            InstallKind::Release
        );
    }

    #[test]
    fn the_refusal_names_the_command_that_updates_it_instead() {
        assert_eq!(InstallKind::Release.refusal(), None);
        let cargo = InstallKind::Cargo.refusal().unwrap();
        assert!(cargo.fix.starts_with("cargo install --git "));
        assert_eq!(
            cargo.to_string(),
            "this moon came from cargo: update it with `cargo install --git \
             https://github.com/towerforge/moon moon-cli` (or pass --force to overwrite it anyway)"
        );
        assert_eq!(InstallKind::Dev.refusal().unwrap().fix, "make build");
    }

    #[test]
    fn the_swap_leaves_the_new_binary_executable() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("moon");
        std::fs::write(&dest, b"old").unwrap();
        check_writable(&dest).unwrap();
        install_binary(b"new", &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"new");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&dest).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "has to stay executable");
        }
        // nothing left behind
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n != "moon")
            .collect();
        assert!(leftovers.is_empty(), "leftovers: {leftovers:?}");
    }

    #[test]
    fn it_installs_where_there_is_nothing_yet() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("moon");
        install_binary(b"new", &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"new");
    }
}
