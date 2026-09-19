//! Files in the context: the project file, live attachments and `@` mentions.

use super::*;

impl App {
    // ----- files in the context ---------------------------------------------

    pub(super) fn load_context_file(&mut self) {
        let name = self.cfg.general.context_file.clone();
        self.context_file = match context::load_context_file(&self.root, &name) {
            Ok(Some(content)) => Some((name, content)),
            Ok(None) => None,
            Err(e) => {
                self.push_item(Item::Info(format!("{name}: could not read: {e}")));
                None
            }
        };
    }

    pub(super) fn read_spec(&self, spec: &Spec) -> Result<Attachment, context::AttachError> {
        context::read_attachment(&self.root, spec, self.cfg.general.max_attachment_bytes)
    }

    /// Re-reads the live attachments from disk.
    pub(super) fn read_live(&self) -> Result<Vec<Attachment>, String> {
        self.live
            .iter()
            .map(|s| {
                self.read_spec(s)
                    .map_err(|e| format!("{}: {e} · ctrl+f to detach it", s.label()))
            })
            .collect()
    }

    pub(super) fn system_prompt_for(&self, live: &[Attachment]) -> Option<String> {
        context::build_system_prompt(
            self.system_prompt.as_deref(),
            self.context_file
                .as_ref()
                .map(|(n, c)| (n.as_str(), c.as_str())),
            live,
        )
    }

    /// Estimated tokens of what would travel in the next request.
    pub(super) fn estimated_request_tokens(
        &self,
        text: &str,
        new: &[Attachment],
        live: &[Attachment],
    ) -> u32 {
        let system = self
            .system_prompt_for(live)
            .map(|s| context::estimate_tokens(&s))
            .unwrap_or(0);
        let history: u32 = self
            .messages()
            .map(|m| {
                context::estimate_tokens(&m.content)
                    + m.attachments.iter().map(|a| a.tokens).sum::<u32>()
            })
            .sum();
        let new: u32 = new.iter().map(|a| a.tokens).sum();
        system + history + new + context::estimate_tokens(text)
    }

    pub(super) fn check_budget(
        &self,
        text: &str,
        new: &[Attachment],
        live: &[Attachment],
    ) -> Result<(), String> {
        let Some(ctx) = self.ctx_window().filter(|c| *c > 0) else {
            return Ok(());
        };
        let total = self.estimated_request_tokens(text, new, live);
        let limit = (ctx as f32 * context::BUDGET_RATIO) as u32;
        if total > limit {
            return Err(format!(
                "context budget: ~{} of {} tok ({}%) · detach with ctrl+f, use a range (@path:1-80) or /new",
                fmt_k(total),
                fmt_k(ctx),
                (total as u64 * 100 / ctx as u64)
            ));
        }
        Ok(())
    }

    // ----- the files panel --------------------------------------------------

    /// Opens the panel: what is attached, what the project adds on its own,
    /// and the two buttons.
    pub(crate) fn open_files_panel(&mut self) {
        self.panel = Some(Panel::Files(self.files_picker("")));
    }

    /// Rebuilds the list in place, keeping the filter: what is attached has
    /// just changed.
    pub(super) fn refresh_files_panel(&mut self) {
        let fresh = self.files_picker("");
        if let Some(Panel::Files(p)) = self.panel.as_mut() {
            p.update_from(fresh);
        }
    }

    /// One row per attached file with what it costs, the project context file
    /// below (it is not attached by hand: it is dimmed), and the buttons.
    pub(crate) fn files_picker(&self, query: &str) -> Picker {
        let mut items = Vec::new();
        let mut tokens = 0;
        for spec in &self.live {
            let label = spec.label();
            let (detail, dim) = match self.read_spec(spec) {
                Ok(a) => {
                    tokens += a.tokens;
                    let mut d = format!("{} tok", fmt_k(a.tokens));
                    if a.truncated {
                        d.push_str(" · truncated");
                    }
                    (d, false)
                }
                Err(e) => (format!("{e}"), true),
            };
            items.push(PickerItem {
                id: label.clone(),
                key: label.clone(),
                label,
                detail,
                active: false,
                dim,
                group: Some(ATTACHED_GROUP.to_string()),
            });
        }
        if let Some((name, content)) = &self.context_file {
            items.push(PickerItem {
                id: CONTEXT_ROW.to_string(),
                key: name.clone(),
                label: name.clone(),
                detail: format!(
                    "{} tok · read from the project",
                    fmt_k(context::estimate_tokens(content))
                ),
                active: false,
                dim: true,
                group: Some(PROJECT_GROUP.to_string()),
            });
        }
        let button = |id: &str, label: &str, detail: &str| PickerItem {
            id: id.to_string(),
            key: label.to_string(),
            label: label.to_string(),
            detail: detail.to_string(),
            active: false,
            dim: false,
            group: Some(ACTIONS_GROUP.to_string()),
        };
        items.push(button(ADD_ROW, "Add files…", "browse the project (ctrl+a)"));
        if !self.live.is_empty() {
            items.push(button(CLEAR_ROW, "Detach all", "leaves nothing attached"));
        }
        let n = self.live.len();
        let mut p = Picker::new("Files", items, query);
        p.hint = "attached to every request and re-read on each send · type to filter".into();
        p.groups = vec![PickerGroup {
            title: ATTACHED_GROUP.into(),
            info: if n == 0 {
                "nothing attached".into()
            } else {
                format!("{n} {} · {} tok", models::plural(n, "file"), fmt_k(tokens))
            },
            mark: None,
        }];
        if self.context_file.is_some() {
            p.groups.push(PickerGroup {
                title: PROJECT_GROUP.into(),
                info: "always in the prompt".into(),
                mark: None,
            });
        }
        p.groups.push(PickerGroup {
            title: ACTIONS_GROUP.into(),
            info: String::new(),
            mark: None,
        });
        p.title_info = if n == 0 {
            "nothing attached".to_string()
        } else {
            format!("{n} {} · {} tok", models::plural(n, "file"), fmt_k(tokens))
        };
        p.keys = vec![
            ("↑↓", "move"),
            ("enter", "detach"),
            ("ctrl+a", "add"),
            ("esc", "close"),
        ];
        p.empty_text = "nothing matches".into();
        p
    }

    /// What `enter` does on the highlighted row of the files panel.
    pub(super) fn files_panel_choose(&mut self) {
        let Some(Panel::Files(p)) = self.panel.as_ref() else {
            return;
        };
        let Some(it) = p.current() else { return };
        match it.id.as_str() {
            ADD_ROW => self.open_browser(PathBuf::new()),
            CLEAR_ROW => {
                let n = self.live.len();
                self.live.clear();
                self.update_session_meta();
                self.notify(format!("detached {n}"));
                self.refresh_files_panel();
            }
            CONTEXT_ROW => {
                let name = it.label.clone();
                self.notify(format!(
                    "{name} is the project context file: it goes in on its own"
                ));
            }
            _ => {
                let label = it.id.clone();
                self.detach(&label);
            }
        }
    }

    /// `del` on a row of the files panel: only a file comes out; on the
    /// buttons or the project file it does nothing.
    pub(super) fn files_panel_detach(&mut self) {
        let Some(Panel::Files(p)) = self.panel.as_ref() else {
            return;
        };
        let Some(it) = p.current() else { return };
        if matches!(it.id.as_str(), ADD_ROW | CLEAR_ROW | CONTEXT_ROW) {
            return;
        }
        let label = it.id.clone();
        self.detach(&label);
    }

    /// Takes a file out of the context. The panel, if open, refreshes.
    pub(super) fn detach(&mut self, label: &str) {
        let before = self.live.len();
        self.live.retain(|s| s.label() != label && s.path != label);
        if before == self.live.len() {
            self.notify(format!("nothing attached as `{label}`"));
            return;
        }
        self.update_session_meta();
        self.notify(format!("detached {label}"));
        self.refresh_files_panel();
    }

    /// Attaches `raw` (`path` or `path:a-b`) if it is not in already, and says
    /// what it cost. Returns whether anything changed.
    pub(super) fn attach(&mut self, raw: &str) -> bool {
        let spec = match Spec::parse(raw) {
            Ok(s) => s,
            Err(e) => {
                self.notify(format!("{raw}: {e}"));
                return false;
            }
        };
        if self.live.iter().any(|l| l.label() == spec.label()) {
            self.notify(format!("already attached: {}", spec.label()));
            return false;
        }
        match self.read_spec(&spec) {
            Ok(a) => {
                self.notify(format!("attached: {} · {} tok", a.label(), fmt_k(a.tokens)));
                self.live.push(spec);
                self.update_session_meta();
                true
            }
            Err(e) => {
                self.notify(format!("{}: {e}", spec.label()));
                false
            }
        }
    }

    // ----- browsing the project ---------------------------------------------

    /// Opens the tree at `dir` (relative to the root) to pick something. The
    /// files panel is what it goes back to.
    pub(super) fn open_browser(&mut self, dir: PathBuf) {
        let picker = match self.browse_picker(&dir) {
            Some(p) => p,
            None => {
                self.notify(format!("could not read {}", show_dir(&dir)));
                return;
            }
        };
        self.panel = Some(Panel::Browse {
            picker: Box::new(picker),
            dir,
        });
    }

    /// The contents of `dir`: folders first, then files with their size, and
    /// a `✓` on whatever is already attached. `None` if it cannot be read.
    fn browse_picker(&self, dir: &Path) -> Option<Picker> {
        let base = self.root.join(dir);
        let mut folders = Vec::new();
        let mut files = Vec::new();
        for e in std::fs::read_dir(&base).ok()? {
            let Ok(e) = e else { continue };
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || matches!(name.as_str(), "target" | "node_modules") {
                continue;
            }
            let rel = dir.join(&name);
            let path = rel.to_string_lossy().to_string();
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let item = PickerItem {
                id: path.clone(),
                key: name.clone(),
                label: if is_dir { format!("{name}/") } else { name },
                detail: match e.metadata() {
                    Ok(m) if !is_dir => fmt_bytes(m.len()),
                    _ => String::new(),
                },
                active: !is_dir && self.live.iter().any(|s| s.path == path),
                dim: false,
                group: Some(if is_dir { FOLDERS_GROUP } else { FILES_GROUP }.to_string()),
            };
            if is_dir {
                folders.push(item);
            } else {
                files.push(item);
            }
        }
        folders.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));
        files.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));
        let mut items = Vec::new();
        // the way out, at the top, as one more folder
        if dir.parent().is_some() {
            items.push(PickerItem {
                id: dir
                    .parent()
                    .unwrap_or(Path::new(""))
                    .to_string_lossy()
                    .to_string(),
                key: "..".into(),
                label: "..".into(),
                detail: "up one folder".into(),
                active: false,
                dim: false,
                group: Some(FOLDERS_GROUP.to_string()),
            });
        }
        let (n_dirs, n_files) = (folders.len(), files.len());
        items.extend(folders);
        items.extend(files);
        let mut p = Picker::new("Add files", items, "");
        p.hint = "enter opens a folder or attaches a file · type to filter".into();
        p.groups = vec![
            PickerGroup {
                title: FOLDERS_GROUP.into(),
                info: format!("{n_dirs} {}", models::plural(n_dirs, "folder")),
                mark: None,
            },
            PickerGroup {
                title: FILES_GROUP.into(),
                info: format!("{n_files} {}", models::plural(n_files, "file")),
                mark: None,
            },
        ];
        p.title_info = show_dir(dir);
        p.keys = vec![("↑↓", "move"), ("enter", "open or attach"), ("esc", "back")];
        p.empty_text = "nothing here".into();
        Some(p)
    }

    /// What `enter` does in the tree: go into a folder, or attach the file
    /// (again to detach it) without leaving the list.
    pub(super) fn browse_choose(&mut self) {
        let Some(Panel::Browse { picker, dir }) = self.panel.as_ref() else {
            return;
        };
        let Some(it) = picker.current() else { return };
        let (id, is_dir, attached) = (
            it.id.clone(),
            it.group.as_deref() == Some(FOLDERS_GROUP),
            it.active,
        );
        if is_dir {
            self.open_browser(PathBuf::from(id));
            return;
        }
        let dir = dir.clone();
        if attached {
            self.detach(&id);
        } else {
            self.attach(&id);
        }
        // the ✓ and the sizes are rebuilt, the filter and the cursor stay
        if let Some(fresh) = self.browse_picker(&dir) {
            if let Some(Panel::Browse { picker, .. }) = self.panel.as_mut() {
                picker.update_from(fresh);
            }
        }
    }

    pub(super) fn show_context(&mut self) {
        let mut lines = Vec::new();
        match &self.context_file {
            Some((name, content)) => lines.push(format!(
                "context file: {name} · {} tok",
                fmt_k(context::estimate_tokens(content))
            )),
            None if self.cfg.general.context_file.trim().is_empty() => {
                lines.push("context file: (disabled)".into())
            }
            None => lines.push(format!(
                "context file: {} (not found in {})",
                self.cfg.general.context_file, self.cwd
            )),
        }
        match &self.system_prompt {
            Some(sp) => lines.push(format!(
                "system prompt: {} tok",
                fmt_k(context::estimate_tokens(sp))
            )),
            None => lines.push("system prompt: (none)".into()),
        }
        let live = match self.read_live() {
            Ok(l) => l,
            Err(e) => {
                self.push_item(Item::Error(e));
                return;
            }
        };
        if live.is_empty() {
            lines.push("attached (live): none · ctrl+f to attach".into());
        } else {
            lines.push("attached (live, re-read on every send):".into());
            for a in &live {
                lines.push(format!(
                    "  {} · {} tok{}",
                    a.label(),
                    fmt_k(a.tokens),
                    if a.truncated { " · truncated" } else { "" }
                ));
            }
        }
        let snaps: Vec<&Attachment> = self.messages().flat_map(|m| m.attachments.iter()).collect();
        if !snaps.is_empty() {
            let tok: u32 = snaps.iter().map(|a| a.tokens).sum();
            lines.push(format!(
                "snapshots in history (@): {} files · {} tok",
                snaps.len(),
                fmt_k(tok)
            ));
        }
        let total = self.estimated_request_tokens("", &[], &live);
        match self.ctx_window() {
            Some(ctx) if ctx > 0 => lines.push(format!(
                "next request: ~{} of {} tok ({}%)",
                fmt_k(total),
                fmt_k(ctx),
                total as u64 * 100 / ctx as u64
            )),
            _ => lines.push(format!("next request: ~{} tok", fmt_k(total))),
        }
        if let Some(cur) = &self.current {
            use crate::sysmon::fmt_gib;
            match &self.loaded {
                LoadedState::Loaded(m) => {
                    let mut line =
                        format!("model: {} · loaded {} GB", cur.model, fmt_gib(m.size_bytes));
                    // the weights come from `/api/tags`; the rest is context cache
                    let weights = self
                        .models
                        .iter()
                        .find(|x| x.provider == cur.provider && x.id == cur.model)
                        .and_then(|x| x.size_bytes)
                        .filter(|w| *w < m.size_bytes);
                    if let Some(w) = weights {
                        line.push_str(&format!(
                            " ({} GB weights + {} GB context",
                            fmt_gib(w),
                            fmt_gib(m.size_bytes - w)
                        ));
                        match m.context_length {
                            Some(c) => line.push_str(&format!(" at {})", fmt_k(c))),
                            None => line.push(')'),
                        }
                    }
                    line.push_str(&format!(" · {}% gpu", m.gpu_percent()));
                    if let Some(d) = m.expires_in() {
                        line.push_str(&format!(" · unloads in {}", fmt_dur(d)));
                    }
                    lines.push(line);
                }
                LoadedState::NotLoaded => lines.push(format!(
                    "model: {} · not loaded (the next request loads it)",
                    cur.model
                )),
                LoadedState::Unknown | LoadedState::Unsupported => {}
            }
        }
        if let Some(s) = self.sys.current() {
            use crate::sysmon::fmt_gib;
            let mut m = format!(
                "machine: cpu {:.0}% · ram {} / {} GB ({:.0}%)",
                s.cpu,
                fmt_gib(s.ram_used),
                fmt_gib(s.ram_total),
                s.ram
            );
            if s.swap_used > 0 {
                m.push_str(&format!(" · swap {} GB", fmt_gib(s.swap_used)));
            }
            m.push_str(&format!(
                " · 3m peak: cpu {:.0}% · ram {:.0}%",
                self.sys.peak_cpu(),
                self.sys.peak_ram()
            ));
            lines.push(m);
        }
        self.push_item(Item::Info(lines.join("\n")));
        self.follow = true;
    }

    /// Name of the loaded context file, if any.
    pub fn context_file_name(&self) -> Option<&str> {
        self.context_file.as_ref().map(|(n, _)| n.as_str())
    }

    /// Completes a path after `@` in the last token of the input.
    pub(super) fn complete_path(&mut self, text: &str) -> bool {
        let Some(tok) = text.split_whitespace().last() else {
            return false;
        };
        let Some(raw) = tok.strip_prefix('@') else {
            return false;
        };
        if !text.ends_with(tok) {
            return false;
        }
        let (bang, raw) = match raw.strip_prefix('!') {
            Some(r) => ("!", r),
            None => ("", raw),
        };
        let (dir, name) = match raw.rsplit_once('/') {
            Some((d, n)) => (format!("{d}/"), n.to_string()),
            None => (String::new(), raw.to_string()),
        };
        let base = if dir.is_empty() {
            self.root.clone()
        } else {
            self.root.join(&dir)
        };
        let Ok(rd) = std::fs::read_dir(&base) else {
            self.notify(format!("no such directory: {dir}"));
            return true;
        };
        let mut cands: Vec<String> = rd
            .flatten()
            .filter_map(|e| {
                let n = e.file_name().to_string_lossy().to_string();
                if !n.starts_with(&name) || (name.is_empty() && n.starts_with('.')) {
                    return None;
                }
                if matches!(n.as_str(), "target" | ".git" | "node_modules") {
                    return None;
                }
                let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                Some(if is_dir { format!("{n}/") } else { n })
            })
            .collect();
        cands.sort();
        let head = &text[..text.len() - tok.len()];
        match cands.len() {
            0 => self.notify(format!("no file starts with `{dir}{name}`")),
            1 => self
                .input
                .set_text(&format!("{head}@{bang}{dir}{}", cands[0])),
            _ => {
                let common = common_prefix(&cands);
                if common.len() > name.len() {
                    self.input.set_text(&format!("{head}@{bang}{dir}{common}"));
                } else {
                    self.notify(cands.iter().take(8).cloned().collect::<Vec<_>>().join("  "));
                }
            }
        }
        true
    }
}

/// How a folder of the project is named on screen: the root has no path.
fn show_dir(dir: &Path) -> String {
    match dir.to_string_lossy().to_string() {
        d if d.is_empty() => "project root".to_string(),
        d => d,
    }
}

/// Size of a file, for the tree: `812 B`, `14.2 KB`, `1.3 MB`.
fn fmt_bytes(n: u64) -> String {
    match n {
        n if n < 1024 => format!("{n} B"),
        n if n < 1024 * 1024 => format!("{:.1} KB", n as f64 / 1024.0),
        n => format!("{:.1} MB", n as f64 / (1024.0 * 1024.0)),
    }
}
