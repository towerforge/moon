//! Model and session pickers, switching models, loading saved sessions.

use super::*;

impl App {
    // ----- models and sessions ----------------------------------------------

    pub fn open_model_picker(&mut self, query: &str, tx: &Tx) {
        self.panel = Some(Panel::Models(self.model_picker(query)));
        self.refresh_models(tx);
    }

    /// Model picker: recent ones at the top and one section per provider with
    /// its status in the header.
    pub(crate) fn model_picker(&self, query: &str) -> Picker {
        // with few models they all fit under their provider: repeating some of
        // them on top helps nobody
        let recent: Vec<String> = if self.models.len() < RECENT_MIN {
            Vec::new()
        } else {
            self.recent
                .iter()
                .filter(|q| self.models.iter().any(|m| m.qualified() == **q))
                .cloned()
                .collect()
        };
        let items = model_items(
            &self.models,
            &self.providers,
            self.current.as_ref(),
            &recent,
        );
        let mut p = Picker::new("Select model", items, query);
        p.hint = "the conversation keeps its history · type to filter".into();
        p.groups = model_groups(&self.models, &self.providers, recent.len());
        p.title_info = format!(
            "{} {} · {} {}",
            self.models.len(),
            plural(self.models.len(), "model"),
            self.providers.len(),
            plural(self.providers.len(), "provider"),
        );
        p.empty_text = if self.models.is_empty() {
            "no models: is the provider running?".into()
        } else {
            "no model matches".into()
        };
        p
    }

    pub(super) fn set_model(&mut self, provider: String, model: String, tx: &Tx) {
        let q = format!("{provider}/{model}");
        self.ctx_len = self
            .models
            .iter()
            .find(|m| m.provider == provider && m.id == model)
            .and_then(|m| m.context_length);
        self.current = Some(Current { provider, model });
        self.caps = None;
        self.loaded = LoadedState::Unknown;
        push_recent(&mut self.recent, &q);
        save_recent(self.recent_file.as_deref(), &self.recent);
        self.fetch_model_state(tx);
        self.update_session_meta();
        self.notify(format!("model: {q}"));
    }

    /// Sessions list: the last ones opened on top, then all of them in
    /// alphabetical order. No dates: the title is what you look for, and the
    /// filter runs on it.
    pub(super) fn open_sessions_picker(&mut self) {
        let Some(store) = &self.store else {
            self.notify("sessions are disabled (save_sessions = false)");
            return;
        };
        let mut list = match store.list() {
            Ok(l) => l,
            Err(e) => {
                self.notify(format!("could not list sessions: {e}"));
                return;
            }
        };
        if list.is_empty() {
            self.notify("no saved sessions yet");
            return;
        }
        list.sort_by(|a, b| {
            a.title
                .to_lowercase()
                .cmp(&b.title.to_lowercase())
                .then(a.created_at.cmp(&b.created_at))
        });
        let current = self.session.as_ref().map(|s| s.id.clone());
        let n = list.len();
        let item = |m: &SessionMeta, group: &str| PickerItem {
            id: m.id.clone(),
            key: format!("{} {}", m.title, m.model.clone().unwrap_or_default()),
            label: m.title.clone(),
            detail: m.model.clone().unwrap_or_default(),
            active: current.as_deref() == Some(m.id.as_str()),
            dim: false,
            group: Some(group.to_string()),
        };
        // the recent ones keep the order they were opened in, not the
        // alphabet; with few sessions the whole list is right there and
        // repeating some on top helps nobody
        let recent: Vec<&SessionMeta> = if list.len() < RECENT_MIN {
            Vec::new()
        } else {
            self.recent_sessions
                .iter()
                .filter_map(|id| list.iter().find(|m| &m.id == id))
                .collect()
        };
        let mut items: Vec<PickerItem> = recent.iter().map(|m| item(m, RECENT_GROUP)).collect();
        items.extend(list.iter().map(|m| item(m, ALL_GROUP)));
        let mut p = Picker::new("Resume a session", items, "");
        p.hint = "the conversation on screen is replaced · type to filter".into();
        p.groups = Vec::new();
        if !recent.is_empty() {
            p.groups.push(PickerGroup {
                title: RECENT_GROUP.into(),
                info: format!("{} {}", recent.len(), plural(recent.len(), "session")),
                mark: None,
            });
        }
        p.groups.push(PickerGroup {
            title: ALL_GROUP.into(),
            info: format!("{n} {}", plural(n, "session")),
            mark: None,
        });
        p.title_info = format!("{n} {}", plural(n, "session"));
        p.keys = vec![
            ("↑↓", "move"),
            ("number", "jump"),
            ("enter", "resume"),
            ("ctrl+r", "rename"),
            ("ctrl+d", "delete"),
            ("esc", "close"),
        ];
        p.empty_text = "no session matches".into();
        self.panel = Some(Panel::Sessions(p));
    }

    /// Notes a session as just used, so it climbs to the top of the list.
    pub(super) fn touch_session(&mut self, id: &str) {
        push_recent(&mut self.recent_sessions, id);
        save_recent(self.recent_sessions_file.as_deref(), &self.recent_sessions);
    }

    pub(super) fn forget_session(&mut self, id: &str) {
        self.recent_sessions.retain(|r| r != id);
        save_recent(self.recent_sessions_file.as_deref(), &self.recent_sessions);
    }

    pub(super) fn load_session(&mut self, id: &str, tx: &Tx) {
        let Some(store) = &self.store else { return };
        match store.load(id) {
            Ok(s) => {
                if self.is_streaming() {
                    self.cancel_generation();
                }
                self.apply_session(s);
                let title = self
                    .session
                    .as_ref()
                    .map(|s| s.title.clone())
                    .unwrap_or_default();
                self.notify(format!("session resumed: {title}"));
                self.fetch_model_state(tx);
            }
            Err(e) => self.notify(format!("could not load the session: {e}")),
        }
    }

    pub(super) fn apply_session(&mut self, s: Session) {
        self.items = s.messages.into_iter().map(Item::Message).collect();
        self.cache = Vec::new();
        self.cache.resize_with(self.items.len(), || None);
        self.view_from = 0;
        self.follow = true;
        let last_usage = self.messages().rev().find_map(|m| m.usage.clone());
        self.last_usage = last_usage;
        self.last_run = None;
        if let Some(sp) = &s.meta.system_prompt {
            self.system_prompt = Some(sp.clone());
        }
        if let Some(q) = &s.meta.model {
            if let Ok((p, m)) = self.registry.resolve(q, None) {
                self.current = Some(Current {
                    provider: p.id().to_string(),
                    model: m,
                });
                self.ctx_len = None;
                self.caps = None;
                self.loaded = LoadedState::Unknown;
            }
        }
        // the conversation had the file tools: back on, unless they are
        if s.meta.tools && !self.tools_on {
            if let Err(e) = self.enable_tools() {
                self.notify(e);
            }
        }
        if s.meta.tools && self.tools_on {
            self.set_tools_scope(s.meta.tools_edit, s.meta.tools_create);
        }
        self.live = s
            .meta
            .attachments
            .iter()
            .filter_map(|a| Spec::parse(a).ok())
            .collect();
        self.touch_session(&s.meta.id);
        self.session = Some(s.meta);
    }
}

pub(super) fn model_items(
    models: &[ModelInfo],
    providers: &[ProviderState],
    current: Option<&Current>,
    recent: &[String],
) -> Vec<PickerItem> {
    let detail_of = |m: &ModelInfo| -> String {
        let mut detail: Vec<String> = Vec::new();
        if let Some(p) = &m.parameter_size {
            detail.push(p.clone());
        }
        if let Some(q) = &m.quantization {
            detail.push(q.clone());
        }
        if let Some(s) = m.size_bytes {
            detail.push(fmt_size(s));
        }
        if let Some(c) = m.context_length {
            detail.push(format!("ctx {}", fmt_k(c)));
        }
        detail.join(" · ")
    };
    let item = |m: &ModelInfo, group: &str, detail: String| PickerItem {
        id: m.qualified(),
        key: m.qualified(),
        label: m.id.clone(),
        detail,
        active: current.is_some_and(|c| c.provider == m.provider && c.model == m.id),
        dim: false,
        group: Some(group.to_string()),
    };
    let mut items = Vec::new();
    // recent ones at the top, with the provider before the detail
    for q in recent {
        if let Some(m) = models.iter().find(|m| m.qualified() == *q) {
            let d = detail_of(m);
            let detail = if d.is_empty() {
                m.provider.clone()
            } else {
                format!("{} · {d}", m.provider)
            };
            items.push(item(m, RECENT_GROUP, detail));
        }
    }
    // one section per provider, in configuration order
    for p in providers {
        for m in models.iter().filter(|m| m.provider == p.id) {
            items.push(item(m, &p.id, detail_of(m)));
        }
    }
    for m in models
        .iter()
        .filter(|m| !providers.iter().any(|p| p.id == m.provider))
    {
        items.push(item(m, &m.provider, detail_of(m)));
    }
    items
}

/// Picker headers: `Recent` if any, plus each provider with its status.
pub(super) fn model_groups(
    models: &[ModelInfo],
    providers: &[ProviderState],
    recent_n: usize,
) -> Vec<PickerGroup> {
    let mut groups = Vec::new();
    if recent_n > 0 {
        groups.push(PickerGroup {
            title: RECENT_GROUP.into(),
            info: format!("{recent_n} {}", plural(recent_n, "model")),
            mark: None,
        });
    }
    for p in providers {
        let (mark, info) = match &p.health {
            Some(Ok(_)) => (Some(true), host_of(&p.base_url)),
            Some(Err(e)) => (Some(false), e.clone()),
            None => (None, "checking…".to_string()),
        };
        groups.push(PickerGroup {
            title: p.id.clone(),
            info,
            mark,
        });
    }
    for m in models {
        if !groups.iter().any(|g| g.title == m.provider) {
            groups.push(PickerGroup {
                title: m.provider.clone(),
                info: String::new(),
                mark: None,
            });
        }
    }
    groups
}

/// `http://localhost:11434/` → `localhost:11434`.
pub(super) fn host_of(url: &str) -> String {
    url.trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/')
        .to_string()
}

pub(super) fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        word.to_string()
    } else {
        format!("{word}s")
    }
}

/// Puts `id` at the head of a recent list, without repeating it, and keeps
/// only the last `RECENT_MAX`.
pub(super) fn push_recent(list: &mut Vec<String>, id: &str) {
    list.retain(|r| r != id);
    list.insert(0, id.to_string());
    list.truncate(RECENT_MAX);
}

/// Writes a recent list, one per line. A failure is only worth a log: it
/// costs the order of a list, nothing else.
pub(super) fn save_recent(path: Option<&Path>, list: &[String]) {
    let Some(path) = path else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(path, list.join("\n")) {
        tracing::warn!(error = %e, "could not save the recent list");
    }
}

/// The saved list of recent models or sessions, one per line.
pub(super) fn load_recent(path: Option<&Path>) -> Vec<String> {
    let Some(path) = path else {
        return Vec::new();
    };
    std::fs::read_to_string(path)
        .map(|s| {
            s.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(String::from)
                .take(RECENT_MAX)
                .collect()
        })
        .unwrap_or_default()
}
