//! A note on disk with what the last check found, so that the start-up check
//! asks GitHub once a day and not on every run.

use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{Client, Version};

pub const FILE: &str = "update-check.json";
/// How long a check is good for. A release is not news by the minute.
pub const TTL: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CheckCache {
    pub checked_at: DateTime<Utc>,
    /// Newest version GitHub had at that moment.
    pub latest: String,
}

impl CheckCache {
    pub fn new(latest: &Version) -> Self {
        Self {
            checked_at: Utc::now(),
            latest: latest.to_string(),
        }
    }

    pub fn path(state_dir: &Path) -> PathBuf {
        state_dir.join(FILE)
    }

    /// Reads the note. Anything wrong with it — missing, unreadable, from an
    /// older format — simply means there is no note.
    pub fn load(state_dir: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(Self::path(state_dir)).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn save(&self, state_dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(state_dir)?;
        let text = serde_json::to_string(self).unwrap_or_default();
        std::fs::write(Self::path(state_dir), text)
    }

    /// Forgets the check. Done right after installing, so the start-up notice
    /// does not survive the update that answered it.
    pub fn forget(state_dir: &Path) {
        let _ = std::fs::remove_file(Self::path(state_dir));
    }

    pub fn fresh_at(&self, now: DateTime<Utc>, ttl: Duration) -> bool {
        match (now - self.checked_at).to_std() {
            Ok(age) => age < ttl,
            // checked in the future: the clock moved, do not trust it
            Err(_) => false,
        }
    }

    pub fn fresh(&self, ttl: Duration) -> bool {
        self.fresh_at(Utc::now(), ttl)
    }

    pub fn version(&self) -> Option<Version> {
        self.latest.parse().ok()
    }
}

/// The published version that beats `current`, if there is one: from the note
/// on disk while it is fresh, from GitHub when it is not. Every failure is a
/// `None`; nobody is told that the check could not be made.
pub async fn newer_than(
    current: &Version,
    state_dir: Option<&Path>,
    ttl: Duration,
) -> Option<Version> {
    let cached = state_dir.and_then(CheckCache::load);
    if let Some(c) = &cached {
        if c.fresh(ttl) {
            return c.version().filter(|v| v > current);
        }
    }
    let latest = Client::new().ok()?.latest().await.ok()?.version;
    if let Some(dir) = state_dir {
        let _ = CheckCache::new(&latest).save(dir);
    }
    (latest > *current).then_some(latest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        s.parse().unwrap()
    }

    #[test]
    fn the_note_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        assert!(CheckCache::load(dir.path()).is_none());
        let c = CheckCache::new(&v("0.2.0"));
        c.save(dir.path()).unwrap();
        let back = CheckCache::load(dir.path()).unwrap();
        assert_eq!(back.version(), Some(v("0.2.0")));
        assert!(back.fresh(TTL));
        CheckCache::forget(dir.path());
        assert!(CheckCache::load(dir.path()).is_none());
    }

    #[test]
    fn a_note_from_yesterday_is_stale() {
        let c = CheckCache {
            checked_at: Utc::now() - chrono::Duration::hours(25),
            latest: "0.2.0".into(),
        };
        assert!(!c.fresh(TTL));
        assert!(c.fresh(Duration::from_secs(48 * 3600)));
    }

    #[test]
    fn a_note_from_the_future_is_not_believed() {
        let c = CheckCache {
            checked_at: Utc::now() + chrono::Duration::hours(2),
            latest: "0.2.0".into(),
        };
        assert!(!c.fresh(TTL));
    }

    #[test]
    fn an_unreadable_note_counts_as_none() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(CheckCache::path(dir.path()), "{ nope").unwrap();
        assert!(CheckCache::load(dir.path()).is_none());
    }
}
