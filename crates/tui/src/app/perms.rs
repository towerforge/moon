//! The permissions of one agent, docked and in two levels: the groups
//! first, each with a summary of what the agent may do; enter opens one,
//! and inside, one row per capability with the `off · ask · allow`
//! selector. One column, because an agent carries its permissions whole
//! in its file, and every step is written to it at once. Opened with
//! `ctrl+t` from the `/agent` picker, or by `/tools`, its alias, over the
//! chosen agent.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use moon_agent::{Category, Kind, CATALOG, DEFAULT_STEPS};
use moon_core::config::ids::{CREATE_FILES, EDIT_FILES};
use moon_core::Permission;

use super::*;

/// Where the panel stands: on the groups, or inside one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermsLevel {
    Groups,
    Group(Category),
}

/// One row of a group's table the cursor can stand on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermRow {
    /// An entry of the catalogue, by its index there.
    Entry(usize),
    /// The step limit, after the editor's entries: the agent's, yours,
    /// and the lower one runs.
    Steps,
}

/// One group's rows, the step limit included in `Editor`.
pub fn group_rows(cat: Category) -> Vec<PermRow> {
    let mut out: Vec<PermRow> = CATALOG
        .iter()
        .enumerate()
        .filter(|(_, e)| e.category == cat)
        .map(|(i, _)| PermRow::Entry(i))
        .collect();
    if cat == Category::Editor {
        out.push(PermRow::Steps);
    }
    out
}

/// How many section titles the group draws over its rows: the languages
/// of `Stack`, none elsewhere.
pub fn group_sections(cat: Category) -> usize {
    let mut n = 0;
    let mut last = None;
    for e in CATALOG.iter().filter(|e| e.category == cat) {
        if e.section.is_some() && e.section != last {
            n += 1;
        }
        last = e.section;
    }
    n
}

/// Whether each entry of the catalogue is on this machine.
pub fn probe() -> Vec<bool> {
    CATALOG.iter().map(|e| e.resolve().is_some()).collect()
}

/// The table on screen: whose permissions, the cell the cursor is on,
/// and what the last paint saw, so the scroll can follow the cursor.
pub struct PermsView {
    pub name: String,
    /// Opened from the picker, which esc returns to; from `/tools`, esc
    /// just closes.
    pub from_picker: bool,
    pub level: PermsLevel,
    /// The group the cursor is on at the first level.
    pub row: usize,
    /// Inside a group: the row the cursor is on.
    pub perm: usize,
    pub scroll: usize,
    pub rows: usize,
    pub total: usize,
    /// One per catalogue entry: the program is on this machine.
    pub found: Vec<bool>,
}

impl App {
    /// `ctrl+t` in the picker, or `/tools`: the table over one agent, the
    /// folder read again first in case a file was edited by hand.
    pub(crate) fn open_perms(&mut self, name: &str, from_picker: bool) {
        self.sync_agents();
        self.panel = Some(Panel::Perms(Box::new(PermsView {
            name: name.to_string(),
            from_picker,
            level: PermsLevel::Groups,
            row: 0,
            perm: 0,
            scroll: 0,
            rows: 0,
            total: 0,
            found: probe(),
        })));
    }

    /// `/tools`, kept for a while as an alias: the table over the agent
    /// of the conversation.
    pub(crate) fn open_agents_permissions(&mut self) {
        let name = self.agent.clone();
        self.open_perms(&name, false);
    }

    pub(super) fn handle_perms_key(&mut self, key: KeyEvent) {
        enum Next {
            Stay,
            Close,
            /// `←`/`→` on a row: one step along `off · ask · allow`.
            Walk(String, PermRow, i32),
            /// Enter or space: off, and back to what the row is for.
            Toggle(String, PermRow),
            /// A digit on the step limit.
            Set(String, usize),
            /// `←`/`→` on a group: the whole of it.
            Group(String, Category, bool),
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(Panel::Perms(p)) = self.panel.as_mut() else {
            return;
        };
        let next = match p.level {
            // the groups: enter opens one, ←→ turn the whole of it off or
            // on with its defaults
            PermsLevel::Groups => match key.code {
                KeyCode::Esc | KeyCode::Backspace => Next::Close,
                KeyCode::Char('c') if ctrl => Next::Close,
                KeyCode::Enter => {
                    if let Some(cat) = Category::ALL.get(p.row) {
                        p.level = PermsLevel::Group(*cat);
                        p.perm = 0;
                        p.scroll = 0;
                    }
                    Next::Stay
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    let n = Category::ALL.len();
                    p.row = (p.row + n - 1) % n;
                    Next::Stay
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    p.row = (p.row + 1) % Category::ALL.len();
                    Next::Stay
                }
                KeyCode::Left | KeyCode::Char('h') => match Category::ALL.get(p.row) {
                    Some(cat) => Next::Group(p.name.clone(), *cat, false),
                    None => Next::Stay,
                },
                KeyCode::Right | KeyCode::Char('l') => match Category::ALL.get(p.row) {
                    Some(cat) => Next::Group(p.name.clone(), *cat, true),
                    None => Next::Stay,
                },
                _ => Next::Stay,
            },
            // inside one: the cell cursor over its table
            PermsLevel::Group(cat) => {
                let rows = group_rows(cat);
                let on_row = |p: &PermsView| rows.get(p.perm).copied();
                match key.code {
                    // esc steps back out onto the group it leaves
                    KeyCode::Esc | KeyCode::Backspace => {
                        p.level = PermsLevel::Groups;
                        p.row = Category::ALL.iter().position(|x| *x == cat).unwrap_or(0);
                        p.scroll = 0;
                        Next::Stay
                    }
                    KeyCode::Char('c') if ctrl => Next::Close,
                    KeyCode::Enter | KeyCode::Char(' ') => match on_row(p) {
                        Some(r) => Next::Toggle(p.name.clone(), r),
                        None => Next::Stay,
                    },
                    KeyCode::Up | KeyCode::Char('k') => {
                        p.perm = p.perm.saturating_sub(1);
                        Next::Stay
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        p.perm = (p.perm + 1).min(rows.len() - 1);
                        Next::Stay
                    }
                    KeyCode::Left | KeyCode::Char('h') => match on_row(p) {
                        Some(r) => Next::Walk(p.name.clone(), r, -1),
                        None => Next::Stay,
                    },
                    KeyCode::Right | KeyCode::Char('l') => match on_row(p) {
                        Some(r) => Next::Walk(p.name.clone(), r, 1),
                        None => Next::Stay,
                    },
                    KeyCode::Char(c) if ('1'..='9').contains(&c) && !ctrl => {
                        if on_row(p) == Some(PermRow::Steps) {
                            Next::Set(p.name.clone(), (c as u8 - b'0') as usize)
                        } else {
                            Next::Stay
                        }
                    }
                    KeyCode::Home => {
                        p.perm = 0;
                        Next::Stay
                    }
                    KeyCode::End => {
                        p.perm = rows.len() - 1;
                        Next::Stay
                    }
                    _ => Next::Stay,
                }
            }
        };
        match next {
            Next::Stay => {}
            Next::Close => {
                if let Some(Panel::Perms(p)) = self.panel.take() {
                    if p.from_picker {
                        self.open_agent_picker();
                    }
                }
            }
            Next::Walk(x, row, delta) => self.walk_perm(&x, row, delta),
            Next::Toggle(x, row) => self.toggle_perm(&x, row),
            Next::Set(x, n) => self.set_steps(&x, n),
            Next::Group(x, cat, on) => self.set_group(&x, cat, on),
        }
    }

    /// `←`/`→` on a row: one step along `off · ask · allow`, clamped at
    /// the ends; where commands run knows no ask.
    fn walk_perm(&mut self, name: &str, row: PermRow, delta: i32) {
        match row {
            PermRow::Steps => {
                let cur = self.steps_of(name);
                let next = (cur as i64 + delta as i64).clamp(1, ROUNDS_MAX as i64) as usize;
                self.set_steps(name, next);
            }
            PermRow::Entry(i) => {
                let Some(e) = CATALOG.get(i) else {
                    return;
                };
                let order: &[Permission] = if e.kind == Kind::Subfolders {
                    &[Permission::Off, Permission::Allow]
                } else {
                    &[Permission::Off, Permission::Ask, Permission::Allow]
                };
                let cur = self.permission_of(name, e.id);
                let at = order.iter().position(|p| *p == cur).unwrap_or(0) as i64;
                let next = order[(at + delta as i64).clamp(0, order.len() as i64 - 1) as usize];
                self.write_permission(name, i, next);
            }
        }
    }

    /// Enter or space on a row: off, and back to what the catalogue turns
    /// it to — `allow` for what only looks, `ask` for what changes things.
    fn toggle_perm(&mut self, name: &str, row: PermRow) {
        match row {
            PermRow::Steps => {
                let cur = self.steps_of(name);
                let next = if cur >= ROUNDS_MAX { 1 } else { cur + 1 };
                self.set_steps(name, next);
            }
            PermRow::Entry(i) => {
                let Some(e) = CATALOG.get(i) else {
                    return;
                };
                let next = if self.permission_of(name, e.id) == Permission::Off {
                    e.on
                } else {
                    Permission::Off
                };
                self.write_permission(name, i, next);
            }
        }
    }

    /// `←`/`→` on a group: the whole of it off, or to its defaults, the
    /// programs that are missing left off.
    fn set_group(&mut self, name: &str, cat: Category, on: bool) {
        let Some(mut def) = self.def_of(name) else {
            return;
        };
        for e in CATALOG.iter().filter(|e| e.category == cat) {
            let p = if on && (!e.is_command() || e.resolve().is_some()) {
                e.on
            } else {
                Permission::Off
            };
            def.policy.set(e.id, p);
        }
        self.save_def(def);
    }

    /// The row's permission as the agent holds it now.
    fn permission_of(&self, name: &str, id: &str) -> Permission {
        self.def_of(name)
            .map(|d| d.policy.get(id))
            .unwrap_or(Permission::Off)
    }

    /// The step limit as the agent holds it now.
    pub(crate) fn steps_of(&self, name: &str) -> usize {
        self.def_of(name)
            .map(|d| d.steps())
            .unwrap_or(DEFAULT_STEPS)
    }

    /// One permission written to the agent's file, whole; a value of off
    /// stays listed as off, since the file shows everything there is.
    fn write_permission(&mut self, name: &str, entry: usize, next: Permission) {
        let Some(e) = CATALOG.get(entry) else {
            return;
        };
        if next != Permission::Off && e.is_command() && e.resolve().is_none() {
            self.notify(format!("{} is not installed", e.program()));
            return;
        }
        let Some(mut def) = self.def_of(name) else {
            return;
        };
        def.policy.set(e.id, next);
        self.save_def(def);
        // writes let through without the diff: git is the only way back
        if next == Permission::Allow
            && (e.id == EDIT_FILES || e.id == CREATE_FILES)
            && !self.root.join(".git").exists()
        {
            self.notify(
                "✎ edits allowed without asking · not a git repository: nothing can undo them",
            );
        }
    }

    /// The step limit written to the agent's file.
    fn set_steps(&mut self, name: &str, n: usize) {
        let Some(mut def) = self.def_of(name) else {
            return;
        };
        def.max_steps = Some(n.clamp(1, ROUNDS_MAX));
        self.save_def(def);
    }
}
