//! The boundary between the model and the disk. Every tool turns the string
//! the model sent into a path through the sandbox, and nothing else in this
//! crate touches the filesystem. Two checks, in order: a lexical one before
//! the disk is looked at (plain components only, relative to the root) and a
//! physical one after (canonical paths, symlinks), plus the deny rules. A path
//! that passes is under the project root whatever the model wrote.

use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use moon_core::context;

mod glob;
#[cfg(test)]
mod tests;

pub use glob::Glob;

/// Folders `list_dir` leaves out, the same ones the files panel skips.
pub const SKIPPED_DIRS: [&str; 3] = ["target", "node_modules", ".git"];
/// Windows names a device with these, in whatever directory they appear;
/// writing to `NUL` discards and reading `CON` blocks. Denied everywhere.
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];
/// Entries `find` looks at before giving up, so a huge tree costs a bounded
/// walk and not a hang.
const FIND_BUDGET: usize = 20_000;
/// Rename attempts on Windows, where another process holding the file makes
/// the first ones fail.
const RENAME_TRIES: u32 = 5;

/// Why a path was refused. The text is what the model reads back, so it says
/// what to do instead where there is something to do.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Denied {
    #[error("empty path")]
    Empty,
    #[error("`{0}` is outside the project")]
    Outside(String),
    #[error(
        "`{0}` is an absolute path outside the project; paths are relative to the project root"
    )]
    Absolute(String),
    #[error("`{0}` {1}")]
    BadName(String, &'static str),
    #[error("`{0}` is a symlink; moon does not write through symlinks")]
    Symlink(String),
    #[error("`{0}` looks like a secret")]
    Secret(String),
    #[error("`{0}` is on the deny list")]
    DenyList(String),
    #[error("`{0}` not found")]
    NotFound(String),
    #[error("`{0}` is a directory")]
    IsDir(String),
    #[error("`{0}` is not a directory")]
    NotDir(String),
    #[error("`{0}` is not a regular file")]
    NotAFile(String),
    #[error("`{0}` is a binary file")]
    Binary(String),
    #[error("`{0}` is too large ({1} bytes; the limit is {2})")]
    TooLarge(String, u64, usize),
    #[error("`{0}` has changed since it was read; read it again")]
    Stale(String),
    #[error("`{0}` was not read in this conversation; read it before editing")]
    NotRead(String),
    #[error("`{0}`: {1}")]
    Io(String, String),
}

/// Line endings of a file, kept so an edit writes back what it found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eol {
    Lf,
    CrLf,
}

impl Eol {
    /// Whatever the first line break is.
    pub fn detect(text: &str) -> Self {
        match text.find('\n') {
            Some(i) if i > 0 && text.as_bytes()[i - 1] == b'\r' => Eol::CrLf,
            _ => Eol::Lf,
        }
    }

    /// What a new file gets.
    pub fn platform() -> Self {
        if cfg!(windows) {
            Eol::CrLf
        } else {
            Eol::Lf
        }
    }
}

/// A text file as read through the sandbox: the text normalized to `\n` and
/// without its BOM, and what is needed to write it back the way it was.
#[derive(Debug, Clone, PartialEq)]
pub struct FileText {
    /// The path as it is shown: `/`-separated, relative to the root.
    pub rel: String,
    pub text: String,
    /// Hash of the bytes on disk, to know whether it has changed since.
    pub hash: u64,
    pub eol: Eol,
    pub bom: bool,
}

/// One entry of a directory listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
    pub size: u64,
}

/// Where a relative path lands on disk once both checks have passed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    pub rel: String,
    pub full: PathBuf,
    pub exists: bool,
    /// Some component on the way is a symlink (one that stays inside).
    pub via_symlink: bool,
}

#[derive(Debug, Clone)]
pub struct Sandbox {
    /// Canonical, so what `canonicalize` gives for a file can be compared
    /// with it: on macOS `/tmp` is `/private/tmp`, on Windows it starts with
    /// `\\?\`.
    root: PathBuf,
    /// The root as it was given, which is how the user sees it and pastes
    /// it: on macOS `/tmp/x` and not `/private/tmp/x`.
    given: PathBuf,
    max_file_bytes: usize,
    deny: Vec<Glob>,
}

impl Sandbox {
    pub fn new(root: &Path, max_file_bytes: usize, deny: &[String]) -> std::io::Result<Self> {
        Ok(Self {
            root: std::fs::canonicalize(root)?,
            given: std::path::absolute(root)?,
            max_file_bytes,
            deny: deny.iter().map(|p| Glob::new(p)).collect(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn max_file_bytes(&self) -> usize {
        self.max_file_bytes
    }

    /// The lexical check: the path as the model wrote it becomes a list of
    /// plain components, or a refusal, without looking at the disk. `\` is a
    /// separator on every platform, so `..\x` is caught on Linux too.
    pub fn components(raw: &str) -> Result<Vec<String>, Denied> {
        let s = raw.trim();
        if s.is_empty() {
            return Err(Denied::Empty);
        }
        let norm = s.replace('\\', "/");
        let mut chars = norm.chars();
        let first = chars.next().unwrap_or(' ');
        let second = chars.next();
        if first == '/' || (first.is_ascii_alphabetic() && second == Some(':')) {
            return Err(Denied::Absolute(s.to_string()));
        }
        if first == '~' {
            return Err(Denied::BadName(
                s.to_string(),
                "starts with `~`; paths are relative to the project root",
            ));
        }
        let mut out = Vec::new();
        for part in norm.split('/') {
            match part {
                "" | "." => continue,
                ".." => return Err(Denied::Outside(s.to_string())),
                p => {
                    if p.contains(':') {
                        return Err(Denied::BadName(s.to_string(), "contains a colon"));
                    }
                    if p.ends_with('.') || p.ends_with(' ') {
                        return Err(Denied::BadName(s.to_string(), "ends with a dot or a space"));
                    }
                    let stem = p.split('.').next().unwrap_or("").to_ascii_uppercase();
                    if RESERVED.contains(&stem.as_str()) {
                        return Err(Denied::BadName(s.to_string(), "is a reserved device name"));
                    }
                    out.push(p.to_string());
                }
            }
        }
        Ok(out)
    }

    /// An absolute path under the root, the way the user pastes it from the
    /// editor, becomes the relative one; anything else is left as it is for
    /// `components` to judge. Lexical, by whole components, against the root
    /// both as given and canonical; the rest still goes through every check.
    fn within_root<'a>(&self, raw: &'a str) -> Cow<'a, str> {
        let p = Path::new(raw.trim());
        if !p.is_absolute() {
            return Cow::Borrowed(raw);
        }
        match p
            .strip_prefix(&self.given)
            .or_else(|_| p.strip_prefix(&self.root))
        {
            Ok(rest) if rest.as_os_str().is_empty() => Cow::Borrowed("."),
            Ok(rest) => Cow::Owned(rest.to_string_lossy().into_owned()),
            Err(_) => Cow::Borrowed(raw),
        }
    }

    /// The path in the form it is shown and keyed by: `/`-separated, no `./`,
    /// relative to the root even when it was written absolute.
    pub fn relative(&self, raw: &str) -> Result<String, Denied> {
        Ok(Self::components(&self.within_root(raw))?.join("/"))
    }

    /// Files whose path ends in `rel`, for a `not found` that was only
    /// missing the first folders (`src/x.tsx` for `frontend/src/x.tsx`).
    /// A bounded walk that skips what `list` skips and the deny rules; no
    /// symlinks followed.
    pub fn find(&self, rel: &str) -> Vec<String> {
        let tail = format!("/{rel}");
        let mut out = Vec::new();
        let mut budget = FIND_BUDGET;
        let mut stack = vec![(self.root.clone(), String::new())];
        while let Some((dir, prefix)) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in rd.flatten() {
                if budget == 0 {
                    return out;
                }
                budget -= 1;
                let name = e.file_name().to_string_lossy().into_owned();
                if SKIPPED_DIRS.iter().any(|s| s.eq_ignore_ascii_case(&name)) {
                    continue;
                }
                let path = if prefix.is_empty() {
                    name
                } else {
                    format!("{prefix}/{name}")
                };
                let Ok(ft) = e.file_type() else { continue };
                if ft.is_dir() {
                    stack.push((e.path(), path));
                } else if ft.is_file() && path.ends_with(&tail) {
                    let comps: Vec<String> = path.split('/').map(String::from).collect();
                    if self.check_deny(&comps, &path).is_ok() {
                        out.push(path);
                    }
                }
            }
        }
        out.sort();
        out
    }

    /// `.git/` anywhere, the secrets moon never attaches, and the globs from
    /// the configuration. Case-insensitive: NTFS and APFS are.
    fn check_deny(&self, comps: &[String], rel: &str) -> Result<(), Denied> {
        if comps.iter().any(|c| c.eq_ignore_ascii_case(".git")) {
            return Err(Denied::DenyList(rel.to_string()));
        }
        if let Some(last) = comps.last() {
            if context::is_denied(Path::new(last)) {
                return Err(Denied::Secret(rel.to_string()));
            }
        }
        if self.deny.iter().any(|g| g.matches(rel)) {
            return Err(Denied::DenyList(rel.to_string()));
        }
        Ok(())
    }

    /// The physical check: walk the components on disk; a symlink on the way
    /// must resolve under the root, and so must the deepest existing path.
    fn locate(&self, comps: &[String], rel: &str) -> Result<Located, Denied> {
        let full = comps.iter().fold(self.root.clone(), |p, c| p.join(c));
        let mut cur = self.root.clone();
        let mut via_symlink = false;
        let mut exists = true;
        for c in comps {
            cur.push(c);
            match std::fs::symlink_metadata(&cur) {
                Ok(m) => {
                    if m.file_type().is_symlink() {
                        via_symlink = true;
                        let real = std::fs::canonicalize(&cur).map_err(|e| io(rel, e))?;
                        if !real.starts_with(&self.root) {
                            return Err(Denied::Outside(rel.to_string()));
                        }
                    }
                }
                Err(e) if missing(&e) => {
                    exists = false;
                    break;
                }
                Err(e) => return Err(io(rel, e)),
            }
        }
        if exists {
            let real = std::fs::canonicalize(&full).map_err(|e| io(rel, e))?;
            if !real.starts_with(&self.root) {
                return Err(Denied::Outside(rel.to_string()));
            }
        }
        Ok(Located {
            rel: rel.to_string(),
            full,
            exists,
            via_symlink,
        })
    }

    /// Both checks and the deny rules, for a file.
    pub fn resolve(&self, raw: &str) -> Result<Located, Denied> {
        let comps = Self::components(&self.within_root(raw))?;
        let rel = comps.join("/");
        if comps.is_empty() {
            return Err(Denied::IsDir(".".into()));
        }
        self.check_deny(&comps, &rel)?;
        self.locate(&comps, &rel)
    }

    /// A text file, whole. Refuses directories, devices, binaries and
    /// anything over the size limit.
    pub fn read(&self, raw: &str) -> Result<FileText, Denied> {
        let loc = self.resolve(raw)?;
        if !loc.exists {
            return Err(Denied::NotFound(loc.rel));
        }
        let meta = std::fs::metadata(&loc.full).map_err(|e| io(&loc.rel, e))?;
        if meta.is_dir() {
            return Err(Denied::IsDir(loc.rel));
        }
        if !meta.is_file() {
            return Err(Denied::NotAFile(loc.rel));
        }
        if meta.len() > self.max_file_bytes as u64 {
            return Err(Denied::TooLarge(loc.rel, meta.len(), self.max_file_bytes));
        }
        let bytes = std::fs::read(&loc.full).map_err(|e| io(&loc.rel, e))?;
        if context::is_binary(&bytes) {
            return Err(Denied::Binary(loc.rel));
        }
        Ok(decode(loc.rel, &bytes))
    }

    /// The deny rules alone, for a string that may name a path: `.env` is
    /// refused, `HEAD~1` or `--stat` pass, since they name nothing denied.
    /// What the file tools would not read, a command is not pointed at.
    pub fn deny_check(&self, raw: &str) -> Result<(), Denied> {
        let Ok(comps) = Self::components(&self.within_root(raw)) else {
            return Ok(());
        };
        let rel = comps.join("/");
        self.check_deny(&comps, &rel)
    }

    /// Both checks and the deny rules, for a folder that must exist. `"."`
    /// is the root.
    pub fn directory(&self, raw: &str) -> Result<Located, Denied> {
        let comps = Self::components(&self.within_root(raw))?;
        let rel = if comps.is_empty() {
            ".".to_string()
        } else {
            comps.join("/")
        };
        if !comps.is_empty() {
            self.check_deny(&comps, &rel)?;
        }
        let loc = self.locate(&comps, &rel)?;
        if !loc.exists {
            return Err(Denied::NotFound(rel));
        }
        let meta = std::fs::metadata(&loc.full).map_err(|e| io(&rel, e))?;
        if !meta.is_dir() {
            return Err(Denied::NotDir(rel));
        }
        Ok(loc)
    }

    /// The entries of a directory, folders first, without the ones the
    /// files panel skips either. `"."` is the root.
    pub fn list(&self, raw: &str) -> Result<Vec<Entry>, Denied> {
        let loc = self.directory(raw)?;
        let rel = loc.rel;
        let mut out = Vec::new();
        for e in std::fs::read_dir(&loc.full).map_err(|e| io(&rel, e))? {
            let e = e.map_err(|e| io(&rel, e))?;
            let name = e.file_name().to_string_lossy().to_string();
            if SKIPPED_DIRS.iter().any(|s| s.eq_ignore_ascii_case(&name)) {
                continue;
            }
            let m = std::fs::metadata(e.path()).map_err(|e| io(&rel, e))?;
            out.push(Entry {
                name,
                dir: m.is_dir(),
                size: if m.is_dir() { 0 } else { m.len() },
            });
        }
        out.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.cmp(&b.name)));
        Ok(out)
    }

    /// Writes `text` (with `\n`) back in the file's own convention. An
    /// existing file must still carry the hash it was read with; a new one
    /// needs `expect == None`. Atomic: the bytes go to a temporary file next
    /// to the target and take its place with a rename. Returns the new hash.
    pub fn write(
        &self,
        raw: &str,
        text: &str,
        eol: Eol,
        bom: bool,
        expect: Option<u64>,
    ) -> Result<u64, Denied> {
        let loc = self.resolve(raw)?;
        if loc.via_symlink {
            return Err(Denied::Symlink(loc.rel));
        }
        if loc.exists {
            let meta = std::fs::metadata(&loc.full).map_err(|e| io(&loc.rel, e))?;
            if meta.is_dir() {
                return Err(Denied::IsDir(loc.rel));
            }
            if !meta.is_file() {
                return Err(Denied::NotAFile(loc.rel));
            }
            let Some(h) = expect else {
                return Err(Denied::NotRead(loc.rel));
            };
            let current = std::fs::read(&loc.full).map_err(|e| io(&loc.rel, e))?;
            if hash(&current) != h {
                return Err(Denied::Stale(loc.rel));
            }
        } else if expect.is_some() {
            return Err(Denied::NotFound(loc.rel));
        }
        let bytes = encode(text, eol, bom);
        if bytes.len() > self.max_file_bytes {
            return Err(Denied::TooLarge(
                loc.rel,
                bytes.len() as u64,
                self.max_file_bytes,
            ));
        }
        let parent = loc
            .full
            .parent()
            .ok_or_else(|| Denied::Io(loc.rel.clone(), "no parent directory".into()))?;
        std::fs::create_dir_all(parent).map_err(|e| io(&loc.rel, e))?;
        let name = loc
            .full
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let tmp = parent.join(format!("{name}.moon-tmp"));
        std::fs::write(&tmp, &bytes).map_err(|e| io(&loc.rel, e))?;
        #[cfg(unix)]
        if loc.exists {
            if let Ok(meta) = std::fs::metadata(&loc.full) {
                let _ = std::fs::set_permissions(&tmp, meta.permissions());
            }
        }
        if let Err(e) = rename(&tmp, &loc.full) {
            let _ = std::fs::remove_file(&tmp);
            return Err(io(&loc.rel, e));
        }
        Ok(hash(&bytes))
    }
}

/// `NotFound`, and what Windows and a file in the middle of the path give
/// instead: from here on nothing exists.
fn missing(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
    )
}

fn io(rel: &str, e: std::io::Error) -> Denied {
    Denied::Io(rel.to_string(), e.to_string())
}

/// Same hash `context::read_attachment` uses, so both agree on a file.
pub fn hash(bytes: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

/// Bytes on disk → text with `\n`, remembering the BOM and the line endings.
pub fn decode(rel: String, bytes: &[u8]) -> FileText {
    let hash = hash(bytes);
    let (bom, body) = match bytes.strip_prefix(BOM) {
        Some(rest) => (true, rest),
        None => (false, bytes),
    };
    let raw = String::from_utf8_lossy(body);
    let eol = Eol::detect(&raw);
    let text = match eol {
        Eol::CrLf => raw.replace("\r\n", "\n"),
        Eol::Lf => raw.into_owned(),
    };
    FileText {
        rel,
        text,
        hash,
        eol,
        bom,
    }
}

/// Text with `\n` → bytes in the file's convention.
pub fn encode(text: &str, eol: Eol, bom: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + 3);
    if bom {
        out.extend_from_slice(BOM);
    }
    match eol {
        Eol::Lf => out.extend_from_slice(text.as_bytes()),
        Eol::CrLf => out.extend_from_slice(text.replace('\n', "\r\n").as_bytes()),
    }
    out
}

/// `fs::rename` replaces the target on every platform, but on Windows it
/// fails while another process holds the file: an antivirus scan, some
/// editors. A few tries with a short pause before giving up.
fn rename(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut last = None;
    for i in 0..RENAME_TRIES {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) => {
                if !cfg!(windows) {
                    return Err(e);
                }
                last = Some(e);
                std::thread::sleep(std::time::Duration::from_millis(20 * (i as u64 + 1)));
            }
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("rename failed")))
}
