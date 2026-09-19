//! Execution of the slash commands parsed by `crate::commands`.

use super::*;

impl App {
    // ----- commands ---------------------------------------------------------

    pub(super) fn run_command(&mut self, cmd: Command, tx: &Tx) {
        match cmd {
            Command::Model => self.open_model_picker("", tx),
            Command::Sessions => self.open_sessions_picker(),
            Command::Provider(None) => {
                let text = providers_table(
                    &self.providers,
                    self.registry.disabled(),
                    self.default_provider.as_deref(),
                );
                self.push_item(Item::Info(text));
                self.follow = true;
            }
            Command::Provider(Some(id)) => {
                if self.registry.has(&id) {
                    self.default_provider = Some(id.clone());
                    self.notify(format!("default provider: {id}"));
                } else {
                    self.notify(format!(
                        "unknown provider: {id} (known: {})",
                        self.registry.ids().join(", ")
                    ));
                }
            }
            Command::New => {
                if self.is_streaming() {
                    self.cancel_generation();
                }
                self.items.clear();
                self.cache.clear();
                self.view_from = 0;
                self.session = None;
                self.last_usage = None;
                self.last_run = None;
                self.follow = true;
                self.live.clear();
                self.load_context_file();
                self.notify("new conversation");
            }
            Command::Clear => {
                self.view_from = self.items.len();
                self.follow = true;
            }
            Command::System(None) => {
                let sp = self
                    .system_prompt
                    .clone()
                    .unwrap_or_else(|| "(none)".into());
                self.notify(format!("system: {sp}"));
            }
            Command::System(Some(t)) => {
                self.system_prompt = if t == "none" || t == "-" {
                    None
                } else {
                    Some(t)
                };
                self.update_session_meta();
                self.notify("system prompt updated");
            }
            Command::Params(text) => match params::parse_pairs(&text) {
                Err(e) => self.notify(e),
                Ok(pairs) if pairs.is_empty() => {
                    let s = self.params.summary();
                    self.notify(format!("params: {s}"));
                }
                Ok(pairs) => {
                    for (k, v) in pairs {
                        if let Err(e) = self.params.apply(&k, &v) {
                            self.notify(e);
                            return;
                        }
                    }
                    let s = self.params.summary();
                    self.notify(format!("params: {s}"));
                }
            },
            Command::Save(name) => {
                if self.messages().next().is_none() {
                    self.notify("nothing to save yet");
                    return;
                }
                if self.store.is_none() {
                    self.notify("sessions are disabled (save_sessions = false)");
                    return;
                }
                if self.session.is_none() {
                    let first = self
                        .messages()
                        .next()
                        .map(|m| m.content.clone())
                        .unwrap_or_default();
                    self.cfg.general.save_sessions = true;
                    self.ensure_session(&first);
                    self.rewrite_session();
                }
                if let Some(n) = name {
                    if let Some(meta) = self.session.as_mut() {
                        meta.title = n;
                    }
                    self.update_session_meta();
                }
                if let Some(meta) = &self.session {
                    let msg = format!("saved: {} · {}", meta.title, meta.path.display());
                    self.notify(msg);
                }
            }
            Command::Export(path) => self.export(path),
            Command::Copy => {
                let last = self
                    .messages()
                    .rev()
                    .find(|m| m.role == Role::Assistant)
                    .map(|m| m.content.clone());
                match last {
                    None => self.notify("no reply to copy"),
                    Some(text) => match crate::clipboard::copy(&text) {
                        Ok(via) => self.notify(format!("reply copied ({via})")),
                        Err(e) => self.notify(format!("could not copy: {e}")),
                    },
                }
            }
            Command::Retry => {
                if self.is_streaming() {
                    self.cancel_generation();
                }
                self.drop_trailing_non_messages();
                if matches!(self.items.last(), Some(Item::Message(m)) if m.role == Role::Assistant)
                {
                    self.pop_item();
                    self.rewrite_session();
                }
                if matches!(self.items.last(), Some(Item::Message(m)) if m.role == Role::User) {
                    self.follow = true;
                    self.start_generation(tx);
                } else {
                    self.notify("nothing to retry");
                }
            }
            Command::Undo => {
                if self.is_streaming() {
                    self.cancel_generation();
                }
                self.drop_trailing_non_messages();
                let mut removed = 0;
                if matches!(self.items.last(), Some(Item::Message(m)) if m.role == Role::Assistant)
                {
                    self.pop_item();
                    removed += 1;
                }
                if matches!(self.items.last(), Some(Item::Message(m)) if m.role == Role::User) {
                    self.pop_item();
                    removed += 1;
                }
                if removed > 0 {
                    self.view_from = self.view_from.min(self.items.len());
                    self.rewrite_session();
                    self.notify("last exchange removed");
                } else {
                    self.notify("nothing to undo");
                }
            }
            Command::Files => self.open_files_panel(),
            Command::Context => self.show_context(),
            Command::Help => self.panel = Some(Panel::Help(HelpState::default())),
            Command::Quit => self.should_quit = true,
        }
    }

    pub(super) fn drop_trailing_non_messages(&mut self) {
        while matches!(
            self.items.last(),
            Some(Item::Error(_)) | Some(Item::Info(_))
        ) {
            self.pop_item();
        }
    }

    pub(super) fn export(&mut self, path: Option<String>) {
        let messages: Vec<Message> = self.messages().cloned().collect();
        if messages.is_empty() {
            self.notify("nothing to export");
            return;
        }
        let meta = self.session.clone().unwrap_or_else(|| SessionMeta {
            id: "unsaved".into(),
            title: title_from(&messages[0].content),
            created_at: messages[0].ts,
            model: self.current.as_ref().map(|c| c.qualified()),
            system_prompt: self.system_prompt.clone(),
            attachments: self.live.iter().map(|s| s.to_string()).collect(),
            path: Default::default(),
        });
        let path = path.unwrap_or_else(|| {
            let slug: String = meta
                .title
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() {
                        c.to_ascii_lowercase()
                    } else {
                        '-'
                    }
                })
                .collect::<String>()
                .trim_matches('-')
                .chars()
                .take(40)
                .collect();
            format!(
                "moon-{}.md",
                if slug.is_empty() {
                    meta.id.clone()
                } else {
                    slug
                }
            )
        });
        let md = export_markdown(&Session { meta, messages });
        match std::fs::write(&path, md) {
            Ok(()) => self.notify(format!("exported to {path}")),
            Err(e) => self.notify(format!("could not write {path}: {e}")),
        }
    }
}

pub(super) fn providers_table(
    providers: &[ProviderState],
    disabled: &std::collections::BTreeMap<String, String>,
    default: Option<&str>,
) -> String {
    let mut out = Vec::new();
    for p in providers {
        let mark = if default == Some(p.id.as_str()) {
            "●"
        } else {
            " "
        };
        let state = match &p.health {
            None => "checking…".to_string(),
            Some(Ok(h)) => {
                let mut s = "ok".to_string();
                if let Some(v) = &h.version {
                    s.push_str(&format!(" · v{v}"));
                }
                if let Some(d) = &h.detail {
                    s.push_str(&format!(" · {d}"));
                }
                s
            }
            Some(Err(e)) => format!("✗ {e}"),
        };
        out.push(format!(
            "{mark}{} · {} · {} · {}",
            p.id, p.kind, p.base_url, state
        ));
    }
    for (id, why) in disabled {
        out.push(format!("–{id} · disabled: {why}"));
    }
    if out.is_empty() {
        out.push("no providers configured".into());
    }
    out.join("\n")
}
