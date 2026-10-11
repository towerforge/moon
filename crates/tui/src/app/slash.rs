//! Execution of the slash commands parsed by `crate::commands`.

use super::*;
use crate::commands::CopyTarget;

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
                self.notify(text);
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
            Command::Copy(target) => self.copy_last(target),
            Command::Retry => {
                if self.turn_active() {
                    self.cancel_generation();
                }
                // everything the last question brought: the reply, and with
                // tools the calls, their results and the step lines
                if self.pop_to_last_request() > 0 {
                    self.rewrite_session();
                }
                if matches!(self.items.last(), Some(Item::Message(m)) if m.role == Role::User) {
                    self.follow = true;
                    if let Some(h) = self.harness.as_mut() {
                        h.begin_turn();
                    }
                    self.start_generation(tx);
                } else {
                    self.notify("nothing to retry");
                }
            }
            Command::Undo => {
                if self.turn_active() {
                    self.cancel_generation();
                }
                let mut removed = self.pop_to_last_request();
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
            Command::Tools => {
                self.notify("the permissions live in /agent now · opening it");
                self.open_agents_permissions();
            }
            Command::Agent => self.open_agent_picker(),
            Command::Context => self.show_context(),
            Command::Thinking => {
                self.show_thinking = !self.show_thinking;
                // every reply with reasoning is drawn again
                self.cache.iter_mut().for_each(|c| *c = None);
                self.notify(if self.show_thinking {
                    "reasoning shown over each reply · /thinking hides it"
                } else {
                    "reasoning hidden · /thinking shows it"
                });
            }
            Command::Machine => self.panel = Some(Panel::Machine),
            Command::Version => {
                let note = self.version_note();
                self.notify(note);
            }
            Command::Help => self.panel = Some(Panel::Help(HelpState::default())),
            Command::Quit => self.should_quit = true,
        }
    }

    pub(super) fn drop_trailing_non_messages(&mut self) {
        while matches!(
            self.items.last(),
            Some(Item::Error(_))
                | Some(Item::Info(_))
                | Some(Item::Step(_))
                | Some(Item::Tools(_))
                | Some(Item::Command { .. })
        ) {
            self.pop_item();
        }
    }

    /// Removes everything after the last user message: replies, tool calls
    /// and results, step lines, notices. Returns how many messages went.
    pub(super) fn pop_to_last_request(&mut self) -> usize {
        let mut removed = 0;
        loop {
            self.drop_trailing_non_messages();
            match self.items.last() {
                Some(Item::Message(m)) if m.role != Role::User => {
                    self.pop_item();
                    removed += 1;
                }
                _ => break,
            }
        }
        removed
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
            tools: self.tools_on,
            tools_edit: self.tools_scope().0,
            tools_create: self.tools_scope().1,
            agent: (self.agent != DEFAULT_AGENT).then(|| self.agent.clone()),
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

impl App {
    /// `/copy`: the last reply, or one of its code blocks.
    fn copy_last(&mut self, target: CopyTarget) {
        let reply = self
            .messages()
            .rev()
            .find(|m| m.role == Role::Assistant)
            .map(|m| m.content.as_str());
        match pick_copy(reply, target) {
            Err(e) => self.notify(e),
            Ok((text, what)) => match crate::clipboard::copy(&text) {
                Ok(via) => self.notify(format!("{what} copied ({via})")),
                Err(e) => self.notify(format!("could not copy: {e}")),
            },
        }
    }
}

/// What `/copy` puts on the clipboard, and how the notice names it. A code
/// block loses its last newline, so a command pasted into a shell waits for
/// enter instead of running.
fn pick_copy(reply: Option<&str>, target: CopyTarget) -> Result<(String, String), String> {
    let reply = reply.ok_or_else(|| "no reply to copy".to_string())?;
    let CopyTarget::Code(n) = target else {
        return Ok((reply.to_string(), "reply".into()));
    };
    let blocks = crate::markdown::code_blocks(reply);
    let total = blocks.len();
    let index = match n {
        None => total.checked_sub(1),
        Some(n) => (n <= total).then(|| n - 1),
    };
    let Some(i) = index else {
        return Err(match total {
            0 => "no code block in the last reply".into(),
            1 => "the last reply has 1 code block".into(),
            _ => format!("the last reply has {total} code blocks"),
        });
    };
    let mut text = blocks.into_iter().nth(i).unwrap_or_default();
    if text.ends_with('\n') {
        text.pop();
    }
    let what = if total == 1 {
        "code block".to_string()
    } else {
        format!("code block {} of {total}", i + 1)
    };
    Ok((text, what))
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPLY: &str = "First:\n\n```sh\ncargo build\n```\n\nThen:\n\n```sh\ncargo test\n```\n";

    #[test]
    fn copies_the_reply_whole() {
        let (text, what) = pick_copy(Some(REPLY), CopyTarget::Reply).unwrap();
        assert_eq!((text.as_str(), what.as_str()), (REPLY, "reply"));
    }

    #[test]
    fn copies_the_last_block_or_the_one_asked() {
        let last = pick_copy(Some(REPLY), CopyTarget::Code(None)).unwrap();
        assert_eq!(last, ("cargo test".into(), "code block 2 of 2".into()));
        let first = pick_copy(Some(REPLY), CopyTarget::Code(Some(1))).unwrap();
        assert_eq!(first, ("cargo build".into(), "code block 1 of 2".into()));
        let only = pick_copy(Some("```\nls\n```"), CopyTarget::Code(None)).unwrap();
        assert_eq!(only, ("ls".into(), "code block".into()));
    }

    #[test]
    fn says_why_there_is_nothing_to_copy() {
        let err = |reply, target| pick_copy(reply, target).unwrap_err();
        assert_eq!(err(None, CopyTarget::Reply), "no reply to copy");
        assert_eq!(
            err(Some("just text"), CopyTarget::Code(None)),
            "no code block in the last reply"
        );
        assert_eq!(
            err(Some(REPLY), CopyTarget::Code(Some(3))),
            "the last reply has 2 code blocks"
        );
    }
}
