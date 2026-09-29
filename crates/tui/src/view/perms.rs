//! The permissions panel drawn, in two levels: the groups with a summary
//! of what the agent may do, and inside one, its rows with the
//! `◀ off · ask · allow ▶` selector.

use moon_agent::{AgentDef, Category, Policy, CATALOG};
use moon_core::Permission;

use super::panel::Content;
use super::*;
use crate::app::{group_rows, Panel, PermRow, PermsLevel};

/// What the panel shows: built from the app, the cursor clamped into
/// view, everything else read fresh so the table always says what the
/// files say.
pub(super) fn perms_content(app: &mut App, t: &Theme, w: usize, rows: usize) -> Content {
    let defs = app.agents.clone();
    let Some(Panel::Perms(p)) = app.panel.as_mut() else {
        unreachable!("the permissions table is open")
    };
    let def = defs
        .iter()
        .find(|d| d.name == p.name)
        .cloned()
        .unwrap_or_else(moon_agent::AgentDef::default_agent);
    let (title, hint, lines, cursor) = match p.level {
        PermsLevel::Groups => {
            let (lines, cursor) = groups_lines(t, &def.policy, def.steps(), p.row, w);
            (
                format!("Permissions · {}", p.name),
                format!(
                    " what {} may do, group by group · enter opens one · ←→ off or on",
                    p.name
                ),
                lines,
                cursor,
            )
        }
        PermsLevel::Group(cat) => {
            let (lines, cursor) = permission_lines(t, cat, &def, &p.found, p.perm, w);
            (
                format!("Permissions · {} › {}", p.name, cat.title()),
                format!(" {}", cat.about()),
                lines,
                cursor,
            )
        }
    };
    let total = lines.len();
    p.rows = rows;
    p.total = total;
    if let Some(c) = cursor {
        if c < p.scroll {
            p.scroll = c;
        } else if c + 1 > p.scroll + rows {
            p.scroll = c + 1 - rows;
        }
    }
    p.scroll = p.scroll.min(total.saturating_sub(rows));
    let info = format!("agents/{}", moon_agent::file_name(&p.name));
    Content {
        prompt: Vec::new(),
        title,
        tabs: Vec::new(),
        info,
        hint: Line::from(Span::styled(hint, t.muted())),
        body: lines.into_iter().skip(p.scroll).take(rows).collect(),
        total,
        scroll: p.scroll,
    }
}

/// The groups: ` ❯ Editor   ▸  allow: read files · ask: edit existing
/// files · 6 steps`, each summarising what actually runs for this agent.
fn groups_lines(
    t: &Theme,
    effective: &Policy,
    steps: usize,
    row: usize,
    w: usize,
) -> (Vec<Line<'static>>, Option<usize>) {
    let label_w = Category::ALL
        .iter()
        .map(|c| width(c.title()))
        .max()
        .unwrap_or(6);
    let mut out = Vec::new();
    let mut at = None;
    for (i, cat) in Category::ALL.iter().enumerate() {
        let selected = i == row;
        if selected {
            at = Some(out.len());
        }
        let names = |p: Permission| {
            CATALOG
                .iter()
                .filter(|e| e.category == *cat && effective.get(e.id) == p)
                .map(|e| e.id)
                .collect::<Vec<_>>()
                .join(", ")
        };
        let mut parts = Vec::new();
        let (allow, ask) = (names(Permission::Allow), names(Permission::Ask));
        if !allow.is_empty() {
            parts.push(format!("allow: {allow}"));
        }
        if !ask.is_empty() {
            parts.push(format!("ask: {ask}"));
        }
        if parts.is_empty() {
            parts.push("off".to_string());
        }
        if *cat == Category::Editor {
            parts.push(format!("{steps} steps"));
        }
        let summary = parts.join(" · ");
        let label = format!("{:<label_w$}", cat.title());
        let detail_w = w.saturating_sub(3 + label_w + 5);
        let on = CATALOG
            .iter()
            .any(|e| e.category == *cat && effective.allows(e.id));
        out.push(Line::from(vec![
            Span::styled(if selected { " ❯ " } else { "   " }, t.accent()),
            Span::styled(
                label,
                if selected {
                    t.soft_bold()
                } else if on {
                    t.text()
                } else {
                    t.muted()
                },
            ),
            Span::styled("  ", t.text()),
            Span::styled("▸", if selected { t.soft() } else { t.accent() }),
            Span::styled("  ", t.text()),
            Span::styled(truncate(&summary, detail_w), t.muted()),
        ]));
    }
    (out, at)
}

/// One group's rows: the id, the `◀ value ▶` selector coloured by what it
/// is, and the entry's help, with a ` Rust ────` title over each section of
/// `Stack`. The line the cursor is on comes back for the scroll.
fn permission_lines(
    t: &Theme,
    cat: Category,
    d: &AgentDef,
    found: &[bool],
    cursor: usize,
    w: usize,
) -> (Vec<Line<'static>>, Option<usize>) {
    let policy = &d.policy;
    let id_w = CATALOG
        .iter()
        .filter(|e| e.category == cat)
        .map(|e| width(e.id))
        .max()
        .unwrap_or(12)
        .max(if cat == Category::Editor {
            width("max steps per message")
        } else {
            0
        });
    let mut at = None;
    let mut out = Vec::new();
    let mut section = None;
    for (ri, row) in group_rows(cat).into_iter().enumerate() {
        // a title where a section starts: a line, not a row the cursor stops on
        if let PermRow::Entry(i) = row {
            if let Some(s) = CATALOG[i].section.filter(|_| CATALOG[i].section != section) {
                section = CATALOG[i].section;
                let title = format!(" {}", s.title());
                let fill = w.saturating_sub(width(&title) + 2);
                out.push(Line::from(vec![
                    Span::styled(title, t.accent()),
                    Span::styled(format!(" {}", "─".repeat(fill)), t.line()),
                ]));
            }
        }
        let sel = ri == cursor;
        if sel {
            at = Some(out.len());
        }
        match row {
            PermRow::Entry(i) => {
                let e = &CATALOG[i];
                let p = policy.get(e.id);
                let missing = !found.get(i).copied().unwrap_or(true);
                let control = format!("◀ {:^5} ▶", if missing { "off" } else { p.as_str() });
                let control_style = match p {
                    _ if missing => t.muted(),
                    Permission::Allow => t.accent(),
                    Permission::Ask => t.soft(),
                    Permission::Off => t.muted(),
                };
                let detail = if missing { "not installed" } else { e.help };
                out.push(Line::from(vec![
                    Span::styled(if sel { " ❯ " } else { "   " }, t.accent()),
                    Span::styled(
                        format!("{:<id_w$}  ", e.id),
                        if sel {
                            t.soft_bold()
                        } else if missing || p == Permission::Off {
                            t.muted()
                        } else {
                            t.text()
                        },
                    ),
                    Span::styled(control, control_style),
                    Span::styled(format!("  {detail}"), t.muted()),
                ]));
            }
            // the step limit as the agent holds it
            PermRow::Steps => {
                let n = d.steps();
                out.push(Line::from(vec![
                    Span::styled(if sel { " ❯ " } else { "   " }, t.accent()),
                    Span::styled(
                        format!("{:<id_w$}  ", "max steps per message"),
                        if sel { t.soft_bold() } else { t.text() },
                    ),
                    Span::styled(format!("◀ {n:^5} ▶"), t.accent()),
                    Span::styled("  tool calls one message may take", t.muted()),
                ]));
            }
        }
    }
    (out, at)
}

#[cfg(test)]
mod tests {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use super::super::tests::{app, screen};
    use super::*;

    #[test]
    fn the_permissions_panel_walks_two_levels() {
        let mut app = app();
        app.loading = false;
        let agents = tempfile::tempdir().unwrap();
        std::fs::write(
            agents.path().join("committer.toml"),
            "description = \"x\"\ninherit = false\nmax_steps = 6\nprompt = \"y\"\n\n\
             [permissions]\n\"git diff\" = \"allow\"\n",
        )
        .unwrap();
        app.agents_dir = Some(agents.path().to_path_buf());
        app.open_perms("committer", true);
        let mut term = Terminal::new(TestBackend::new(100, 30)).unwrap();
        term.draw(|f| view(&mut app, f)).unwrap();
        let s = screen(&term);
        // the groups first, one row each with what runs for this agent
        let title = s
            .iter()
            .position(|l| l.contains("Permissions · committer"))
            .expect("title row");
        assert!(s[title].contains("agents/committer.toml"), "{}", s[title]);
        let row = |needle: &str| {
            s.iter()
                .find(|l| l.contains(needle))
                .unwrap_or_else(|| panic!("no row with {needle}: {s:?}"))
                .clone()
        };
        assert!(row(" Editor ").contains("▸"), "{s:?}");
        // the agent's own permissions, whole: git diff runs whatever the
        // grants say, and the step limit is its own
        assert!(row(" Git ").contains("allow: git diff"), "{s:?}");
        assert!(row(" Editor ").contains("6 steps"), "{s:?}");
        assert!(s[29].contains("enter open"), "{}", s[29]);

        // inside Git: one row per entry, the selector coloured
        if let Some(Panel::Perms(p)) = app.panel.as_mut() {
            p.level = PermsLevel::Group(moon_agent::Category::Git);
            p.perm = 1;
        }
        term.draw(|f| view(&mut app, f)).unwrap();
        let s = screen(&term);
        assert!(
            s.iter()
                .any(|l| l.contains("Permissions · committer › Git")),
            "{s:?}"
        );
        let diff = s.iter().find(|l| l.contains("git diff")).unwrap();
        assert!(diff.contains("❯") && diff.contains("◀ allow ▶"), "{diff}");
        let status = s.iter().find(|l| l.contains("git status")).unwrap();
        assert!(status.contains("◀  off  ▶"), "{status}");
        assert!(!s.iter().any(|l| l.contains("cargo")), "{s:?}");
        assert!(s[29].contains("esc"), "{}", s[29]);

        // the editor group carries the step limit as its last row
        if let Some(Panel::Perms(p)) = app.panel.as_mut() {
            p.level = PermsLevel::Group(moon_agent::Category::Editor);
            p.perm = group_rows(moon_agent::Category::Editor).len() - 1;
        }
        term.draw(|f| view(&mut app, f)).unwrap();
        let s = screen(&term);
        let steps = s
            .iter()
            .find(|l| l.contains("max steps per message"))
            .unwrap();
        assert!(
            steps.contains("❯") && steps.contains("◀   6   ▶"),
            "{steps}"
        );

        // Stack sets each language apart under a title the cursor skips:
        // the first row is make, under Make
        if let Some(Panel::Perms(p)) = app.panel.as_mut() {
            p.level = PermsLevel::Group(moon_agent::Category::Stack);
            p.perm = 0;
            p.scroll = 0;
        }
        term.draw(|f| view(&mut app, f)).unwrap();
        let s = screen(&term);
        let at = |needle: &str| {
            s.iter()
                .position(|l| l.contains(needle))
                .unwrap_or_else(|| panic!("no {needle}: {s:?}"))
        };
        let make = at(" make ");
        assert!(s[make].contains("❯"), "{}", s[make]);
        assert!(s[make - 1].contains("Make ─"), "{}", s[make - 1]);
        assert!(s[at("cargo check") - 1].contains("Rust ─"), "{s:?}");
        assert!(at("Node ─") < at("npm run"), "{s:?}");
    }
}
