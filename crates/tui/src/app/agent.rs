//! The model editing files, on screen: the switch, the loop's commands
//! turned into conversation items and panels, and the keys of the approval
//! panel. The loop itself lives in `moon-agent`; this is where its commands
//! meet the interface.

use super::*;
use moon_agent::{
    editor_with, Command, Event, Harness, Limits, PendingEdit, Sandbox, Stop, Tool, Verdict,
};

/// The `/tools` panel: a handful of choices walked with the cursor. There is
/// no cancel: `Esc` applies whatever is set and closes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolsDialog {
    /// The model may read and list files in this conversation.
    pub on: bool,
    /// …and edit the ones that exist (`edit_file`).
    pub edit: bool,
    /// …and create new ones (`write_file`).
    pub create: bool,
    /// Replies with tool calls one message may take before the turn stops.
    pub rounds: usize,
    /// Row the cursor is on, `ToolsDialog::ROWS` of them.
    pub row: usize,
}

impl ToolsDialog {
    pub const ROWS: usize = 4;
    const ROUNDS_MAX: usize = 20;
    /// Cursor rows, top to bottom.
    pub const ON: usize = 0;
    pub const EDIT: usize = 1;
    pub const CREATE: usize = 2;
    pub const ROUNDS: usize = 3;

    pub fn up(&mut self) {
        self.row = (self.row + Self::ROWS - 1) % Self::ROWS;
    }

    pub fn down(&mut self) {
        self.row = (self.row + 1) % Self::ROWS;
    }

    /// `space`: the checkbox under the cursor. Editing and creating need
    /// reading, so the boxes stay consistent: ticking either ticks `Read
    /// files`, unticking `Read files` unticks both. All off is tools off.
    pub fn toggle(&mut self) {
        match self.row {
            Self::ON => {
                self.on = !self.on;
                if !self.on {
                    self.edit = false;
                    self.create = false;
                }
            }
            Self::EDIT => {
                self.edit = !self.edit;
                self.on |= self.edit;
            }
            Self::CREATE => {
                self.create = !self.create;
                self.on |= self.create;
            }
            _ => {}
        }
    }

    /// `←`/`→`: the selector under the cursor, or the checkbox.
    pub fn change(&mut self, delta: i32) {
        match self.row {
            Self::ROUNDS => {
                self.rounds =
                    (self.rounds as i64 + delta as i64).clamp(1, Self::ROUNDS_MAX as i64) as usize;
            }
            _ => self.toggle(),
        }
    }
}

/// The edit on screen and the choice the cursor is on.
pub struct Approval {
    pub edit: PendingEdit,
    pub choice: EditChoice,
    /// First diff line shown, and how many rows the last paint had.
    pub scroll: usize,
    pub rows: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditChoice {
    Apply,
    Skip,
}

impl EditChoice {
    fn toggle(self) -> Self {
        match self {
            EditChoice::Apply => EditChoice::Skip,
            EditChoice::Skip => EditChoice::Apply,
        }
    }
}

impl Approval {
    pub fn new(edit: PendingEdit) -> Self {
        Self {
            edit,
            choice: EditChoice::Apply,
            scroll: 0,
            rows: 0,
        }
    }

    pub fn scroll_by(&mut self, delta: i32) {
        let max = self.edit.diff.lines.len().saturating_sub(self.rows) as i64;
        self.scroll = (self.scroll as i64 + delta as i64).clamp(0, max) as usize;
    }

    /// Title of the panel: what would happen to which file.
    pub fn title(&self) -> String {
        let verb = if self.edit.expect.is_none() {
            "Create"
        } else {
            "Edit"
        };
        format!("{verb} {}", self.edit.path)
    }
}

impl App {
    /// What the model may do to files: `(edit, create)`, from the agent
    /// behind the switch; both while it is off, which is what turning it on
    /// gives.
    pub(crate) fn tools_scope(&self) -> (bool, bool) {
        match &self.harness {
            Some(h) => (
                h.agent().has(Tool::EditFile),
                h.agent().has(Tool::WriteFile),
            ),
            None => (true, true),
        }
    }

    /// The model can write something, so an approval may come.
    pub(crate) fn tools_write(&self) -> bool {
        let (edit, create) = self.tools_scope();
        self.tools_on && (edit || create)
    }

    /// The agent behind the switch swapped for the one with this scope.
    pub(crate) fn set_tools_scope(&mut self, edit: bool, create: bool) {
        if let Some(h) = self.harness.as_mut() {
            h.set_agent(editor_with(edit, create));
        }
    }

    /// An edit is on screen and the loop is paused on it.
    pub fn waiting_approval(&self) -> bool {
        matches!(self.panel, Some(Panel::Approval(_)))
    }

    /// Between the user's message and the model's last reply: a generation
    /// running, or the loop between two of them.
    pub fn turn_active(&self) -> bool {
        self.is_streaming() || self.harness.as_ref().is_some_and(Harness::in_turn)
    }

    /// On, from the panel or the configuration: the sandbox on the start-up
    /// directory and the editor agent behind it. Refuses only what cannot
    /// work at all.
    pub(crate) fn enable_tools(&mut self) -> Result<(), String> {
        if self.tools_on {
            return Ok(());
        }
        let sandbox = Sandbox::new(
            &self.root,
            self.cfg.tools.max_file_bytes,
            &self.cfg.tools.deny,
        )
        .map_err(|e| format!("cannot enable edits: {} · {e}", self.cwd))?;
        // the scope the configuration asks for; from the panel and on a
        // resume it is set again right after, from the boxes or the session
        self.harness = Some(Harness::new(
            editor_with(self.cfg.tools.edit, self.cfg.tools.create),
            sandbox,
            Limits::default(),
        ));
        self.tools_on = true;
        if let Some(c) = &self.current {
            if self.current_is_ollama() && self.caps.is_some_and(|caps| !caps.tools) {
                self.notify(format!(
                    "{} does not report tool support: the requests may fail · /model to pick another",
                    c.model
                ));
            }
        }
        if !self.root.join(".git").exists() {
            self.push_item(Item::Info(
                "✎ edits on · not a git repository: moon cannot undo what you apply".into(),
            ));
            self.follow = true;
        }
        self.update_session_meta();
        Ok(())
    }

    /// `/tools`: the panel, filled with how things are now.
    pub(crate) fn open_tools_dialog(&mut self) {
        // off, the boxes show off: what is ticked is what will be on
        let (edit, create) = if self.tools_on {
            self.tools_scope()
        } else {
            (false, false)
        };
        let rounds = match &self.harness {
            Some(h) => h.limits().rounds,
            None => Limits::default().rounds,
        };
        self.panel = Some(Panel::Tools(ToolsDialog {
            on: self.tools_on,
            edit,
            create,
            rounds,
            row: ToolsDialog::ON,
        }));
    }

    /// `Esc`: the panel's choices become the state.
    fn apply_tools_dialog(&mut self, d: ToolsDialog) {
        if !d.on {
            if self.tools_on {
                self.disable_tools();
                self.notify("tools off · the model only reads what you attach");
            }
            return;
        }
        if !self.tools_on {
            if let Err(e) = self.enable_tools() {
                self.notify(e);
                return;
            }
        }
        self.set_tools_scope(d.edit, d.create);
        if let Some(h) = self.harness.as_mut() {
            let mut limits = h.limits();
            limits.rounds = d.rounds;
            h.set_limits(limits);
        }
        self.update_session_meta();
        self.notify(match (d.edit, d.create) {
            (true, true) => "✎ edits on · the model can read, edit and create files under this directory, each edit with your ok",
            (true, false) => "✎ edits on · the model can read and edit files under this directory, not create them, each edit with your ok",
            (false, true) => "✎ edits on · the model can read and create files under this directory, not edit the ones that exist, each with your ok",
            (false, false) => "· reads on · the model can read and list files under this directory, and change nothing",
        });
    }

    pub(super) fn handle_tools_key(&mut self, key: KeyEvent) {
        enum Next {
            Stay,
            Apply,
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(Panel::Tools(d)) = self.panel.as_mut() else {
            return;
        };
        let next = match key.code {
            // no cancel: however it closes, what is set is what applies
            KeyCode::Esc | KeyCode::Char('q') => Next::Apply,
            KeyCode::Char('c') if ctrl => Next::Apply,
            KeyCode::Enter => {
                d.toggle();
                Next::Stay
            }
            KeyCode::Up | KeyCode::BackTab | KeyCode::Char('k') => {
                d.up();
                Next::Stay
            }
            KeyCode::Down | KeyCode::Tab | KeyCode::Char('j') => {
                d.down();
                Next::Stay
            }
            KeyCode::Char(' ') => {
                d.toggle();
                Next::Stay
            }
            KeyCode::Left | KeyCode::Char('h') => {
                d.change(-1);
                Next::Stay
            }
            KeyCode::Right | KeyCode::Char('l') => {
                d.change(1);
                Next::Stay
            }
            _ => Next::Stay,
        };
        match next {
            Next::Stay => {}
            Next::Apply => {
                if let Some(Panel::Tools(d)) = self.panel.take() {
                    self.apply_tools_dialog(d);
                }
            }
        }
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

    /// `Esc`, an error, the panel turning it off: the edit on screen is dropped and the
    /// calls left are closed as not run, so the history stays well formed.
    pub(super) fn abort_turn(&mut self) {
        if self.waiting_approval() {
            self.panel = None;
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

    /// What the loop asked for, done. Without `tx` nothing can be sent, so a
    /// `Continue` ends the turn instead.
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
                Command::Ask(edit) => {
                    self.panel = Some(Panel::Approval(Box::new(Approval::new(edit))));
                    self.follow = true;
                }
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
                EditChoice::Apply => Verdict::Apply,
                EditChoice::Skip => Verdict::Skip,
            }),
            KeyCode::Char('a') | KeyCode::Char('y') => Next::Verdict(Verdict::Apply),
            KeyCode::Char('s') | KeyCode::Char('n') => Next::Verdict(Verdict::Skip),
            // two options side by side: any direction walks them
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
                a.choice = a.choice.toggle();
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
