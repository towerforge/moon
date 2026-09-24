//! The model acting on the project, on screen: the `/tools` panel, the
//! loop's commands turned into conversation items and panels, the keys of
//! the approval panel, and the command that runs off the main thread. The
//! loop itself lives in `moon-agent`; this is where its commands meet the
//! interface.

use std::sync::atomic::{AtomicBool, Ordering};

use super::*;
use moon_agent::{
    editor_with, run_command, Agent, Category, Command, Entry, Event, Exec, Harness, Limits,
    Pending, PendingEdit, Policy, Sandbox, Stop, Verdict, CATALOG,
};
use moon_core::config::ids::{CREATE_FILES, EDIT_FILES, READ_FILES};
use moon_core::{Permission, ToolsFile};

/// Where the `/tools` panel stands: on the groups, or inside one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// The categories, one row each with what is on in them.
    Groups,
    /// Inside one category: its entries, one row each with its permission;
    /// in `Editor`, the step limit as the last row.
    Group(Category),
}

/// The `/tools` panel: two levels walked with the cursor. On the first, the
/// groups of the catalogue, each with a summary of what is on and a way to
/// turn the whole of it off or on; on the second, inside one group, its
/// entries with their permission as a selector. There is no cancel: `Esc`
/// steps back out of a group, and applies whatever is set from the groups.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolsDialog {
    pub policy: Policy,
    /// One per catalogue entry: the program is on this machine. What is
    /// not there stays off.
    pub found: Vec<bool>,
    /// Replies with tool calls one message may take before the turn stops.
    pub max_steps: usize,
    /// The tools were on when the panel opened: closing with nothing on
    /// is worth a word then.
    pub was_on: bool,
    pub level: Level,
    /// Row the cursor is on at the level it is at: a group; inside a
    /// group, one of its entries, or in `Editor` one past them for the
    /// step limit.
    pub row: usize,
    /// First body line shown, and the rows and lines of the last paint, to
    /// keep the cursor in view.
    pub scroll: usize,
    pub rows: usize,
    pub total: usize,
}

impl ToolsDialog {
    pub const ROUNDS_MAX: usize = 20;
    /// The group the step limit lives in, as its last row.
    pub const STEPS_GROUP: Category = Category::Editor;

    pub fn new(policy: Policy, max_steps: usize, found: Vec<bool>) -> Self {
        Self {
            was_on: !policy.is_empty(),
            policy,
            found,
            max_steps,
            level: Level::Groups,
            row: 0,
            scroll: 0,
            rows: 0,
            total: 0,
        }
    }

    /// Whether each entry of the catalogue is on this machine.
    pub fn probe() -> Vec<bool> {
        CATALOG.iter().map(|e| e.resolve().is_some()).collect()
    }

    /// The row of the step limit inside `Editor`: after its entries.
    pub fn steps_row(&self) -> usize {
        Self::members(Self::STEPS_GROUP).len()
    }

    /// The cursor on the step limit, inside `Editor`.
    pub fn go_to_steps(&mut self) {
        self.level = Level::Group(Self::STEPS_GROUP);
        self.row = self.steps_row();
        self.scroll = 0;
    }

    /// Inside `Editor`, on its last row.
    pub fn on_steps(&self) -> bool {
        self.level == Level::Group(Self::STEPS_GROUP) && self.row == self.steps_row()
    }

    /// The entries of a category, as indices into the catalogue.
    pub fn members(cat: Category) -> Vec<usize> {
        CATALOG
            .iter()
            .enumerate()
            .filter(|(_, e)| e.category == cat)
            .map(|(i, _)| i)
            .collect()
    }

    /// The category the cursor is on or in.
    pub fn category(&self) -> Option<Category> {
        match self.level {
            Level::Groups => Category::ALL.get(self.row).copied(),
            Level::Group(c) => Some(c),
        }
    }

    /// Inside a group, the index into the catalogue of the entry under the
    /// cursor; none on the step limit.
    pub fn index(&self) -> Option<usize> {
        match self.level {
            Level::Groups => None,
            Level::Group(c) => Self::members(c).get(self.row).copied(),
        }
    }

    /// Inside a group, the entry under the cursor.
    pub fn entry(&self) -> Option<&'static Entry> {
        self.index().and_then(|i| CATALOG.get(i))
    }

    pub fn found_at(&self, i: usize) -> bool {
        self.found.get(i).copied().unwrap_or(true)
    }

    /// Into the group of the entry with this id, the cursor on it.
    pub fn go_to(&mut self, id: &str) {
        let Some(e) = moon_agent::catalog::find(id) else {
            return;
        };
        let Some(i) = CATALOG.iter().position(|x| x.id == e.id) else {
            return;
        };
        let cat = CATALOG[i].category;
        self.level = Level::Group(cat);
        self.row = Self::members(cat).iter().position(|m| *m == i).unwrap_or(0);
        self.scroll = 0;
    }

    /// Rows at the level the cursor is at.
    fn len(&self) -> usize {
        match self.level {
            Level::Groups => Category::ALL.len(),
            Level::Group(c) if c == Self::STEPS_GROUP => Self::members(c).len() + 1,
            Level::Group(c) => Self::members(c).len(),
        }
    }

    pub fn up(&mut self) {
        let n = self.len().max(1);
        self.row = (self.row + n - 1) % n;
    }

    pub fn down(&mut self) {
        self.row = (self.row + 1) % self.len().max(1);
    }

    /// `enter`: on a group, into it; inside one, the entry on and off.
    pub fn enter(&mut self) {
        match self.level {
            Level::Groups => {
                if let Some(cat) = self.category() {
                    self.level = Level::Group(cat);
                    self.row = 0;
                    self.scroll = 0;
                }
            }
            Level::Group(_) => self.toggle(),
        }
    }

    /// `esc` inside a group: back to the groups, the cursor on the one
    /// left. From the groups there is nowhere back to: `false`.
    pub fn back(&mut self) -> bool {
        match self.level {
            Level::Groups => false,
            Level::Group(c) => {
                self.level = Level::Groups;
                self.row = Category::ALL.iter().position(|x| *x == c).unwrap_or(0);
                self.scroll = 0;
                true
            }
        }
    }

    /// `space`: on and off. Inside a group, the entry: on is what the
    /// catalogue turns it to, `allow` for what only looks, `ask` for what
    /// changes things, and a program that is not installed stays off; the
    /// step limit has no off. On a group, the whole of it: off if anything
    /// is on, its defaults otherwise.
    pub fn toggle(&mut self) {
        match self.level {
            Level::Groups => {
                let Some(cat) = self.category() else {
                    return;
                };
                if self.group_on(cat) {
                    self.group_off(cat);
                } else {
                    self.group_defaults(cat);
                }
            }
            Level::Group(_) => {
                let Some(i) = self.index() else {
                    return;
                };
                let e = &CATALOG[i];
                if self.policy.allows(e.id) {
                    self.policy.set(e.id, Permission::Off);
                } else if self.found_at(i) {
                    self.policy.set(e.id, e.on);
                }
            }
        }
    }

    /// `←`/`→`: inside a group, one step along `off · ask · allow` (`off ·
    /// allow` for where commands run, which has nothing to ask), or the
    /// number on the step limit; on a group, the whole of it off or to its
    /// defaults.
    pub fn change(&mut self, delta: i32) {
        match self.level {
            Level::Groups => match self.category() {
                Some(cat) if delta < 0 => self.group_off(cat),
                Some(cat) => self.group_defaults(cat),
                None => {}
            },
            Level::Group(_) if self.on_steps() => {
                self.max_steps = (self.max_steps as i64 + delta as i64)
                    .clamp(1, Self::ROUNDS_MAX as i64) as usize;
            }
            Level::Group(_) => {
                let Some(i) = self.index() else {
                    return;
                };
                if !self.found_at(i) {
                    return;
                }
                let e = &CATALOG[i];
                let order: &[Permission] = if e.kind == moon_agent::Kind::Subfolders {
                    &[Permission::Off, Permission::Allow]
                } else {
                    &[Permission::Off, Permission::Ask, Permission::Allow]
                };
                let at = order
                    .iter()
                    .position(|p| *p == self.policy.get(e.id))
                    .unwrap_or(0) as i64;
                let next = (at + delta as i64).clamp(0, order.len() as i64 - 1) as usize;
                self.policy.set(e.id, order[next]);
            }
        }
    }

    /// Anything on in the group.
    pub fn group_on(&self, cat: Category) -> bool {
        Self::members(cat)
            .into_iter()
            .any(|i| self.policy.allows(CATALOG[i].id))
    }

    fn group_off(&mut self, cat: Category) {
        for i in Self::members(cat) {
            self.policy.set(CATALOG[i].id, Permission::Off);
        }
    }

    /// Every entry of the group to what the catalogue turns it to, the ones
    /// that are not installed left off.
    fn group_defaults(&mut self, cat: Category) {
        for i in Self::members(cat) {
            if self.found_at(i) {
                self.policy.set(CATALOG[i].id, CATALOG[i].on);
            }
        }
    }

    /// What a group has on, for its row: `allow: git status, git diff ·
    /// ask: git commit`, or `off`; `Editor` says its step limit too.
    pub fn summary(&self, cat: Category) -> String {
        let names = |p: Permission| {
            Self::members(cat)
                .into_iter()
                .map(|i| CATALOG[i].id)
                .filter(|id| self.policy.get(id) == p)
                .collect::<Vec<_>>()
        };
        let mut parts = Vec::new();
        for p in [Permission::Allow, Permission::Ask] {
            let n = names(p);
            if !n.is_empty() {
                parts.push(format!("{}: {}", p.as_str(), n.join(", ")));
            }
        }
        if parts.is_empty() {
            parts.push("off".to_string());
        }
        if cat == Self::STEPS_GROUP {
            parts.push(format!("{} steps", self.max_steps));
        }
        parts.join(" · ")
    }

    /// Left by the paint: `rows` shown of `total`, and the scroll moved so
    /// the line the cursor is on stays in view.
    pub fn show(&mut self, cursor_line: usize, total: usize, rows: usize) {
        self.rows = rows;
        self.total = total;
        if rows == 0 {
            self.scroll = 0;
            return;
        }
        if cursor_line < self.scroll {
            self.scroll = cursor_line;
        } else if cursor_line >= self.scroll + rows {
            self.scroll = cursor_line + 1 - rows;
        }
        self.scroll = self.scroll.min(total.saturating_sub(rows));
    }
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
    pub fn new(pending: Pending) -> Self {
        Self {
            pending,
            choice: EditChoice::Apply,
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

    /// Title of the panel: what would happen to which file, or what would run.
    pub fn title(&self) -> String {
        match &self.pending {
            Pending::Edit(e) => {
                let verb = if e.expect.is_none() { "Create" } else { "Edit" };
                format!("{verb} {}", e.path)
            }
            Pending::Run(e) => format!("Run {}", e.id),
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

    /// The word on the confirming chip and in the hints: `Apply` an edit,
    /// `Run` a command.
    pub fn verb(&self) -> &'static str {
        match &self.pending {
            Pending::Edit(_) => "Apply",
            Pending::Run(_) => "Run",
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
    /// What the model may do right now; nothing while the tools are off.
    pub(crate) fn policy(&self) -> Policy {
        match &self.harness {
            Some(h) => h.agent().policy.clone(),
            None => Policy::default(),
        }
    }

    /// What the model may do to files: `(edit, create)`, from the agent
    /// behind the switch; both while it is off, which is what turning it on
    /// gives.
    pub(crate) fn tools_scope(&self) -> (bool, bool) {
        match &self.harness {
            Some(h) => (h.agent().policy.edits(), h.agent().policy.creates()),
            None => (true, true),
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
        let (edit, create) = self.tools_scope();
        self.tools_on && (edit || create)
    }

    /// The policy made the state: nothing on is tools off; anything on
    /// turns them on and swaps the agent behind the switch.
    pub(crate) fn set_policy(&mut self, policy: Policy) {
        if policy.is_empty() {
            if self.tools_on {
                self.disable_tools();
            }
            return;
        }
        if !self.tools_on {
            if let Err(e) = self.enable_tools() {
                self.notify(e);
                return;
            }
        }
        if let Some(h) = self.harness.as_mut() {
            h.set_agent(Agent::for_policy(policy));
        }
    }

    /// The two writing boxes, on the policy as it is: what a session's
    /// meta line says.
    pub(crate) fn set_tools_scope(&mut self, edit: bool, create: bool) {
        let mut p = self.policy();
        p.set(READ_FILES, Permission::Allow);
        let ask = |on: bool| if on { Permission::Ask } else { Permission::Off };
        p.set(EDIT_FILES, ask(edit));
        p.set(CREATE_FILES, ask(create));
        self.set_policy(p);
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

    /// On, from the panel or the configuration: the sandbox on the start-up
    /// directory and, behind it, the agent the configuration's permissions
    /// call for. Refuses only what cannot work at all.
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
        // the permissions the configuration asks for; from the panel, the
        // file and on a resume they are set again right after
        let policy = Policy::from_pairs(self.cfg.tools.startup_permissions());
        let agent = if policy.is_empty() {
            editor_with(self.cfg.tools.edit, self.cfg.tools.create)
        } else {
            Agent::for_policy(policy)
        };
        self.harness = Some(Harness::new(agent, sandbox, Limits::default()));
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
            let warn = "✎ edits on · not a git repository: moon cannot undo what you apply";
            // from `/tools`, under the command, even while its panel is still
            // open; from the configuration, on its own
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

    /// The panel's file read again and, if it changed since the last time,
    /// made the state: the permissions and the limit. `Ok(true)` when
    /// there is a file and it was applied now.
    pub(crate) fn sync_tools_file(&mut self) -> Result<bool, String> {
        let Some(path) = self.tools_file.clone() else {
            return Ok(false);
        };
        let f = ToolsFile::load(&path).map_err(|e| e.to_string())?;
        let Some(f) = f else {
            return Ok(false);
        };
        if self.tools_seen.as_ref() == Some(&f) {
            return Ok(false);
        }
        self.apply_tools_file(&f);
        self.tools_seen = Some(f);
        Ok(true)
    }

    /// `sync_tools_file` where a change is worth a word: before a message
    /// and when the panel opens.
    pub(crate) fn reload_tools_file(&mut self) {
        match self.sync_tools_file() {
            Ok(true) => self.notify(format!("{} changed · reloaded", ToolsFile::FILE)),
            Ok(false) => {}
            Err(e) => self.notify(e),
        }
    }

    fn apply_tools_file(&mut self, f: &ToolsFile) {
        self.set_policy(Policy::from_pairs(f.permissions.clone()));
        self.set_max_steps(f.max_steps);
        self.update_session_meta();
    }

    fn set_max_steps(&mut self, n: usize) {
        if let Some(h) = self.harness.as_mut() {
            let mut limits = h.limits();
            limits.rounds = n.clamp(1, ToolsDialog::ROUNDS_MAX);
            h.set_limits(limits);
        }
    }

    fn max_steps(&self) -> usize {
        self.harness
            .as_ref()
            .map_or(Limits::default().rounds, |h| h.limits().rounds)
    }

    /// The state, as the file would say it.
    fn tools_file_state(&self) -> ToolsFile {
        ToolsFile {
            max_steps: self.max_steps(),
            permissions: self.policy().pairs(),
        }
    }

    /// The state written to the panel's file, so it is there next time.
    fn save_tools_file(&mut self) {
        let Some(path) = self.tools_file.clone() else {
            return;
        };
        // the whole catalogue, off included, so the file shows what there is
        let f = self.tools_file_state();
        let text = moon_agent::catalog::render_tools_file(&self.policy(), f.max_steps);
        match ToolsFile::write_text(&path, &text) {
            Ok(()) => self.tools_seen = Some(f),
            Err(e) => self.notify(format!("could not save {}: {e}", ToolsFile::FILE)),
        }
    }

    /// `/tools`: the panel, filled with how things are now, the file read
    /// first in case it was edited by hand.
    pub(crate) fn open_tools_dialog(&mut self) {
        self.reload_tools_file();
        self.panel = Some(Panel::Tools(ToolsDialog::new(
            self.policy(),
            self.max_steps(),
            ToolsDialog::probe(),
        )));
    }

    /// The panel's choices made the state and written to the file: on the
    /// way out of a group, quietly; on closing, with a word on what is on,
    /// or that it went off.
    fn apply_tools_dialog(&mut self, d: &ToolsDialog, announce: bool) {
        self.set_policy(d.policy.clone());
        if self.tools_on {
            self.set_max_steps(d.max_steps);
            self.update_session_meta();
        }
        self.save_tools_file();
        if !announce {
            return;
        }
        if !self.tools_on {
            if d.was_on {
                self.notify("tools off · the model only reads what you attach");
            }
            return;
        }
        let names = |p: Permission| {
            d.policy
                .entries()
                .into_iter()
                .filter(|(_, x)| *x == p)
                .map(|(e, _)| e.id)
                .collect::<Vec<_>>()
                .join(", ")
        };
        let (allow, ask) = (names(Permission::Allow), names(Permission::Ask));
        let mut line = String::from("⏵⏵ tools on");
        if !allow.is_empty() {
            line.push_str(&format!(" · allow: {allow}"));
        }
        if !ask.is_empty() {
            line.push_str(&format!(" · ask: {ask}"));
        }
        self.notify(line);
        // writes let through without the diff: git is the only way back
        let unwatched = [EDIT_FILES, CREATE_FILES]
            .iter()
            .any(|id| d.policy.get(id) == Permission::Allow);
        if unwatched && !self.root.join(".git").exists() {
            self.notify(
                "✎ edits allowed without asking · not a git repository: nothing can undo them",
            );
        }
    }

    pub(super) fn handle_tools_key(&mut self, key: KeyEvent) {
        enum Next {
            Stay,
            /// Out of a group: what is set applies, and the panel stays.
            Save,
            /// Closed: what is set applies, and is said.
            Close,
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(Panel::Tools(d)) = self.panel.as_mut() else {
            return;
        };
        let next = match key.code {
            // no cancel: esc always saves, on its way out of a group or
            // closing the panel from the groups
            KeyCode::Esc | KeyCode::Backspace => {
                if d.back() {
                    Next::Save
                } else {
                    Next::Close
                }
            }
            KeyCode::Char('c') if ctrl => Next::Close,
            KeyCode::Enter => {
                d.enter();
                Next::Stay
            }
            KeyCode::Char(' ') => {
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
            Next::Save => {
                let d = d.clone();
                self.apply_tools_dialog(&d, false);
            }
            Next::Close => {
                if let Some(Panel::Tools(d)) = self.panel.take() {
                    self.apply_tools_dialog(&d, true);
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
                EditChoice::Apply => Verdict::Apply,
                EditChoice::Skip => Verdict::Skip,
            }),
            KeyCode::Char('a') | KeyCode::Char('y') | KeyCode::Char('r') => {
                Next::Verdict(Verdict::Apply)
            }
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
