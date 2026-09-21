//! Keyboard handling: the input box, panels, history and command completion.

use super::*;

impl App {
    // ----- keyboard ---------------------------------------------------------

    pub(super) fn handle_key(&mut self, key: KeyEvent, tx: &Tx) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        self.notice = None;
        if self.panel.is_some() {
            self.handle_panel_key(key, tx);
            return;
        }
        // a mouse selection in the box lasts until the next key: esc is there
        // only to drop it, anything else drops it and goes on
        if self.input.has_selection() {
            self.input.clear_selection();
            if key.code == KeyCode::Esc {
                return;
            }
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let page = self.view_height.saturating_sub(1).max(1) as i32;
        if let Some((specs, sel)) = self.suggestions() {
            // with the command list in view, tab/enter complete; ↑↓ walk it
            // when there is something to choose from and we are not in history
            let spec = specs[sel];
            let navigable = self.suggest_navigable();
            let exact = commands::parse(&self.input.text()).is_ok();
            match key.code {
                KeyCode::Up if !ctrl && navigable => return self.suggest_move(-1),
                KeyCode::Down if !ctrl && navigable => return self.suggest_move(1),
                KeyCode::BackTab => return self.suggest_move(-1),
                KeyCode::Tab => {
                    self.suggest_accept();
                    return;
                }
                KeyCode::Enter if !alt && !shift && !exact => {
                    self.suggest_accept();
                    if spec.args.is_empty() {
                        self.submit(tx);
                    }
                    return;
                }
                _ => {}
            }
        }
        match key.code {
            KeyCode::Char('c') if ctrl => self.ctrl_c(),
            KeyCode::Char('d') if ctrl => {
                if self.input.is_empty() {
                    self.should_quit = true;
                }
            }
            KeyCode::Char('p') if ctrl => self.open_model_picker("", tx),
            KeyCode::Char('s') if ctrl => self.open_sessions_picker(),
            KeyCode::Char('f') if ctrl => self.open_files_panel(),
            KeyCode::Char('l') if ctrl => {}
            KeyCode::Char('w') if ctrl => self.input.delete_word_back(),
            KeyCode::Char('u') if ctrl => self.input.kill_to_start(),
            KeyCode::Char('k') if ctrl => self.input.kill_to_end(),
            KeyCode::Char('a') if ctrl => self.input.home(),
            KeyCode::Char('e') if ctrl => self.input.end(),
            KeyCode::Char('j') if ctrl => self.input.newline(),
            KeyCode::Esc => {
                if self.is_streaming() {
                    self.cancel_generation();
                } else if self.selection.is_some() {
                    self.selection = None;
                } else if !self.input.is_empty() {
                    self.input.clear();
                }
            }
            KeyCode::Enter if alt || shift => self.input.newline(),
            KeyCode::Enter => self.submit(tx),
            KeyCode::Tab => self.complete(),
            KeyCode::PageUp => self.scroll_by(-page),
            KeyCode::PageDown => self.scroll_by(page),
            KeyCode::Up if ctrl => self.scroll_by(-1),
            KeyCode::Down if ctrl => self.scroll_by(1),
            KeyCode::End if ctrl => self.follow = true,
            KeyCode::Home if ctrl => {
                self.follow = false;
                self.scroll_offset = 0;
            }
            KeyCode::Up => {
                if self.input.on_first_line() {
                    self.history_prev();
                } else {
                    self.input.up();
                }
            }
            KeyCode::Down => {
                if self.input.on_last_line() {
                    self.history_next();
                } else {
                    self.input.down();
                }
            }
            KeyCode::Left => self.input.left(),
            KeyCode::Right => self.input.right(),
            KeyCode::Home => self.input.home(),
            KeyCode::End => self.input.end(),
            KeyCode::Backspace => self.input.backspace(),
            KeyCode::Delete => self.input.delete(),
            KeyCode::Char(c) if !ctrl => self.input.insert_char(c),
            _ => {}
        }
    }

    pub(super) fn ctrl_c(&mut self) {
        if self.is_streaming() {
            self.cancel_generation();
            return;
        }
        if !self.input.is_empty() {
            self.input.clear();
            return;
        }
        if self.ctrl_c_at.is_some_and(|t| t.elapsed() < CTRL_C_WINDOW) {
            self.should_quit = true;
        } else {
            self.ctrl_c_at = Some(Instant::now());
            self.notify("ctrl+c again to quit");
        }
    }

    pub(super) fn handle_panel_key(&mut self, key: KeyEvent, tx: &Tx) {
        enum Outcome {
            Nothing,
            Close,
            /// Out of the tree and back to the files panel.
            Back,
            Choose,
            /// The tree, to attach something.
            Add,
            Delete,
            Rename,
        }
        if matches!(self.panel, Some(Panel::SessionAction { .. })) {
            self.handle_session_action_key(key);
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let outcome = match self.panel.as_mut() {
            Some(Panel::Help(h)) => match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') | KeyCode::Char('?') => {
                    Outcome::Close
                }
                KeyCode::Char('c') if ctrl => Outcome::Close,
                KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                    h.select(h.tab.shift(1));
                    Outcome::Nothing
                }
                KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
                    h.select(h.tab.shift(-1));
                    Outcome::Nothing
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    h.scroll_by(-1);
                    Outcome::Nothing
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    h.scroll_by(1);
                    Outcome::Nothing
                }
                KeyCode::PageUp => {
                    h.scroll_by(-h.page());
                    Outcome::Nothing
                }
                KeyCode::PageDown | KeyCode::Char(' ') => {
                    h.scroll_by(h.page());
                    Outcome::Nothing
                }
                KeyCode::Home => {
                    h.scroll_by(i32::MIN);
                    Outcome::Nothing
                }
                KeyCode::End => {
                    h.scroll_by(i32::MAX);
                    Outcome::Nothing
                }
                _ => Outcome::Nothing,
            },
            Some(panel) => {
                // the tree steps back into the files panel; every other list
                // closes the bottom
                let leave = match panel {
                    Panel::Browse { .. } => Outcome::Back,
                    _ => Outcome::Close,
                };
                let Some(p) = panel.picker_mut() else {
                    return;
                };
                // a row number is typed digit by digit; any other key drops
                // the one in progress
                let digit = matches!(key.code, KeyCode::Char(c) if c.is_ascii_digit())
                    && (alt || p.query.is_empty());
                let typing_number = p.pending.is_some();
                if !digit {
                    p.pending = None;
                }
                match key.code {
                    // a half-typed number is what esc drops first
                    KeyCode::Esc if typing_number => Outcome::Nothing,
                    KeyCode::Esc => leave,
                    KeyCode::Char('c') if ctrl => leave,
                    KeyCode::Enter => Outcome::Choose,
                    KeyCode::Char('a') if ctrl => Outcome::Add,
                    KeyCode::Up | KeyCode::BackTab => {
                        p.up();
                        Outcome::Nothing
                    }
                    KeyCode::Down | KeyCode::Tab => {
                        p.down();
                        Outcome::Nothing
                    }
                    KeyCode::Char('p') | KeyCode::Char('k') if ctrl => {
                        p.up();
                        Outcome::Nothing
                    }
                    KeyCode::Char('n') | KeyCode::Char('j') if ctrl => {
                        p.down();
                        Outcome::Nothing
                    }
                    KeyCode::Char('u') if ctrl => {
                        p.query.clear();
                        p.refilter();
                        Outcome::Nothing
                    }
                    KeyCode::Delete => Outcome::Delete,
                    KeyCode::Char('d') if ctrl => Outcome::Delete,
                    KeyCode::Char('r') if ctrl => Outcome::Rename,
                    KeyCode::Backspace => {
                        p.backspace();
                        Outcome::Nothing
                    }
                    // a digit takes you to the row with that number and
                    // picks it, unless a longer number still reaches the
                    // list: then it waits for the next digit, or for enter.
                    // It is a shortcut only while no filter is being typed
                    // (`llama3.1` is a model, not a row); with alt, always
                    KeyCode::Char(c) if digit => match p.number(c) {
                        true => Outcome::Choose,
                        false => Outcome::Nothing,
                    },
                    KeyCode::Char(c) if !ctrl && !alt => {
                        p.push(c);
                        Outcome::Nothing
                    }
                    _ => Outcome::Nothing,
                }
            }
            None => Outcome::Nothing,
        };
        match outcome {
            Outcome::Nothing => {}
            Outcome::Close => self.panel = None,
            Outcome::Choose => match self.panel.take() {
                Some(Panel::Models(p)) => {
                    if let Some(it) = p.current() {
                        if it.dim {
                            self.notify(format!("{}: {}", it.label, it.detail));
                            self.panel = Some(Panel::Models(p));
                        } else if let Some(m) = self.models.iter().find(|m| m.qualified() == it.id)
                        {
                            let (prov, id) = (m.provider.clone(), m.id.clone());
                            self.set_model(prov, id, tx);
                        }
                    }
                }
                Some(Panel::Sessions(p)) => {
                    if let Some(it) = p.current() {
                        let id = it.id.clone();
                        self.load_session(&id, tx);
                    }
                }
                // these two keep their panel: it is put back before acting on
                // it, since both rebuild the list they are standing on
                Some(panel @ Panel::Files(_)) => {
                    self.panel = Some(panel);
                    self.files_panel_choose();
                }
                Some(panel @ Panel::Browse { .. }) => {
                    self.panel = Some(panel);
                    self.browse_choose();
                }
                _ => {}
            },
            Outcome::Back => self.open_files_panel(),
            Outcome::Add => {
                if let Some(Panel::Files(_)) = &self.panel {
                    self.open_browser(PathBuf::new());
                }
            }
            Outcome::Delete => match &self.panel {
                // in the files panel `del` takes the highlighted file out,
                // the same as enter on it
                Some(Panel::Files(_)) => self.files_panel_detach(),
                _ => self.open_session_action(true),
            },
            Outcome::Rename => self.open_session_action(false),
        }
    }

    /// `ctrl+d`/`Del` or `ctrl+r` in the sessions list: the panel switches to
    /// deleting or renaming the highlighted session, keeping the list to
    /// return to it.
    pub(super) fn open_session_action(&mut self, delete: bool) {
        // deleting and renaming belong to the sessions list; from any other
        // panel the key does nothing, and above all does not close it
        if !matches!(self.panel, Some(Panel::Sessions(_))) {
            return;
        }
        let Some(Panel::Sessions(p)) = self.panel.take() else {
            return;
        };
        let Some(it) = p.current() else {
            self.panel = Some(Panel::Sessions(p));
            return;
        };
        let (id, title) = (it.id.clone(), it.label.clone());
        let action = if delete {
            let open = self.session.as_ref().is_some_and(|s| s.id == id);
            SessionAction::Delete {
                id,
                title,
                choice: Choice::Delete,
                open,
            }
        } else {
            SessionAction::Rename { id, input: title }
        };
        self.panel = Some(Panel::SessionAction {
            picker: Box::new(p),
            action,
        });
    }

    pub(super) fn handle_session_action_key(&mut self, key: KeyEvent) {
        enum Next {
            Stay,
            Back,
            Done(Result<String, String>),
        }
        let Some(Panel::SessionAction { picker, mut action }) = self.panel.take() else {
            return;
        };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let next = match &mut action {
            SessionAction::Delete {
                id, title, choice, ..
            } => match key.code {
                KeyCode::Esc | KeyCode::Char('n') => Next::Back,
                KeyCode::Char('c') if ctrl => Next::Back,
                // two options, one under the other: the same cursor as any
                // other list walks them
                KeyCode::Up
                | KeyCode::Down
                | KeyCode::Left
                | KeyCode::Right
                | KeyCode::Tab
                | KeyCode::BackTab
                | KeyCode::Char('h')
                | KeyCode::Char('j')
                | KeyCode::Char('k')
                | KeyCode::Char('l') => {
                    *choice = choice.toggle();
                    Next::Stay
                }
                KeyCode::Char('y') => Next::Done(self.delete_session(id, title)),
                KeyCode::Char('d') if ctrl => Next::Done(self.delete_session(id, title)),
                KeyCode::Enter if *choice == Choice::Delete => {
                    Next::Done(self.delete_session(id, title))
                }
                KeyCode::Enter => Next::Back,
                _ => Next::Stay,
            },
            SessionAction::Rename { id, input } => match key.code {
                KeyCode::Esc => Next::Back,
                KeyCode::Char('c') if ctrl => Next::Back,
                KeyCode::Enter => {
                    let new = input.trim().to_string();
                    if new.is_empty() {
                        Next::Stay
                    } else {
                        Next::Done(self.rename_session(id, &new))
                    }
                }
                KeyCode::Backspace => {
                    input.pop();
                    Next::Stay
                }
                KeyCode::Char('u') if ctrl => {
                    input.clear();
                    Next::Stay
                }
                KeyCode::Char(c) if !ctrl => {
                    input.push(c);
                    Next::Stay
                }
                _ => Next::Stay,
            },
        };
        match next {
            Next::Stay => self.panel = Some(Panel::SessionAction { picker, action }),
            Next::Back => self.panel = Some(Panel::Sessions(*picker)),
            Next::Done(result) => {
                // the picker reopens with the list up to date and the same filter
                let query = picker.query.clone();
                self.open_sessions_picker();
                if let Some(Panel::Sessions(p)) = self.panel.as_mut() {
                    p.query = query;
                    p.refilter();
                }
                match result {
                    Ok(n) | Err(n) => self.notify(n),
                }
            }
        }
    }

    pub(super) fn delete_session(&mut self, id: &str, title: &str) -> Result<String, String> {
        let Some(store) = &self.store else {
            return Err("sessions are disabled (save_sessions = false)".into());
        };
        store
            .delete(id)
            .map_err(|e| format!("could not delete: {e}"))?;
        self.forget_session(id);
        // deleting the one you are in only unhooks it: what is on screen
        // stays, and the next message opens a new session
        if self.session.as_ref().is_some_and(|s| s.id == id) {
            self.session = None;
            return Ok(format!(
                "session deleted: {title} · this conversation is no longer saved"
            ));
        }
        Ok(format!("session deleted: {title}"))
    }

    pub(super) fn rename_session(&mut self, id: &str, title: &str) -> Result<String, String> {
        let Some(store) = &self.store else {
            return Err("sessions are disabled (save_sessions = false)".into());
        };
        let mut meta = store
            .list()
            .map_err(|e| format!("could not list sessions: {e}"))?
            .into_iter()
            .find(|m| m.id == id)
            .ok_or_else(|| format!("session not found: {id}"))?;
        meta.title = title.to_string();
        store
            .update_meta(&meta)
            .map_err(|e| format!("could not rename: {e}"))?;
        if let Some(s) = self.session.as_mut().filter(|s| s.id == id) {
            s.title = title.to_string();
        }
        Ok(format!("session renamed: {title}"))
    }

    pub(super) fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let idx = match self.hist_idx {
            None => {
                self.draft = self.input.text();
                self.history.len() - 1
            }
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.hist_idx = Some(idx);
        self.input.set_text(&self.history[idx]);
    }

    pub(super) fn history_next(&mut self) {
        let Some(i) = self.hist_idx else { return };
        if i + 1 < self.history.len() {
            self.hist_idx = Some(i + 1);
            self.input.set_text(&self.history[i + 1]);
        } else {
            self.hist_idx = None;
            let d = std::mem::take(&mut self.draft);
            self.input.set_text(&d);
        }
    }

    pub(super) fn complete(&mut self) {
        let text = self.input.text();
        if self.complete_path(&text) {
            return;
        }
        let Some(rest) = text.strip_prefix('/') else {
            return;
        };
        if rest.contains(' ') {
            return;
        }
        let cands = commands::complete(rest);
        match cands.len() {
            0 => self.notify(format!("no command starts with /{rest}")),
            1 => self.input.set_text(&format!("/{} ", cands[0])),
            _ => self.notify(
                cands
                    .iter()
                    .map(|c| format!("/{c}"))
                    .collect::<Vec<_>>()
                    .join("  "),
            ),
        }
    }

    pub(super) fn submit(&mut self, tx: &Tx) {
        let text = self.input.text().trim().to_string();
        if text.is_empty() {
            return;
        }
        self.input.clear();
        self.hist_idx = None;
        self.draft.clear();
        if self.history.last() != Some(&text) {
            self.history.push(text.clone());
        }
        if text.starts_with('/') {
            match commands::parse(&text) {
                Ok(cmd) => self.run_command(cmd, tx),
                Err(e) => self.notify(e),
            }
        } else {
            self.send_message(text, tx);
        }
    }

    // ----- command suggestions ---------------------------------------------

    /// Command prefix being typed: the box holds a single line `/something`
    /// with no spaces and there is no panel.
    pub fn typing_command(&self) -> Option<String> {
        if self.panel.is_some() {
            return None;
        }
        let text = self.input.text();
        let rest = text.strip_prefix('/')?;
        if rest.contains(char::is_whitespace) {
            return None;
        }
        Some(rest.to_string())
    }

    /// Commands matching what was typed and which one is highlighted. `None`
    /// if no command is being typed or none starts that way.
    pub fn suggestions(&self) -> Option<(Vec<&'static commands::Spec>, usize)> {
        let prefix = self.typing_command()?;
        let specs = commands::matching(&prefix);
        if specs.is_empty() {
            return None;
        }
        let sel = if self.suggest_for == prefix {
            self.suggest_sel.min(specs.len() - 1)
        } else {
            0
        };
        Some((specs, sel))
    }

    /// `↑↓` walk the list only if there are two or more candidates and the
    /// prompt history is not being browsed (there the arrows still belong to it).
    pub fn suggest_navigable(&self) -> bool {
        self.hist_idx.is_none() && self.suggestions().is_some_and(|(s, _)| s.len() > 1)
    }

    pub(super) fn suggest_move(&mut self, delta: i32) {
        let Some((specs, sel)) = self.suggestions() else {
            return;
        };
        let n = specs.len() as i32;
        self.suggest_sel = (sel as i32 + delta).rem_euclid(n) as usize;
        self.suggest_for = self.typing_command().unwrap_or_default();
    }

    /// Puts the highlighted command in the box, ready for its arguments.
    pub(super) fn suggest_accept(&mut self) {
        if let Some((specs, sel)) = self.suggestions() {
            self.input.set_text(&format!("/{} ", specs[sel].name));
        }
    }

    /// Characters at the start of the box that form the command (`/name`),
    /// to highlight them: only if it is a known command or the start of one.
    pub fn command_span(&self) -> usize {
        let text = self.input.text();
        let first = text.lines().next().unwrap_or("");
        let Some(rest) = first.strip_prefix('/') else {
            return 0;
        };
        let name = rest.split(char::is_whitespace).next().unwrap_or("");
        if name.is_empty() {
            return 0;
        }
        let known =
            !commands::matching(name).is_empty() || commands::parse(&format!("/{name}")).is_ok();
        if known {
            1 + name.chars().count()
        } else {
            0
        }
    }
}
