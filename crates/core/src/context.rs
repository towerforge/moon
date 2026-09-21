//! Files in the context without tools: the model only sees what moon attaches
//! for it. Three sources: the project context file (`MOON.md`), `@path`
//! mentions (snapshots inside the message) and the live attachments from
//! the files panel (re-read on every send, in the system prompt).

use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const DEFAULT_CONTEXT_FILE: &str = "MOON.md";
pub const DEFAULT_MAX_BYTES: usize = 200_000;
/// Fraction of the context window beyond which moon does not send.
pub const BUDGET_RATIO: f32 = 0.8;

#[derive(Debug, Error)]
pub enum AttachError {
    #[error("not found")]
    NotFound,
    #[error("is a directory")]
    IsDir,
    #[error("binary file")]
    Binary,
    #[error("looks like a secret; use @!{0} to force")]
    Denied(String),
    #[error("invalid range `{0}`")]
    BadRange(String),
    #[error("{0}")]
    Io(String),
}

/// What the user types: `path`, `path:40-120`, `!path` (skips the secrets
/// exclusion).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub path: String,
    pub range: Option<(usize, usize)>,
    pub force: bool,
}

impl Spec {
    pub fn parse(raw: &str) -> Result<Spec, AttachError> {
        let mut s = raw.trim();
        let force = s.starts_with('!');
        if force {
            s = &s[1..];
        }
        let (path, range) = match s.rsplit_once(':') {
            Some((p, r))
                if !p.is_empty()
                    && !r.is_empty()
                    && r.chars().all(|c| c.is_ascii_digit() || c == '-') =>
            {
                let (a, b) = match r.split_once('-') {
                    Some((a, b)) => (a, b),
                    None => (r, r),
                };
                let bad = || AttachError::BadRange(r.to_string());
                let a: usize = a.parse().map_err(|_| bad())?;
                let b: usize = if b.is_empty() {
                    usize::MAX
                } else {
                    b.parse().map_err(|_| bad())?
                };
                if a == 0 || b < a {
                    return Err(bad());
                }
                (p.to_string(), Some((a, b)))
            }
            _ => (s.to_string(), None),
        };
        if path.is_empty() {
            return Err(AttachError::NotFound);
        }
        Ok(Spec { path, range, force })
    }

    /// Path with range, without the `!`: what gets displayed.
    pub fn label(&self) -> String {
        match self.range {
            Some((a, b)) if b == usize::MAX => format!("{}:{a}-", self.path),
            Some((a, b)) => format!("{}:{a}-{b}", self.path),
            None => self.path.clone(),
        }
    }
}

impl fmt::Display for Spec {
    /// Persistable form, with the `!` if it has one.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.force {
            write!(f, "!")?;
        }
        write!(f, "{}", self.label())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attachment {
    /// Path exactly as the user typed it.
    pub path: String,
    pub content: String,
    /// Hash of the whole file, to know whether it has changed.
    pub hash: u64,
    /// Token estimate of the attached content.
    pub tokens: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<(usize, usize)>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
}

impl Attachment {
    pub fn label(&self) -> String {
        match self.range {
            Some((a, b)) if b == usize::MAX => format!("{}:{a}-", self.path),
            Some((a, b)) => format!("{}:{a}-{b}", self.path),
            None => self.path.clone(),
        }
    }

    /// Block the model sees.
    pub fn block(&self) -> String {
        let ext = Path::new(&self.path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_string();
        let fence = if self.content.contains("```") {
            "````"
        } else {
            "```"
        };
        let lines = match self.range {
            Some((a, b)) if b == usize::MAX => format!(" lines=\"{a}-\""),
            Some((a, b)) => format!(" lines=\"{a}-{b}\""),
            None => String::new(),
        };
        let note = if self.truncated {
            " truncated=\"true\""
        } else {
            ""
        };
        format!(
            "<file path=\"{}\"{lines}{note}>\n{fence}{ext}\n{}\n{fence}\n</file>",
            self.path,
            self.content.trim_end_matches('\n')
        )
    }
}

/// Approximation: about 3.5 characters per token in text and code.
pub fn estimate_tokens(text: &str) -> u32 {
    (text.chars().count() as f32 / 3.5).ceil() as u32
}

/// Files that are not attached without `!`: credentials and keys.
pub fn is_denied(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    name.starts_with(".env")
        || name.starts_with("id_rsa")
        || name.starts_with("id_ed25519")
        || name.starts_with("id_ecdsa")
        || name.starts_with("id_dsa")
        || name.contains("secret")
        || name.contains("credential")
        || [".pem", ".key", ".p12", ".pfx", ".jks", ".keystore"]
            .iter()
            .any(|ext| name.ends_with(ext))
}

pub fn is_binary(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(8192)];
    if head.contains(&0) {
        return true;
    }
    match std::str::from_utf8(head) {
        Ok(_) => false,
        // a character cut off at the end of the chunk is not binary
        Err(e) => e.valid_up_to() + 4 < head.len(),
    }
}

fn resolve(root: &Path, path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = crate::paths::home_dir() {
            return home.join(rest);
        }
    }
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.join(p)
    }
}

/// Reads a file to attach it: text, with an optional range and truncated to
/// `max_bytes`.
pub fn read_attachment(
    root: &Path,
    spec: &Spec,
    max_bytes: usize,
) -> Result<Attachment, AttachError> {
    let full = resolve(root, &spec.path);
    if !spec.force && is_denied(&full) {
        return Err(AttachError::Denied(spec.label()));
    }
    let meta = std::fs::metadata(&full).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => AttachError::NotFound,
        _ => AttachError::Io(e.to_string()),
    })?;
    if meta.is_dir() {
        return Err(AttachError::IsDir);
    }
    let bytes = std::fs::read(&full).map_err(|e| AttachError::Io(e.to_string()))?;
    if is_binary(&bytes) {
        return Err(AttachError::Binary);
    }
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    let hash = hasher.finish();
    let text = String::from_utf8_lossy(&bytes);
    let mut content = match spec.range {
        Some((a, b)) => {
            let lines: Vec<&str> = text.lines().collect();
            if a > lines.len() {
                return Err(AttachError::BadRange(format!(
                    "{a}-{b} (file has {} lines)",
                    lines.len()
                )));
            }
            let end = b.min(lines.len());
            lines[a - 1..end].join("\n")
        }
        None => text.into_owned(),
    };
    let mut truncated = false;
    if content.len() > max_bytes {
        let mut cut = max_bytes;
        while !content.is_char_boundary(cut) {
            cut -= 1;
        }
        content.truncate(cut);
        content.push_str("\n… [truncated]");
        truncated = true;
    }
    let tokens = estimate_tokens(&content);
    Ok(Attachment {
        path: spec.path.clone(),
        content,
        hash,
        tokens,
        range: spec.range,
        truncated,
    })
}

/// The project context file, if it exists.
pub fn load_context_file(root: &Path, name: &str) -> Result<Option<String>, std::io::Error> {
    if name.trim().is_empty() {
        return Ok(None);
    }
    match std::fs::read_to_string(root.join(name)) {
        Ok(s) if s.trim().is_empty() => Ok(None),
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Full system prompt: the base one, the context file and the live attachments.
pub fn build_system_prompt(
    base: Option<&str>,
    context_file: Option<(&str, &str)>,
    live: &[Attachment],
) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(b) = base.map(str::trim).filter(|b| !b.is_empty()) {
        parts.push(b.to_string());
    }
    if let Some((name, content)) = context_file {
        parts.push(format!(
            "# Project context ({name})\n\n{}",
            content.trim_end()
        ));
    }
    if !live.is_empty() {
        let blocks: Vec<String> = live.iter().map(Attachment::block).collect();
        parts.push(format!(
            "# Attached files (current versions, re-read on every message)\n\n{}",
            blocks.join("\n\n")
        ));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs() {
        let s = Spec::parse("src/app.rs:40-120").unwrap();
        assert_eq!(s.path, "src/app.rs");
        assert_eq!(s.range, Some((40, 120)));
        assert_eq!(s.label(), "src/app.rs:40-120");
        let s = Spec::parse("!.env").unwrap();
        assert!(s.force);
        assert_eq!(s.to_string(), "!.env");
        assert_eq!(Spec::parse("a.rs:7").unwrap().range, Some((7, 7)));
        assert_eq!(Spec::parse("a.rs:7-").unwrap().label(), "a.rs:7-");
        assert!(Spec::parse("a.rs:0-3").is_err());
        assert!(Spec::parse("a.rs:9-3").is_err());
        assert!(Spec::parse("").is_err());
    }

    #[test]
    fn reads_ranges_and_limits() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "l1\nl2\nl3\nl4\n").unwrap();
        std::fs::create_dir(dir.path().join("d")).unwrap();
        std::fs::write(dir.path().join("b.bin"), [0u8, 1, 2, 3]).unwrap();
        std::fs::write(dir.path().join(".env"), "SECRET=1").unwrap();

        let a = read_attachment(dir.path(), &Spec::parse("a.txt").unwrap(), 1000).unwrap();
        assert_eq!(a.content, "l1\nl2\nl3\nl4\n");
        assert!(a.tokens > 0);
        let r = read_attachment(dir.path(), &Spec::parse("a.txt:2-3").unwrap(), 1000).unwrap();
        assert_eq!(r.content, "l2\nl3");
        assert_eq!(r.label(), "a.txt:2-3");
        assert!(matches!(
            read_attachment(dir.path(), &Spec::parse("a.txt:9-").unwrap(), 1000),
            Err(AttachError::BadRange(_))
        ));
        assert!(matches!(
            read_attachment(dir.path(), &Spec::parse("nope").unwrap(), 1000),
            Err(AttachError::NotFound)
        ));
        assert!(matches!(
            read_attachment(dir.path(), &Spec::parse("d").unwrap(), 1000),
            Err(AttachError::IsDir)
        ));
        assert!(matches!(
            read_attachment(dir.path(), &Spec::parse("b.bin").unwrap(), 1000),
            Err(AttachError::Binary)
        ));
        assert!(matches!(
            read_attachment(dir.path(), &Spec::parse(".env").unwrap(), 1000),
            Err(AttachError::Denied(_))
        ));
        assert!(read_attachment(dir.path(), &Spec::parse("!.env").unwrap(), 1000).is_ok());
        let t = read_attachment(dir.path(), &Spec::parse("a.txt").unwrap(), 5).unwrap();
        assert!(t.truncated);
        assert!(t.content.ends_with("[truncated]"));
    }

    #[test]
    fn block_and_system_prompt() {
        let a = Attachment {
            path: "x.rs".into(),
            content: "fn main() {}\n".into(),
            hash: 1,
            tokens: 4,
            range: Some((1, 1)),
            truncated: false,
        };
        let b = a.block();
        assert!(
            b.starts_with("<file path=\"x.rs\" lines=\"1-1\">\n```rs\nfn main() {}\n```\n</file>")
        );
        let sp = build_system_prompt(Some("be brief"), Some(("MOON.md", "rust project\n")), &[a])
            .unwrap();
        assert!(sp.starts_with(
            "be brief\n\n# Project context (MOON.md)\n\nrust project\n\n# Attached files"
        ));
        assert!(sp.contains("<file path=\"x.rs\""));
        assert_eq!(build_system_prompt(None, None, &[]), None);
        assert!(is_denied(Path::new("/x/id_rsa.pub")));
        assert!(is_denied(Path::new("server.key")));
        assert!(!is_denied(Path::new("keyboard.rs")));
        assert!(!is_binary("text\n".as_bytes()));
        assert!(is_binary(&[0x89, b'P', b'N', b'G', 0, 0]));
    }
}
