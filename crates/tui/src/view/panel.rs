//! The bottom panel: the model and session lists, the help and the session
//! dialogs. Nothing floats over the conversation any more and nothing paints a
//! surface: the panel unfolds under it, in the input box's place, on the same
//! background, and the box comes back when it closes.

use super::*;

/// Fixed rows of a panel: title, hint, blank, …body…, blank, footer.
pub(super) const CHROME: u16 = 5;
/// Rows the conversation keeps however much the panel would like to grow.
const CONV_MIN: u16 = 3;
/// What sits between the conversation and the panel: the status row and the
/// separator.
const ABOVE: u16 = 2;
/// Fewest body rows a panel settles for.
const MIN_BODY: usize = 6;

/// Body rows of a list: half the screen, never fewer than `MIN_BODY`.
fn list_body(area: Rect) -> usize {
    ((area.height / 2) as usize).max(MIN_BODY)
}

/// The help is a long read: it takes two thirds at most, and only as much of
/// that as the open section fills.
fn help_body(app: &App, h: &HelpState, area: Rect) -> usize {
    let max = (area.height as usize * 2 / 3).max(MIN_BODY);
    let body_w = area.width.saturating_sub(2) as usize;
    help_lines(app, &app.theme, h.tab, body_w)
        .len()
        .clamp(MIN_BODY, max)
}

/// Rows the open panel takes, its separator aside. `0` if there is none.
pub(super) fn height(app: &App, area: Rect) -> u16 {
    let Some(panel) = &app.panel else { return 0 };
    let body = match panel {
        Panel::Models(p) | Panel::Sessions(p) | Panel::Files(p) => {
            p.rows().len().clamp(1, list_body(area))
        }
        Panel::Browse { picker, .. } => picker.rows().len().clamp(1, list_body(area)),
        Panel::Help(h) => help_body(app, h, area),
        Panel::Machine => machine::BODY,
        Panel::SessionAction { action, .. } => match action {
            SessionAction::Delete { .. } => 2,
            SessionAction::Rename { .. } => 1,
        },
        Panel::Approval(a) => a.edit.diff.lines.len().clamp(1, list_body(area)),
        Panel::Tools(d) => tools_lines(&app.theme, d, area.width.saturating_sub(2) as usize).len(),
    };
    (body as u16 + CHROME).min(area.height.saturating_sub(CONV_MIN + ABOVE))
}

/// What a panel puts on screen, whatever it is underneath.
struct Content {
    title: String,
    /// Sections shown next to the title, and which one is open. Empty if the
    /// panel has none.
    tabs: Vec<(&'static str, bool)>,
    /// Right of the title: counts.
    info: String,
    /// Second row: what the panel is for, the filter, what is being decided.
    hint: Line<'static>,
    body: Vec<Line<'static>>,
    /// Body rows there are in total and the first one shown, for the `a-b/n`.
    total: usize,
    scroll: usize,
}

pub(super) fn render(app: &mut App, frame: &mut Frame, area: Rect) {
    if area.height == 0 {
        return;
    }
    // the only panel that is not a list of lines: it draws its own area
    if matches!(app.panel, Some(Panel::Machine)) {
        machine::render(app, frame, area);
        return;
    }
    let t = app.theme.clone();
    let w = area.width as usize;
    // on a short terminal the blank rows are the first to go: the list is
    // what is worth the room
    let compact = area.height < CHROME + 3;
    let chrome = if compact { CHROME - 2 } else { CHROME };
    let rows = area.height.saturating_sub(chrome) as usize;
    // the last column is the scroll bar's, with a blank one before it, so the
    // body never runs into it
    let body_w = w.saturating_sub(2);
    let keys = app.panel_keys();
    let cwd = app.cwd.clone();
    let c = if matches!(app.panel, Some(Panel::Help(_))) {
        help_content(app, &t, body_w, rows)
    } else {
        match app.panel.as_mut() {
            Some(Panel::Models(p)) | Some(Panel::Sessions(p)) | Some(Panel::Files(p)) => {
                list_content(&t, p, body_w, rows)
            }
            Some(Panel::Browse { picker, .. }) => list_content(&t, picker, body_w, rows),
            Some(Panel::SessionAction { action, .. }) => action_content(&t, action, body_w),
            Some(Panel::Approval(a)) => approval_content(&t, a, body_w, rows),
            Some(Panel::Tools(d)) => tools_content(&cwd, &t, d, body_w),
            _ => return,
        }
    };
    let mut lines = vec![title_line(&t, &c, w), c.hint];
    if !compact {
        lines.push(Line::from(""));
    }
    lines.extend(c.body);
    // the footer always sits on the last row: what does not fit is the body
    lines.resize(area.height.saturating_sub(1) as usize, Line::from(""));
    lines.push(panel_footer(&t, &keys, w, c.total, rows, c.scroll));
    // no surface of its own: the panel sits on the same background as the
    // conversation, the separator and the title tell it apart
    frame.render_widget(Paragraph::new(lines), area);
    // the bar spans the body only, over the free column on the right
    let body = Rect {
        y: area.y + if compact { 2 } else { 3 },
        height: rows as u16,
        ..area
    };
    render_scrollbar(frame, &t, body, c.total, rows, c.scroll);
}

/// Bar on the right of the body: `total` rows, `rows` of them visible from
/// `scroll`. Paints nothing if it all fits.
pub(super) fn render_scrollbar(
    frame: &mut Frame,
    t: &Theme,
    body: Rect,
    total: usize,
    rows: usize,
    scroll: usize,
) {
    if total <= rows || rows == 0 || body.height == 0 {
        return;
    }
    let mut state = ScrollbarState::new(total - rows + 1)
        .position(scroll)
        .viewport_content_length(rows);
    let bar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some("│"))
        .thumb_symbol("█")
        .track_style(t.line())
        .thumb_style(t.soft());
    frame.render_stateful_widget(bar, body, &mut state);
}

/// First row: the title in `moon`, the sections next to it when the panel has
/// any, and on the right what there is to count. The open section is the only
/// thing in the panel painted on `moon`, so the eye lands on it.
fn title_line(t: &Theme, c: &Content, w: usize) -> Line<'static> {
    head_line(t, &c.title, &c.tabs, &c.info, w)
}

/// Title of a block: the name in `moon`, the sections it may have, and the
/// count on the right.
pub(super) fn head_line(
    t: &Theme,
    title: &str,
    tabs: &[(&'static str, bool)],
    info: &str,
    w: usize,
) -> Line<'static> {
    let title = format!(" {title}");
    let mut used = width(&title);
    let mut spans = vec![Span::styled(title, t.accent_bold())];
    for (label, open) in tabs {
        let chip = format!(" {label} ");
        used += width(&chip) + 1;
        spans.push(Span::styled(" ", t.text()));
        spans.push(Span::styled(
            chip,
            if *open { t.selected() } else { t.muted() },
        ));
    }
    // with no room for both, the sections win: the count is the first to go
    if !info.is_empty() {
        if let Some(pad) = w.checked_sub(used + width(info) + 1) {
            spans.push(Span::styled(" ".repeat(pad), t.text()));
            spans.push(Span::styled(format!("{info} "), t.muted()));
        }
    }
    Line::from(spans)
}

/// Second row of a list: what it is for, or the filter as it is typed.
fn filter_line(t: &Theme, p: &Picker, w: usize) -> Line<'static> {
    if let Some(n) = p.pending {
        return Line::from(vec![
            Span::styled(" going to ", t.muted()),
            Span::styled(n.to_string(), t.soft()),
            Span::styled("… · another digit, or enter", t.muted()),
        ]);
    }
    if p.query.is_empty() {
        return Line::from(Span::styled(
            format!(" {}", truncate(&p.hint, w.saturating_sub(2))),
            t.muted(),
        ));
    }
    Line::from(vec![
        Span::styled(" filter ", t.muted()),
        Span::styled(truncate(&p.query, w.saturating_sub(10)), t.text()),
        Span::styled("█", t.accent()),
    ])
}

/// A row of the list: `❯` when it is under the cursor, its number while there
/// is a digit for it, `✓` right after the text when it is the one in use, and
/// the detail to the right.
#[derive(Default)]
struct RowState {
    /// Place in the list as it is drawn, and the width its column takes so
    /// every label starts at the same column. Without a number, no column.
    n: Option<usize>,
    num_w: usize,
    selected: bool,
    /// The one in use: the model of the conversation, the open session.
    active: bool,
    dim: bool,
}

fn row_line(t: &Theme, label: &str, detail: &str, st: RowState, w: usize) -> Line<'static> {
    let RowState {
        n,
        num_w,
        selected,
        active,
        dim,
    } = st;
    // the row under the cursor goes in moon-soft, in use or not: `❯` marks
    // where you are, `✓` what is in use
    let label_style = if dim {
        t.muted()
    } else if selected {
        t.soft_bold()
    } else if active {
        t.accent()
    } else {
        t.text()
    };
    let num = match n {
        Some(n) => format!("{n:>w$}. ", w = num_w.saturating_sub(2)),
        None => " ".repeat(num_w),
    };
    let check = if active { " ✓" } else { "" };
    // cursor, number, the check after the text and the space at the end
    let body_w = w.saturating_sub(4 + num_w + width(check));
    let detail_w = width(detail);
    let label = truncate(label, body_w.saturating_sub(detail_w + 2).max(12));
    let pad = body_w.saturating_sub(width(&label) + detail_w);
    Line::from(vec![
        Span::styled(if selected { " ❯ " } else { "   " }, t.accent()),
        Span::styled(num, if selected { t.soft() } else { t.muted() }),
        Span::styled(label, label_style),
        Span::styled(check, t.accent()),
        Span::styled(" ".repeat(pad), t.text()),
        Span::styled(detail.to_string(), t.muted()),
    ])
}

/// Section header inside the panel: ` title ─────── ● info `.
pub(super) fn section_line(t: &Theme, g: &PickerGroup, w: usize) -> Line<'static> {
    let title = format!(" {}", g.title);
    let mark = match g.mark {
        Some(true) => Some(Span::styled("● ", t.accent())),
        Some(false) => Some(Span::styled("✗ ", t.muted())),
        None => None,
    };
    let info_w = width(&g.info) + mark.as_ref().map_or(0, |_| 2);
    let fill = w.saturating_sub(width(&title) + 1 + info_w + 2);
    let mut spans = vec![
        Span::styled(title, t.accent()),
        Span::styled(format!(" {} ", "─".repeat(fill)), t.line()),
    ];
    spans.extend(mark);
    spans.push(Span::styled(format!("{} ", g.info), t.muted()));
    Line::from(spans)
}

/// Shortcuts of the panel: the key in `moon` and the action in `ink`, so they
/// read at a glance: ` ↑↓ move · enter select · esc close`.
pub(super) fn keys_spans(t: &Theme, keys: &[(&str, &str)]) -> Vec<Span<'static>> {
    let mut spans = vec![Span::raw(" ")];
    for (i, (k, label)) in keys.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", t.muted()));
        }
        spans.push(Span::styled(k.to_string(), t.accent()));
        spans.push(Span::styled(format!(" {label}"), t.text()));
    }
    spans
}

/// Last row: the shortcuts and, if there is more than is shown, `a-b/n` on the
/// right.
pub(super) fn panel_footer(
    t: &Theme,
    keys: &[(&str, &str)],
    w: usize,
    total: usize,
    rows: usize,
    scroll: usize,
) -> Line<'static> {
    let mut footer = Line::from(keys_spans(t, keys));
    if rows == 0 || total <= rows {
        return footer;
    }
    let pos = format!("{}-{}/{} ", scroll + 1, (scroll + rows).min(total), total);
    // with no room for both, and a gap that keeps them apart, the shortcuts win
    match w.checked_sub(footer.width() + width(&pos)) {
        Some(pad) if pad >= 2 => {
            footer.spans.push(Span::styled(" ".repeat(pad), t.text()));
            footer.spans.push(Span::styled(pos, t.muted()));
        }
        _ => {}
    }
    footer
}

fn list_content(t: &Theme, p: &Picker, w: usize, rows: usize) -> Content {
    let all = p.rows();
    let mut start = match p.selected_row() {
        Some(r) if r >= rows => r + 1 - rows,
        _ => 0,
    };
    // on the last item what is left below is shown too (an empty section
    // of a provider that is down), without losing sight of the cursor
    if let Some(r) = p.selected_row() {
        if p.selected + 1 == p.len() {
            start = start.max(all.len().saturating_sub(rows).min(r));
        }
    }
    let mut body = Vec::new();
    if p.is_empty() {
        body.push(Line::from(Span::styled(
            format!(" {}", p.empty_text),
            t.muted(),
        )));
    }
    let room = rows.saturating_sub(usize::from(p.is_empty()));
    // the number belongs to the row, not to the screen: it is the same one
    // however the window scrolls, and every row has one
    let num_w = digits(p.len()) + 2;
    let mut n = 0;
    for (r, row) in all.iter().enumerate() {
        if let Row::Item(..) = row {
            n += 1;
        }
        if r < start || body.len() >= room + usize::from(p.is_empty()) {
            continue;
        }
        body.push(match row {
            Row::Blank => Line::from(""),
            Row::Header(g) => section_line(t, g, w),
            Row::Item(i, selected, _) => row_line(
                t,
                &i.label,
                &i.detail,
                RowState {
                    n: Some(n),
                    num_w,
                    selected: *selected,
                    active: i.active,
                    dim: i.dim,
                },
                w,
            ),
        });
    }
    Content {
        title: p.title.clone(),
        tabs: Vec::new(),
        info: p.title_info.clone(),
        hint: filter_line(t, p, w),
        body,
        total: all.len(),
        scroll: start,
    }
}

/// Columns a number takes: `9` one, `13` two.
fn digits(n: usize) -> usize {
    n.max(1).to_string().len()
}

/// What is being done with a session: the deletion, as two options walked with
/// the same cursor as any other list, or the new title being typed.
fn action_content(t: &Theme, action: &SessionAction, w: usize) -> Content {
    let (title, hint, body) = match action {
        SessionAction::Delete {
            title,
            choice,
            open,
            ..
        } => (
            "Delete session",
            Line::from(Span::styled(
                if *open {
                    format!(
                        " «{}» · the one you are in: what is on screen stops being saved",
                        truncate(title, w.saturating_sub(62))
                    )
                } else {
                    format!(
                        " «{}» · the saved conversation file is removed",
                        truncate(title, w.saturating_sub(46))
                    )
                },
                t.muted(),
            )),
            vec![
                row_line(
                    t,
                    "Delete",
                    "",
                    RowState {
                        selected: *choice == Choice::Delete,
                        ..RowState::default()
                    },
                    w,
                ),
                row_line(
                    t,
                    "Keep",
                    "",
                    RowState {
                        selected: *choice == Choice::Keep,
                        ..RowState::default()
                    },
                    w,
                ),
            ],
        ),
        SessionAction::Rename { input, .. } => (
            "Rename session",
            Line::from(Span::styled(" the new title", t.muted())),
            vec![Line::from(vec![
                Span::styled(" ❯ ", t.accent()),
                Span::styled(truncate(input, w.saturating_sub(6)), t.text()),
                Span::styled("█", t.accent()),
            ])],
        ),
    };
    let total = body.len();
    Content {
        title: title.to_string(),
        tabs: Vec::new(),
        info: String::new(),
        hint,
        body,
        total,
        scroll: 0,
    }
}

/// `/tools`: a question, what it means, and a few choices walked with the
/// cursor, each with its control on the right: `[✓]` for a checkbox, `◀ 8 ▶`
/// for a number. No `Continue` row: `Esc` applies whatever is set.
fn tools_content(cwd: &str, t: &Theme, d: &ToolsDialog, w: usize) -> Content {
    let body = tools_lines(t, d, w);
    let total = body.len();
    Content {
        title: "Let the model use files?".to_string(),
        tabs: Vec::new(),
        info: cwd.to_string(),
        hint: Line::from(Span::styled(
            " only under this directory · every edit is a diff you apply or skip · no shell",
            t.muted(),
        )),
        body,
        total,
        scroll: 0,
    }
}

/// One line per row of the tools panel, shown under them for the row the
/// cursor is on.
const TOOLS_ROW_HELP: [&str; 4] = [
    "the model can open and list files under this directory, and nothing more",
    "the model proposes a diff; nothing is written until you apply it",
    "the model proposes a new file; nothing is written until you apply it",
    "how many times the model may use a tool before it has to answer",
];

/// The body of the tools panel; its length is the panel's height.
pub(super) fn tools_lines(t: &Theme, d: &ToolsDialog, w: usize) -> Vec<Line<'static>> {
    let mut lines = help_text(
        t,
        "It gets read_file and list_dir; edit_file and write_file with the boxes below. It cannot run commands, delete or rename files, or reach anything above this directory.",
        w,
    );
    lines.push(Line::from(""));
    let check = |on: bool| if on { "[✓]" } else { "[ ]" };
    let rows: [(&str, String); 4] = [
        ("Read files", check(d.on).to_string()),
        ("Edit existing files", check(d.edit).to_string()),
        ("Create new files", check(d.create).to_string()),
        ("Max steps per message", format!("◀ {} ▶", d.rounds)),
    ];
    for (i, (label, control)) in rows.iter().enumerate() {
        let selected = d.row == i;
        // the number only matters with the tools on
        let dim = i == ToolsDialog::ROUNDS && !d.on;
        let label_style = if selected {
            t.soft_bold()
        } else if dim {
            t.muted()
        } else {
            t.text()
        };
        let control_w = width(control);
        let label = truncate(label, w.saturating_sub(control_w + 6).max(8));
        let pad = w.saturating_sub(3 + width(&label) + 1 + control_w + 1);
        lines.push(Line::from(vec![
            Span::styled(if selected { " ❯ " } else { "   " }, t.accent()),
            Span::styled(label, label_style),
            Span::styled(" ".repeat(pad + 1), t.text()),
            Span::styled(
                control.clone(),
                if dim {
                    t.muted()
                } else if selected {
                    t.soft()
                } else {
                    t.accent()
                },
            ),
        ]));
    }
    // what the row under the cursor means, one muted line that keeps its
    // place, so the panel does not change height as the cursor walks
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("   ", t.text()),
        Span::styled(
            truncate(
                TOOLS_ROW_HELP[d.row.min(TOOLS_ROW_HELP.len() - 1)],
                w.saturating_sub(4),
            ),
            t.muted(),
        ),
    ]));
    lines
}

/// An edit the model asked for: the file and the counts in the title, the
/// two choices on the second row as chips (the one the cursor is on painted
/// on `moon`, like an open tab), and the diff as the body, which scrolls.
/// Nothing is on disk until `Apply`.
fn approval_content(t: &Theme, a: &mut Approval, w: usize, rows: usize) -> Content {
    a.rows = rows;
    a.scroll_by(0);
    let chip = |label: &str, on: bool| {
        Span::styled(
            format!(" {label} "),
            if on { t.selected() } else { t.muted() },
        )
    };
    let hint = Line::from(vec![
        Span::styled(" ", t.text()),
        chip("Apply", a.choice == EditChoice::Apply),
        Span::styled(" ", t.text()),
        chip("Skip", a.choice == EditChoice::Skip),
        Span::styled(
            "  the model wants this change · nothing is written until you apply",
            t.muted(),
        ),
    ]);
    let lines = diff::lines(t, &a.edit.diff, w);
    let total = lines.len();
    Content {
        title: a.title(),
        tabs: Vec::new(),
        info: a.edit.counts(),
        hint,
        body: lines.into_iter().skip(a.scroll).take(rows).collect(),
        total,
        scroll: a.scroll,
    }
}

/// The help is read one section at a time: tab walks them and each one
/// scrolls on its own with ↑↓, PgUp/PgDn and the wheel. The window it was
/// painted in is left in the state so the scroll can be clamped.
fn help_content(app: &mut App, t: &Theme, w: usize, rows: usize) -> Content {
    let version = app.version.clone();
    let tab = match &app.panel {
        Some(Panel::Help(h)) => h.tab,
        _ => unreachable!("the help panel is open"),
    };
    let lines = help_lines(app, t, tab, w);
    let Some(Panel::Help(h)) = app.panel.as_mut() else {
        unreachable!("the help panel is open")
    };
    let total = lines.len();
    h.rows = rows;
    h.total = total;
    h.scroll_by(0);
    let scroll = h.scroll;
    let (info, hint) = match tab {
        HelpTab::General => (
            format!("moon v{version}"),
            "what it is and how to get going",
        ),
        HelpTab::Commands => (
            format!("{} commands", SPECS.len()),
            "at the start of the input; tab completes them",
        ),
        HelpTab::Keys => (
            format!("{} keys", KEYS.len()),
            "shortcuts of the box and of the conversation",
        ),
    };
    Content {
        title: "Help".to_string(),
        tabs: HelpTab::ALL
            .iter()
            .map(|x| (x.title(), *x == tab))
            .collect(),
        info,
        hint: Line::from(Span::styled(format!(" {hint}"), t.muted())),
        body: lines.into_iter().skip(scroll).take(rows).collect(),
        total,
        scroll,
    }
}

/// Two-column row for the help: key in `moon-soft` and description wrapped
/// to the right column, with a hanging indent on the continuations.
pub(super) fn help_row(
    t: &Theme,
    key: &str,
    desc: &str,
    key_w: usize,
    w: usize,
) -> Vec<Line<'static>> {
    let desc_w = w.saturating_sub(key_w + 1).max(10);
    let desc = Line::from(Span::styled(desc.to_string(), t.text()));
    wrap_line(&desc, desc_w)
        .into_iter()
        .enumerate()
        .map(|(i, part)| {
            let head = if i == 0 {
                format!(" {key}")
            } else {
                String::new()
            };
            let pad = " ".repeat(key_w.saturating_sub(width(&head)));
            let mut spans = vec![Span::styled(head, t.soft()), Span::styled(pad, t.text())];
            spans.extend(part.spans);
            Line::from(spans)
        })
        .collect()
}

/// Section header of the help: `Essentials ───────`. Unlike a list's, it
/// counts nothing: the count is next to the title, in the section's tab.
pub(super) fn help_section(t: &Theme, title: &str, w: usize) -> Line<'static> {
    let g = PickerGroup {
        title: title.to_string(),
        info: String::new(),
        mark: None,
    };
    section_line(t, &g, w)
}

/// A paragraph of the help: plain text wrapped to the panel, one space in.
fn help_text(t: &Theme, text: &str, w: usize) -> Vec<Line<'static>> {
    let line = Line::from(Span::styled(text.to_string(), t.text()));
    wrap_line(&line, w.saturating_sub(1))
        .into_iter()
        .map(|part| {
            let mut spans = vec![Span::styled(" ", t.text())];
            spans.extend(part.spans);
            Line::from(spans)
        })
        .collect()
}

/// What moon is, in the words of the README.
const ABOUT: &str = "A keyboard-first chat client for language models that run on your own machine: Ollama over its native API, and any OpenAI-compatible server next to it.";

/// The handful of keys and commands worth knowing before the full lists.
const BASICS: &[(&str, &str)] = &[
    ("/model", "switch model, or ctrl+p; the conversation stays"),
    ("/sessions", "resume a saved conversation, or ctrl+s"),
    (
        "/files",
        "what is attached and what it costs, or ctrl+f; the count sits in the status row",
    ),
    (
        "/tools",
        "let the model read, edit or create files under this directory, each write with your ok",
    ),
    (
        "@path",
        "attach a file to this message, or a range with @path:40-120",
    ),
    ("ctrl+j", "newline"),
    (
        "esc · ctrl+c",
        "cancel the generation · close the panel · clear the box (ctrl+c twice with an empty input quits)",
    ),
];

/// The `General` tab: what moon is, the basics, and where its files are.
fn general_lines(app: &App, t: &Theme, w: usize) -> Vec<Line<'static>> {
    let mut lines = help_text(t, ABOUT, w);
    lines.push(Line::from(""));
    lines.push(help_section(t, "Essentials", w));
    lines.push(Line::from(""));
    let key_w = BASICS.iter().map(|(k, _)| width(k)).max().unwrap_or(10) + 3;
    for (k, d) in BASICS {
        lines.extend(help_row(t, k, d, key_w, w));
    }
    lines.push(Line::from(""));
    lines.push(help_section(t, "Files", w));
    lines.push(Line::from(""));
    let config = app.config_note();
    let files: [(&str, &str); 4] = [
        (
            "MOON.md",
            "read into every conversation; /context shows what the model is actually sent",
        ),
        ("Sessions", "saved automatically, as JSONL"),
        ("Config", &config),
        (
            "Updates",
            "`moon update` brings in the latest release from GitHub; with update_check = true moon says at startup when there is one",
        ),
    ];
    let key_w = files.iter().map(|(k, _)| width(k)).max().unwrap_or(10) + 3;
    for (k, d) in files {
        lines.extend(help_row(t, k, d, key_w, w));
    }
    lines
}

/// The `Commands` tab: every slash command with its arguments.
fn commands_lines(t: &Theme, w: usize) -> Vec<Line<'static>> {
    let name_w = command_column_width();
    SPECS
        .iter()
        .flat_map(|s| {
            let head = if s.args.is_empty() {
                format!("/{}", s.name)
            } else {
                format!("/{} {}", s.name, s.args)
            };
            help_row(t, &head, s.help, name_w, w)
        })
        .collect()
}

/// The `Keys` tab: every shortcut.
fn keys_lines(t: &Theme, w: usize) -> Vec<Line<'static>> {
    let key_w = KEYS.iter().map(|(k, _)| width(k)).max().unwrap_or(10) + 3;
    KEYS.iter()
        .flat_map(|(k, d)| help_row(t, k, d, key_w, w))
        .collect()
}

pub(super) fn help_lines(app: &App, t: &Theme, tab: HelpTab, w: usize) -> Vec<Line<'static>> {
    match tab {
        HelpTab::General => general_lines(app, t, w),
        HelpTab::Commands => commands_lines(t, w),
        HelpTab::Keys => keys_lines(t, w),
    }
}
