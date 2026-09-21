//! Sessions as JSONL, one file per conversation, append-only. The first
//! line is `meta`; the rest, messages.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::SessionError;
use crate::types::{Message, Role};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionMeta {
    pub id: String,
    pub title: String,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    /// Live attachments (the files panel), by path; re-read on resume.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<String>,
    #[serde(skip)]
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub meta: SessionMeta,
    pub messages: Vec<Message>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Record {
    Meta(SessionMeta),
    Message(Message),
}

#[derive(Debug, Clone)]
pub struct SessionStore {
    dir: PathBuf,
}

const TITLE_MAX: usize = 60;

/// Title from the first message: first line, 60 characters.
pub fn title_from(text: &str) -> String {
    let first = text
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    let mut t: String = first.chars().take(TITLE_MAX).collect();
    if first.chars().count() > TITLE_MAX {
        t.push('…');
    }
    if t.is_empty() {
        "untitled".to_string()
    } else {
        t
    }
}

fn io_err(path: &Path) -> impl FnOnce(std::io::Error) -> SessionError + '_ {
    move |source| SessionError::Io {
        path: path.to_path_buf(),
        source,
    }
}

impl SessionStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Creates the file with the `meta` line.
    pub fn create(
        &self,
        title: &str,
        model: Option<String>,
        system_prompt: Option<String>,
    ) -> Result<SessionMeta, SessionError> {
        fs::create_dir_all(&self.dir).map_err(io_err(&self.dir))?;
        let created_at = Utc::now();
        let id = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
        let name = format!("{}_{}.jsonl", created_at.format("%Y-%m-%dT%H-%M-%S"), id);
        let meta = SessionMeta {
            id,
            title: title.to_string(),
            created_at,
            model,
            system_prompt,
            attachments: Vec::new(),
            path: self.dir.join(name),
        };
        let mut f = fs::File::create(&meta.path).map_err(io_err(&meta.path))?;
        write_record(&mut f, &Record::Meta(meta.clone()), &meta.path)?;
        Ok(meta)
    }

    pub fn append(&self, meta: &SessionMeta, msg: &Message) -> Result<(), SessionError> {
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(&meta.path)
            .map_err(io_err(&meta.path))?;
        write_record(&mut f, &Record::Message(msg.clone()), &meta.path)
    }

    /// Rewrites the whole file: new `meta` and these messages.
    pub fn rewrite(&self, meta: &SessionMeta, messages: &[Message]) -> Result<(), SessionError> {
        let tmp = meta.path.with_extension("jsonl.tmp");
        {
            let mut f = fs::File::create(&tmp).map_err(io_err(&tmp))?;
            write_record(&mut f, &Record::Meta(meta.clone()), &tmp)?;
            for m in messages {
                write_record(&mut f, &Record::Message(m.clone()), &tmp)?;
            }
        }
        fs::rename(&tmp, &meta.path).map_err(io_err(&meta.path))
    }

    /// Changes only the first line (title, model, system prompt).
    pub fn update_meta(&self, meta: &SessionMeta) -> Result<(), SessionError> {
        let s = self.load_path(&meta.path)?;
        self.rewrite(meta, &s.messages)
    }

    /// Every session, most recent first. Reads only the `meta` line.
    pub fn list(&self) -> Result<Vec<SessionMeta>, SessionError> {
        let rd = match fs::read_dir(&self.dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io_err(&self.dir)(e)),
        };
        let mut out = Vec::new();
        for entry in rd.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            if let Some(meta) = read_meta(&path)? {
                out.push(meta);
            }
        }
        out.sort_by_key(|m| std::cmp::Reverse(m.created_at));
        Ok(out)
    }

    pub fn latest(&self) -> Result<Option<SessionMeta>, SessionError> {
        Ok(self.list()?.into_iter().next())
    }

    /// Loads by id (or id prefix).
    pub fn load(&self, id: &str) -> Result<Session, SessionError> {
        let meta = self
            .list()?
            .into_iter()
            .find(|m| m.id == id || m.id.starts_with(id))
            .ok_or_else(|| SessionError::NotFound(id.to_string()))?;
        self.load_path(&meta.path)
    }

    /// Deletes a session's file by id (or id prefix) and returns its metadata.
    pub fn delete(&self, id: &str) -> Result<SessionMeta, SessionError> {
        let meta = self
            .list()?
            .into_iter()
            .find(|m| m.id == id || m.id.starts_with(id))
            .ok_or_else(|| SessionError::NotFound(id.to_string()))?;
        fs::remove_file(&meta.path).map_err(io_err(&meta.path))?;
        Ok(meta)
    }

    pub fn load_path(&self, path: &Path) -> Result<Session, SessionError> {
        let f = fs::File::open(path).map_err(io_err(path))?;
        let mut meta = None;
        let mut messages = Vec::new();
        for (i, line) in BufReader::new(f).lines().enumerate() {
            let line = line.map_err(io_err(path))?;
            if line.trim().is_empty() {
                continue;
            }
            let rec: Record =
                serde_json::from_str(&line).map_err(|source| SessionError::Parse {
                    path: path.to_path_buf(),
                    line: i + 1,
                    source,
                })?;
            match rec {
                Record::Meta(mut m) => {
                    m.path = path.to_path_buf();
                    meta = Some(m);
                }
                Record::Message(m) => messages.push(m),
            }
        }
        let meta = meta.ok_or_else(|| SessionError::NotFound(path.display().to_string()))?;
        Ok(Session { meta, messages })
    }
}

fn write_record(f: &mut fs::File, rec: &Record, path: &Path) -> Result<(), SessionError> {
    let mut line = serde_json::to_string(rec).map_err(|e| SessionError::Io {
        path: path.to_path_buf(),
        source: std::io::Error::other(e),
    })?;
    line.push('\n');
    f.write_all(line.as_bytes()).map_err(io_err(path))
}

fn read_meta(path: &Path) -> Result<Option<SessionMeta>, SessionError> {
    let f = fs::File::open(path).map_err(io_err(path))?;
    let mut first = String::new();
    BufReader::new(f)
        .read_line(&mut first)
        .map_err(io_err(path))?;
    if first.trim().is_empty() {
        return Ok(None);
    }
    match serde_json::from_str::<Record>(&first) {
        Ok(Record::Meta(mut m)) => {
            m.path = path.to_path_buf();
            Ok(Some(m))
        }
        _ => Ok(None),
    }
}

/// Markdown dump for `/export`.
pub fn export_markdown(session: &Session) -> String {
    let mut out = format!("# {}\n\n", session.meta.title);
    out.push_str(&format!(
        "_{}{}_\n\n",
        session.meta.created_at.format("%Y-%m-%d %H:%M"),
        session
            .meta
            .model
            .as_deref()
            .map(|m| format!(" · {m}"))
            .unwrap_or_default()
    ));
    if let Some(sp) = &session.meta.system_prompt {
        out.push_str(&format!("> system: {sp}\n\n"));
    }
    for m in &session.messages {
        match m.role {
            Role::User => {
                out.push_str(&format!("**❯** {}\n\n", m.content));
                for a in &m.attachments {
                    out.push_str(&format!("_attached: {} · {} tok_\n\n", a.label(), a.tokens));
                }
            }
            Role::Assistant => {
                out.push_str(&m.content);
                if m.partial {
                    out.push_str("\n\n_(reply cancelled)_");
                }
                out.push_str("\n\n");
            }
            Role::System => out.push_str(&format!("> system: {}\n\n", m.content)),
            Role::Tool => out.push_str(&format!("```\n{}\n```\n\n", m.content)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_cycle() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(dir.path().join("sessions"));
        assert!(store.list().unwrap().is_empty());

        let meta = store
            .create("hello", Some("ollama/x".into()), None)
            .unwrap();
        store.append(&meta, &Message::user("hello")).unwrap();
        let mut a = Message::assistant("how are you");
        a.model = Some("ollama/x".into());
        store.append(&meta, &a).unwrap();

        let list = store.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "hello");

        let s = store.load(&meta.id[..4]).unwrap();
        assert_eq!(s.messages.len(), 2);
        assert_eq!(s.messages[1].model.as_deref(), Some("ollama/x"));

        let mut renamed = s.meta.clone();
        renamed.title = "other".into();
        store.update_meta(&renamed).unwrap();
        assert_eq!(store.latest().unwrap().unwrap().title, "other");

        store.rewrite(&renamed, &s.messages[..1]).unwrap();
        assert_eq!(store.load(&meta.id).unwrap().messages.len(), 1);

        let md = export_markdown(&store.load(&meta.id).unwrap());
        assert!(md.starts_with("# other\n"));
        assert!(md.contains("**❯** hello"));

        assert_eq!(store.delete(&meta.id[..4]).unwrap().title, "other");
        assert!(store.list().unwrap().is_empty());
        assert!(matches!(
            store.delete(&meta.id),
            Err(SessionError::NotFound(_))
        ));
    }

    #[test]
    fn title() {
        assert_eq!(title_from("  \n first line\nmore"), "first line");
        assert_eq!(title_from(""), "untitled");
        let long = "a".repeat(80);
        let t = title_from(&long);
        assert_eq!(t.chars().count(), 61);
        assert!(t.ends_with('…'));
    }
}
