//! The model acting on the project, on screen: the agents and the one the
//! conversation runs through, the loop's commands turned into conversation
//! items and panels, the keys of the approval panel, and the command that
//! runs off the main thread. The loop itself lives in `moon-agent`; this is
//! where its commands meet the interface.

use std::sync::atomic::{AtomicBool, Ordering};

use super::*;
use moon_agent::{
    editor_with, file_name, run_command, AgentFile, Command, Event, Exec, Harness, Limits, Pending,
    PendingEdit, Policy, Sandbox, Stop, Verdict,
};
use moon_core::write_text;

/// Replies with tool calls one message may take before the turn stops.
pub const ROUNDS_MAX: usize = 20;

/// The three things `ctrl+a`, `ctrl+r` and `ctrl+d` start on an agent.
#[derive(Clone, Copy)]
enum AgentActionKind {
    New,
    Rename,
    Delete,
}

/// What is on screen waiting for the user, an edit or a command, and the
/// choice the cursor is on.
pub struct Approval {
    pub pending: Pending,
    pub choice: EditChoice,
    /// First body line shown, and how many rows the last paint had.
    pub scroll: usize,
    pub rows: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditChoice {
    Yes,
    No,
}

impl Approval {
    pub fn new(pending: Pending) -> Self {
        Self {
            pending,
            choice: EditChoice::Yes,
            scroll: 0,
            rows: 0,
        }
    }

    pub fn edit(&self) -> Option<&PendingEdit> {
        match &self.pending {
            Pending::Edit(e) => Some(e),
            Pending::Run(_) => None,
        }
    }

    pub fn exec(&self) -> Option<&Exec> {
        match &self.pending {
            Pending::Run(e) => Some(e),
            Pending::Edit(_) => None,
        }
    }

    /// Body lines there are to scroll: the diff, or the few of a command.
    pub fn body_len(&self) -> usize {
        match &self.pending {
            Pending::Edit(e) => e.diff.lines.len(),
            Pending::Run(_) => crate::view::RUN_BODY,
        }
    }

    pub fn scroll_by(&mut self, delta: i32) {
        let max = self.body_len().saturating_sub(self.rows) as i64;
        self.scroll = (self.scroll as i64 + delta as i64).clamp(0, max) as usize;
    }

    /// Title of the panel: the kind of action, what it acts on goes under it.
    pub fn title(&self) -> &'static str {
        match &self.pending {
            Pending::Edit(e) if e.expect.is_none() => "Create file",
            Pending::Edit(_) => "Edit file",
            Pending::Run(_) => "Run command",
        }
    }

    /// Under the title: the file, or the command of the catalogue.
    pub fn subject(&self) -> &str {
        match &self.pending {
            Pending::Edit(e) => &e.path,
            Pending::Run(e) => e.id,
        }
    }

    /// Right of the title: the counts of an edit, the folder of a command
    /// when it is not the root.
    pub fn info(&self) -> String {
        match &self.pending {
            Pending::Edit(e) => e.counts(),
            Pending::Run(e) if e.rel_dir != "." => format!("in {}", e.rel_dir),
            Pending::Run(_) => String::new(),
        }
    }

    /// The question over the choices, split around what it is about, which
    /// goes in bold: the file name, or the command.
    pub fn question(&self) -> (&'static str, &str, &'static str) {
        match &self.pending {
            Pending::Edit(e) => {
                let name = e.path.rsplit('/').next().unwrap_or(&e.path);
                if e.expect.is_none() {
                    ("Do you want to create ", name, "?")
                } else {
                    ("Do you want to make this edit to ", name, "?")
                }
            }
            Pending::Run(e) => ("Do you want to run ", e.id, "?"),
        }
    }
}

/// A command running off the main thread: what to raise to kill it, and
/// since when.
pub(crate) struct Running {
    id: u64,
    cancel: Arc<AtomicBool>,
    pub started: Instant,
}

impl App {
    /// What the model may do right now — the chosen agent's permissions;
    /// nothing while the tools are off.
    pub(crate) fn policy(&self) -> Policy {
        match &self.harness {
            Some(h) => h.agent().policy.clone(),
            None => Policy::default(),
        }
    }

    /// What the agent may do to files: `(edit, create)`, for the session's
    /// meta line; both while the tools are off, as the older sessions had.
    pub(crate) fn tools_scope(&self) -> (bool, bool) {
        match self.tools_on {
            true => (self.policy().edits(), self.policy().creates()),
            false => (true, true),
        }
    }

    /// The agents folder read again: `default.toml` made if it is not
    /// there, every file brought up to the catalogue, then one definition
    /// per file. What happened comes back, one line each — a file written,
    /// one rewritten, one that could not be taken. Without a folder the
    /// definitions in memory stay as they are.
    pub(crate) fn reload_agents(&mut self) -> Vec<String> {
        let Some(dir) = self.agents_dir.clone() else {
            return Vec::new();
        };
        let mut said = Vec::new();
        match moon_agent::ensure_default(&dir, &self.cfg.tools) {
            Ok(Some(from)) => said.push(format!(
                "agents/{} written {from}",
                file_name(DEFAULT_AGENT)
            )),
            Ok(None) => {}
            Err(e) => said.push(format!(
                "agents/{} could not be written: {e}",
                file_name(DEFAULT_AGENT)
            )),
        }
        said.extend(moon_agent::sync_dir(&dir));
        let (mut defs, errors) = moon_agent::defs_from_dir(&dir);
        said.extend(errors.into_iter().map(|e| format!("agent skipped · {e}")));
        // the loop never runs on a ghost: default is there even when its
        // file is not, with nothing on
        if !defs.iter().any(|d| d.name == DEFAULT_AGENT) {
            defs.insert(0, AgentDef::default_agent());
        }
        self.agents = defs;
        said
    }

    /// The folder read again before something uses it — a message, the
    /// panel — so an edit made by hand counts: the chosen agent rebuilt if
    /// its file changed, and said; a chosen agent whose file is gone falls
    /// back to `default`.
    pub(crate) fn sync_agents(&mut self) {
        let before = self.agent_def();
        for line in self.reload_agents() {
            self.notify(line);
        }
        if !self.agents.iter().any(|d| d.name == self.agent) {
            let gone = std::mem::replace(&mut self.agent, DEFAULT_AGENT.into());
            self.refresh_agent_state();
            self.update_session_meta();
            self.notify(format!("agent {gone} is gone · back to default"));
            return;
        }
        if self.agent_def() != before {
            self.refresh_agent_state();
            self.update_session_meta();
            self.notify(format!(
                "agents/{} changed · reloaded",
                file_name(&self.agent)
            ));
        }
    }

    /// One definition by name, as last read.
    pub(crate) fn def_of(&self, name: &str) -> Option<AgentDef> {
        self.agents.iter().find(|d| d.name == name).cloned()
    }

    /// A definition changed made the file and the state: written where the
    /// agent lives, when there is a folder; put among the definitions; and
    /// the switch and the loop refreshed if it is the chosen one. Going dark
    /// is worth a word.
    pub(crate) fn save_def(&mut self, def: AgentDef) {
        if let Some(path) = self.agent_path(&def.name) {
            if let Err(e) = write_text(&path, &AgentFile::from_def(&def).to_toml()) {
                self.notify(format!("could not save {}: {e}", def.name));
                return;
            }
        }
        match self.agents.iter_mut().find(|d| d.name == def.name) {
            Some(d) => *d = def,
            None => self.agents.push(def),
        }
        let was_on = self.tools_on;
        self.refresh_agent_state();
        self.update_session_meta();
        if was_on && !self.tools_on {
            self.notify("tools off · the model only reads what you attach");
        }
    }

    /// The definition of the chosen agent; a name no definition carries any
    /// more gives `default`, so the loop never runs on a ghost.
    pub(crate) fn agent_def(&self) -> AgentDef {
        self.agents
            .iter()
            .find(|d| d.name == self.agent)
            .cloned()
            .unwrap_or_else(AgentDef::default_agent)
    }

    /// The harness given what the chosen agent calls for: its prompt, its
    /// permissions and its step limit.
    pub(super) fn rebuild_agent(&mut self) {
        let def = self.agent_def();
        let Some(h) = self.harness.as_mut() else {
            return;
        };
        h.set_agent(def.agent());
        let mut limits = h.limits();
        limits.rounds = def.steps().clamp(1, ROUNDS_MAX);
        h.set_limits(limits);
    }

    /// `/agent`: the picker, the folder read again first so a new file
    /// shows up without a restart.
    pub(crate) fn open_agent_picker(&mut self) {
        let said = self.reload_agents();
        if !said.is_empty() {
            self.notify(said.join(" · "));
        }
        let items = self
            .agents
            .iter()
            .map(|d| PickerItem {
                id: d.name.clone(),
                key: format!("{} {}", d.name, d.description),
                label: d.name.clone(),
                detail: d.description.clone(),
                active: d.name == self.agent,
                dim: false,
                group: None,
            })
            .collect();
        let n = self.agents.len();
        let mut p = Picker::new("Agent", items, "");
        p.hint = "a prompt and its permissions, whole: choosing the agent is choosing them".into();
        p.title_info = format!("{n} {}", models::plural(n, "agent"));
        p.keys = vec![
            ("↑↓", "move"),
            ("enter", "select"),
            ("ctrl+a", "new"),
            ("ctrl+t", "permissions"),
            ("ctrl+e", "prompt"),
            ("ctrl+r", "rename"),
            ("ctrl+d", "delete"),
            ("esc", "close"),
        ];
        p.empty_text = "no agent matches".into();
        self.panel = Some(Panel::Agents(p));
    }

    /// The choice made the state: the loop gets the agent, and the session
    /// remembers the name.
    pub(super) fn select_agent(&mut self, name: &str) {
        self.agent = name.to_string();
        self.refresh_agent_state();
        self.update_session_meta();
        let mut line = format!("set agent to {name}");
        if !self.tools_on {
            line.push_str(" · all off · ctrl+t opens its permissions");
        }
        self.notify(line);
    }

    /// The file behind an agent; none without a folder.
    pub(super) fn agent_path(&self, name: &str) -> Option<PathBuf> {
        self.agents_dir.as_ref().map(|d| d.join(file_name(name)))
    }

    /// `ctrl+a`, `ctrl+r` and `ctrl+d` in the agent picker: the panel
    /// switches to naming a new agent, renaming one or confirming a
    /// delete, keeping the list to return to it.
    pub(super) fn open_agent_new(&mut self) {
        self.open_agent_action(AgentActionKind::New);
    }

    pub(super) fn open_agent_rename(&mut self) {
        self.open_agent_action(AgentActionKind::Rename);
    }

    pub(super) fn open_agent_delete(&mut self) {
        self.open_agent_action(AgentActionKind::Delete);
    }

    fn open_agent_action(&mut self, kind: AgentActionKind) {
        if !matches!(self.panel, Some(Panel::Agents(_))) {
            return;
        }
        if self.agents_dir.is_none() {
            self.notify("no agents folder to write to");
            return;
        }
        let Some(Panel::Agents(picker)) = self.panel.take() else {
            return;
        };
        let action = match kind {
            AgentActionKind::New => AgentAction::New {
                input: String::new(),
            },
            _ => {
                let Some(name) = picker.current().map(|it| it.id.clone()) else {
                    self.panel = Some(Panel::Agents(picker));
                    return;
                };
                if name == DEFAULT_AGENT {
                    let verb = match kind {
                        AgentActionKind::Delete => "deleted",
                        _ => "renamed",
                    };
                    self.panel = Some(Panel::Agents(picker));
                    self.notify(format!(
                        "default is the agent every conversation starts with: it cannot be {verb}"
                    ));
                    return;
                }
                match kind {
                    AgentActionKind::Delete => AgentAction::Delete {
                        name,
                        choice: Choice::Delete,
                    },
                    _ => AgentAction::Rename {
                        input: name.clone(),
                        name,
                    },
                }
            }
        };
        self.panel = Some(Panel::AgentAction {
            picker: Box::new(picker),
            action,
        });
    }

    pub(super) fn handle_agent_action_key(&mut self, key: KeyEvent) {
        enum Next {
            Stay,
            Back,
            Create(String),
            RenameTo(String, String),
            Remove(String),
        }
        let Some(Panel::AgentAction { picker, mut action }) = self.panel.take() else {
            return;
        };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let next = match &mut action {
            AgentAction::New { input } => match key.code {
                KeyCode::Esc => Next::Back,
                KeyCode::Char('c') if ctrl => Next::Back,
                KeyCode::Enter => Next::Create(input.trim().to_string()),
                KeyCode::Backspace => {
                    input.pop();
                    Next::Stay
                }
                KeyCode::Char(ch) if !ctrl => {
                    input.push(ch);
                    Next::Stay
                }
                _ => Next::Stay,
            },
            AgentAction::Rename { name, input } => match key.code {
                KeyCode::Esc => Next::Back,
                KeyCode::Char('c') if ctrl => Next::Back,
                KeyCode::Enter => Next::RenameTo(name.clone(), input.trim().to_string()),
                KeyCode::Backspace => {
                    input.pop();
                    Next::Stay
                }
                KeyCode::Char(ch) if !ctrl => {
                    input.push(ch);
                    Next::Stay
                }
                _ => Next::Stay,
            },
            AgentAction::Delete { name, choice } => match key.code {
                KeyCode::Esc | KeyCode::Char('n') => Next::Back,
                KeyCode::Char('c') if ctrl => Next::Back,
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
                KeyCode::Enter => match choice {
                    Choice::Delete => Next::Remove(name.clone()),
                    Choice::Keep => Next::Back,
                },
                KeyCode::Char('y') | KeyCode::Char('d') => Next::Remove(name.clone()),
                _ => Next::Stay,
            },
        };
        match next {
            Next::Stay => self.panel = Some(Panel::AgentAction { picker, action }),
            Next::Back => self.panel = Some(Panel::Agents(*picker)),
            Next::Create(name) => match self.create_agent(&name) {
                Ok(()) => {
                    self.open_perms(&name, true);
                    self.notify(format!("agent {name} created"));
                }
                Err(e) => {
                    // the dialog stays: the word must be a notice, so the
                    // panel goes back before it is said
                    self.panel = Some(Panel::AgentAction { picker, action });
                    self.notify(e);
                }
            },
            Next::RenameTo(old, new) => {
                if old == new {
                    self.panel = Some(Panel::Agents(*picker));
                    return;
                }
                match self.rename_agent(&old, &new) {
                    Ok(()) => self.open_agent_picker(),
                    Err(e) => {
                        // the dialog stays here too
                        self.panel = Some(Panel::AgentAction { picker, action });
                        self.notify(e);
                    }
                }
            }
            Next::Remove(name) => {
                self.delete_agent(&name);
                self.open_agent_picker();
            }
        }
    }

    /// What a name may be: it is the file's, so lowercase and simple, and
    /// not one that is taken.
    fn check_agent_name(&self, name: &str) -> Result<(), String> {
        if name.is_empty() {
            return Err("type a name for the agent".into());
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        {
            return Err("a name is lowercase letters, digits, - and _".into());
        }
        if self.agents.iter().any(|d| d.name == name) {
            return Err(format!("`{name}` already exists"));
        }
        Ok(())
    }

    /// The name typed after `ctrl+a`: the template written — the folder
    /// made if it was not there — and the new agent left open to edit.
    fn create_agent(&mut self, name: &str) -> Result<(), String> {
        self.check_agent_name(name)?;
        let Some(path) = self.agent_path(name) else {
            return Err("no agents folder to write to".into());
        };
        write_text(&path, &AgentFile::template().to_toml()).map_err(|e| e.to_string())?;
        let _ = self.reload_agents();
        Ok(())
    }

    /// `ctrl+r`: the file is the name, so renaming the agent renames it; a
    /// selection that named it follows.
    fn rename_agent(&mut self, old: &str, new: &str) -> Result<(), String> {
        if old == DEFAULT_AGENT {
            return Err("default cannot be renamed".into());
        }
        self.check_agent_name(new)?;
        let (Some(from), Some(to)) = (self.agent_path(old), self.agent_path(new)) else {
            return Err("no agents folder to write to".into());
        };
        std::fs::rename(&from, &to).map_err(|e| format!("could not rename {old}: {e}"))?;
        let _ = self.reload_agents();
        if self.agent == old {
            self.agent = new.to_string();
            self.rebuild_agent();
            self.update_session_meta();
        }
        self.notify(format!("agent {old} renamed to {new}"));
        Ok(())
    }

    /// The file goes; a selection that named it falls back to `default`.
    fn delete_agent(&mut self, name: &str) {
        if name == DEFAULT_AGENT {
            return;
        }
        let Some(path) = self.agent_path(name) else {
            return;
        };
        if let Err(e) = std::fs::remove_file(&path) {
            self.notify(format!("could not delete {name}: {e}"));
            return;
        }
        let _ = self.reload_agents();
        if self.agent == name {
            self.agent = DEFAULT_AGENT.into();
            self.rebuild_agent();
            self.update_session_meta();
            self.notify(format!("agent {name} deleted · back to default"));
        } else {
            self.notify(format!("agent {name} deleted"));
        }
    }

    /// The commands the model may run, by id; none while it is off.
    pub(crate) fn tools_commands(&self) -> Vec<&'static str> {
        match &self.harness {
            Some(h) => h.agent().command_ids(),
            None => Vec::new(),
        }
    }

    /// The model can write something, so an approval may come.
    pub(crate) fn tools_write(&self) -> bool {
        let p = self.policy();
        self.tools_on && (p.edits() || p.creates())
    }

    /// The chosen agent's permissions set whole, file and state: nothing
    /// on is tools off; anything on turns them on.
    pub(crate) fn set_policy(&mut self, policy: Policy) {
        let mut def = self.agent_def();
        def.policy = policy;
        self.save_def(def);
    }

    /// The switch follows the chosen agent's permissions: empty is tools
    /// off, anything on turns them on.
    pub(crate) fn refresh_agent_state(&mut self) {
        if self.agent_def().policy.is_empty() {
            if self.tools_on {
                self.disable_tools();
            }
            return;
        }
        if !self.tools_on {
            if let Err(e) = self.enable_harness() {
                self.notify(e);
                return;
            }
        }
        self.rebuild_agent();
    }

    /// An edit or a command is on screen and the loop is paused on it.
    pub fn waiting_approval(&self) -> bool {
        matches!(self.panel, Some(Panel::Approval(_)))
    }

    /// The command being run, if one is.
    pub fn running_command(&self) -> Option<&Exec> {
        self.running.as_ref()?;
        self.harness.as_ref().and_then(Harness::running)
    }

    /// Between the user's message and the model's last reply: a generation
    /// running, or the loop between two of them.
    pub fn turn_active(&self) -> bool {
        self.is_streaming() || self.harness.as_ref().is_some_and(Harness::in_turn)
    }

    /// On, from the tests or a configuration without an agents folder: the
    /// chosen agent given what the old `[tools]` keys ask for when it has
    /// nothing on — the editor's whole policy when they ask for nothing —
    /// then the harness.
    pub(crate) fn enable_tools(&mut self) -> Result<(), String> {
        if self.tools_on {
            return Ok(());
        }
        if self.agent_def().policy.is_empty() {
            let mut policy = Policy::from_pairs(self.cfg.tools.startup_permissions());
            if policy.is_empty() {
                policy = editor_with(self.cfg.tools.edit, self.cfg.tools.create).policy;
            }
            self.set_policy(policy);
            return match self.tools_on {
                true => Ok(()),
                false => Err("the tools could not be turned on".into()),
            };
        }
        self.enable_harness()
    }

    /// The sandbox on the start-up directory and, behind it, the chosen
    /// agent as its file says.
    pub(super) fn enable_harness(&mut self) -> Result<(), String> {
        if self.tools_on {
            return Ok(());
        }
        let sandbox = Sandbox::new(
            &self.root,
            self.cfg.tools.max_file_bytes,
            &self.cfg.tools.deny,
        )
        .map_err(|e| format!("cannot enable edits: {} · {e}", self.cwd))?;
        let agent = self.agent_def().agent();
        self.harness = Some(Harness::new(agent, sandbox, Limits::default()));
        self.tools_on = true;
        self.rebuild_agent();
        if let Some(c) = &self.current {
            if self.current_is_ollama() && self.caps.is_some_and(|caps| !caps.tools) {
                self.notify(format!(
                    "{} does not report tool support: the requests may fail · /model to pick another",
                    c.model
                ));
            }
        }
        if !self.root.join(".git").exists() {
            let warn = "✎ edits on · not a git repository: moon cannot undo what you apply";
            // from the panel, under the command, even while it is still
            // open; from the file at startup, on its own
            if let Some((_, out)) = self.echo.as_mut() {
                out.push(warn.into());
            } else {
                self.push_item(Item::Info(warn.into()));
                self.follow = true;
            }
        }
        self.update_session_meta();
        Ok(())
    }

    /// Off, from the panel: whatever is in flight is cancelled first.
    pub(crate) fn disable_tools(&mut self) {
        if self.turn_active() {
            self.cancel_generation();
        }
        self.harness = None;
        self.tools_on = false;
        self.update_session_meta();
    }

    /// The reply is in: its calls, if any, go to the loop. A reply with none
    /// while a turn is open closes the turn. A reply that *is* a call written
    /// as text, as the small models do, becomes one: the JSON leaves the
    /// conversation and the call runs like any other.
    pub(super) fn after_reply(&mut self, tx: &Tx) {
        if !self.tools_on {
            return;
        }
        let Some(h) = self.harness.as_ref() else {
            return;
        };
        let mut rewritten = false;
        let last = self.items.iter_mut().rev().find_map(|i| match i {
            Item::Message(m) if m.role == Role::Assistant => Some(m),
            _ => None,
        });
        let calls = match last {
            Some(m) if !m.tool_calls.is_empty() => m.tool_calls.clone(),
            Some(m) => {
                let found = h.calls_from_text(&m.content);
                if !found.is_empty() {
                    m.tool_calls = found.clone();
                    m.content.clear();
                    rewritten = true;
                }
                found
            }
            None => Vec::new(),
        };
        if rewritten {
            self.rewrite_session();
        }
        let Some(h) = self.harness.as_mut() else {
            return;
        };
        if calls.is_empty() && !h.in_turn() {
            return;
        }
        let cmds = h.feed(Event::ModelDone(calls));
        self.run_commands(cmds, Some(tx));
    }

    /// `Esc`, an error, the panel turning it off: what is on screen is
    /// dropped, a command that runs is killed, and the calls left are closed
    /// as not run, so the history stays well formed.
    pub(super) fn abort_turn(&mut self) {
        if self.waiting_approval() {
            self.panel = None;
        }
        if let Some(r) = self.running.take() {
            r.cancel.store(true, Ordering::Relaxed);
        }
        let Some(h) = self.harness.as_mut() else {
            return;
        };
        if !h.in_turn() {
            return;
        }
        let cmds = h.feed(Event::Cancel);
        self.run_commands(cmds, None);
    }

    /// What the loop asked for, done. Without `tx` nothing can be sent or
    /// run, so a `Continue` or a `Run` ends the turn instead.
    fn run_commands(&mut self, cmds: Vec<Command>, tx: Option<&Tx>) {
        for cmd in cmds {
            match cmd {
                Command::Continue(results) => {
                    for m in results {
                        self.persist(&m);
                        self.push_item(Item::Message(m));
                    }
                    self.follow = true;
                    match tx {
                        Some(tx) => self.start_generation(tx),
                        None => self.abort_turn(),
                    }
                }
                Command::Ask(pending) => {
                    self.panel = Some(Panel::Approval(Box::new(Approval::new(pending))));
                    self.follow = true;
                }
                Command::Run(exec) => match tx {
                    Some(tx) => self.spawn_run(exec, tx),
                    None => self.abort_turn(),
                },
                Command::Step(step) => {
                    self.push_item(Item::Step(step));
                    self.follow = true;
                }
                Command::Finished => {}
                Command::Stopped { reason, results } => {
                    for m in results {
                        self.persist(&m);
                        self.push_item(Item::Message(m));
                    }
                    if self.waiting_approval() {
                        self.panel = None;
                    }
                    match reason {
                        Stop::Cancelled => self.notify("turn cancelled"),
                        other => {
                            self.push_item(Item::Info(format!("✗ turn stopped: {other}")));
                            self.follow = true;
                        }
                    }
                }
            }
        }
    }

    /// Runs the command on a thread of its own; what it printed comes back
    /// as `Action::Ran`, with the number that tells a late one from the one
    /// that is waited for.
    fn spawn_run(&mut self, exec: Exec, tx: &Tx) {
        self.run_seq += 1;
        let id = self.run_seq;
        let cancel = Arc::new(AtomicBool::new(false));
        self.running = Some(Running {
            id,
            cancel: cancel.clone(),
            started: Instant::now(),
        });
        self.follow = true;
        let tx = tx.clone();
        std::thread::spawn(move || {
            let out = run_command::execute(&exec, &cancel);
            let _ = tx.send(Action::Ran(id, out));
        });
    }

    /// The command is done: its output goes to the loop, unless the turn
    /// that asked for it was cancelled meanwhile.
    pub(super) fn on_ran(&mut self, id: u64, output: moon_agent::Output, tx: &Tx) {
        if !self.running.as_ref().is_some_and(|r| r.id == id) {
            return;
        }
        self.running = None;
        let Some(h) = self.harness.as_mut() else {
            return;
        };
        let cmds = h.feed(Event::Ran(output));
        self.run_commands(cmds, Some(tx));
    }

    pub(super) fn approval_verdict(&mut self, verdict: Verdict, tx: &Tx) {
        self.panel = None;
        let Some(h) = self.harness.as_mut() else {
            return;
        };
        let cmds = h.feed(Event::Verdict(verdict));
        self.run_commands(cmds, Some(tx));
    }

    pub(super) fn handle_approval_key(&mut self, key: KeyEvent, tx: &Tx) {
        enum Next {
            Stay,
            Verdict(Verdict),
            Cancel,
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(Panel::Approval(a)) = self.panel.as_mut() else {
            return;
        };
        let page = a.rows.max(1) as i32;
        let next = match key.code {
            KeyCode::Esc => Next::Cancel,
            KeyCode::Char('c') if ctrl => Next::Cancel,
            KeyCode::Enter => Next::Verdict(match a.choice {
                EditChoice::Yes => Verdict::Apply,
                EditChoice::No => Verdict::Skip,
            }),
            // the numbers of the list, or its first letters, answer at once
            KeyCode::Char('1') | KeyCode::Char('y') => Next::Verdict(Verdict::Apply),
            KeyCode::Char('2') | KeyCode::Char('n') => Next::Verdict(Verdict::Skip),
            // one option over the other
            KeyCode::Up | KeyCode::Char('k') => {
                a.choice = EditChoice::Yes;
                Next::Stay
            }
            KeyCode::Down | KeyCode::Char('j') => {
                a.choice = EditChoice::No;
                Next::Stay
            }
            KeyCode::PageUp => {
                a.scroll_by(-page);
                Next::Stay
            }
            KeyCode::PageDown | KeyCode::Char(' ') => {
                a.scroll_by(page);
                Next::Stay
            }
            KeyCode::Home => {
                a.scroll_by(i32::MIN);
                Next::Stay
            }
            KeyCode::End => {
                a.scroll_by(i32::MAX);
                Next::Stay
            }
            _ => Next::Stay,
        };
        match next {
            Next::Stay => {}
            Next::Verdict(v) => self.approval_verdict(v, tx),
            Next::Cancel => self.cancel_generation(),
        }
    }
}
