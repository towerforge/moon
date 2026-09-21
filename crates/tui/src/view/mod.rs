//! Rendering of the state: the zones stacked from the conversation down to the
//! hints, and the panel that takes the bottom over when one is open. No logic.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};
use ratatui::Frame;

use crate::app::{App, Choice, HelpState, HelpTab, Panel, SessionAction};
use crate::commands::{KEYS, SPECS};
use crate::picker::{Picker, PickerGroup, Row};
use crate::theme::Theme;
use crate::wrap::{truncate, width, wrap_line};

mod machine;
mod panel;
#[cfg(test)]
mod tests;

/// Under this height every row counts: the conversation keeps the blank one
/// it would otherwise give to the status row.
const GAP_MIN_HEIGHT: u16 = 12;

pub fn view(app: &mut App, frame: &mut Frame) {
    let area = frame.area();
    let t = app.theme.clone();
    if area.height < 6 || area.width < 24 {
        frame.render_widget(Paragraph::new("terminal too small").style(t.muted()), area);
        return;
    }
    // an open panel takes the whole bottom: the box and the suggestions
    // step aside until it closes
    let panel_h = panel::height(app, area);
    let input_h = app.input.height(area.width);
    let suggest_h = suggest_height(app);
    let bottom_h = if panel_h > 0 {
        1 + panel_h
    } else {
        suggest_h + input_h + 3
    };
    // a blank row between the conversation and the status: the reply, the
    // question being answered and the `✓ done` that closes it were touching.
    // On a cramped terminal the row goes back to the conversation
    let gap = u16::from(area.height >= GAP_MIN_HEIGHT);
    let [conv_area, _gap, status_area, bottom] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(gap),
        Constraint::Length(1),
        Constraint::Length(bottom_h),
    ])
    .areas(area);

    let lines = app.visible_lines(conv_area.width, conv_area.height);
    frame.render_widget(Paragraph::new(Text::from(lines)), conv_area);
    app.conv_area = Some(conv_area);
    render_selection(app, frame, conv_area);
    app.jump_rect = None;
    if !app.follow && app.lines_below() > 0 && conv_area.height > 1 {
        render_jump_to_bottom(app, frame, conv_area);
    }
    render_status(app, frame, status_area);

    // the separator over the zone that has the focus goes in `moon-soft`
    let rule = |style| Line::from(Span::styled("─".repeat(area.width as usize), style));

    if panel_h > 0 {
        let [sep, panel_area] =
            Layout::vertical([Constraint::Length(1), Constraint::Length(panel_h)]).areas(bottom);
        frame.render_widget(Paragraph::new(rule(t.soft())), sep);
        panel::render(app, frame, panel_area);
        app.input_area = None;
        return;
    }

    let [suggest_area, sep1, input_area, sep2, hints_area] = Layout::vertical([
        Constraint::Length(suggest_h),
        Constraint::Length(1),
        Constraint::Length(input_h),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(bottom);

    render_suggest(app, frame, suggest_area);
    frame.render_widget(Paragraph::new(rule(t.soft())), sep1);
    frame.render_widget(Paragraph::new(rule(t.soft())), sep2);

    let cmd_chars = app.command_span();
    let cursor = app
        .input
        .render(input_area, frame.buffer_mut(), &t, cmd_chars);
    app.input_area = Some(input_area);
    render_hints(app, frame, hints_area);

    // the terminal cursor stays hidden; moon draws the `moon` block itself
    let (x, y) = cursor;
    if x < area.right() && y < area.bottom() {
        frame.buffer_mut()[(x, y)].set_style(t.selected());
    }
}

/// The commands matching what is typed, right over the box. They are drawn
/// like the bottom panel: a title, no surface of their own, the cursor on the
/// left and the bar on the right when they do not all fit.
const SUGGEST_MAX_ROWS: usize = 6;

fn suggest_height(app: &App) -> u16 {
    // one row for the title, as in the panels
    app.suggestions()
        .map_or(0, |(specs, _)| specs.len().min(SUGGEST_MAX_ROWS) as u16 + 1)
}

/// Width of the `/name args` column in the help and in the suggestions.
fn command_column_width() -> usize {
    SPECS
        .iter()
        .map(|s| width(s.name) + 1 + width(s.args))
        .max()
        .unwrap_or(10)
        + 3
}

fn render_suggest(app: &App, frame: &mut Frame, area: Rect) {
    if area.height < 2 {
        return;
    }
    let Some((specs, sel)) = app.suggestions() else {
        return;
    };
    let t = &app.theme;
    // what is already typed, to set it apart in every name
    let typed = app.typing_command().unwrap_or_default().chars().count();
    let rows = area.height as usize - 1;
    let start = sel.saturating_sub(rows - 1);
    let name_w = command_column_width();
    // the last column is the bar's, with a blank one before it
    let w = area.width as usize;
    let body_w = w.saturating_sub(2);
    let n = specs.len();
    let info = format!("{n} {}", if n == 1 { "command" } else { "commands" });
    let mut lines = vec![panel::head_line(t, "Commands", &[], &info, w)];
    lines.extend(
        specs
            .iter()
            .enumerate()
            .skip(start)
            .take(rows)
            .map(|(i, s)| {
                let selected = i == sel;
                // the tail of the name is what completing would add: under
                // the cursor it goes in moon-soft, on the other rows it is
                // plain text, like the help
                let (name_st, help_st) = if selected {
                    (t.soft(), t.text())
                } else {
                    (t.muted(), t.muted())
                };
                let head_w = width(s.name)
                    + 1
                    + if s.args.is_empty() {
                        0
                    } else {
                        width(s.args) + 1
                    };
                let pad = name_w.saturating_sub(head_w);
                // `/` plus the characters already typed, in moon and bold
                // as in the box: the rest is what completing would add
                let cut = s
                    .name
                    .char_indices()
                    .nth(typed)
                    .map_or(s.name.len(), |(i, _)| i);
                let (head, tail) = s.name.split_at(cut);
                let mut spans = vec![
                    Span::styled(if selected { " ❯ " } else { "   " }, t.accent()),
                    Span::styled(format!("/{head}"), t.accent_bold()),
                    Span::styled(tail, name_st),
                ];
                if !s.args.is_empty() {
                    spans.push(Span::styled(format!(" {}", s.args), t.soft()));
                }
                spans.push(Span::raw(" ".repeat(pad)));
                spans.push(Span::styled(
                    truncate(s.help, body_w.saturating_sub(name_w + 4)),
                    help_st,
                ));
                Line::from(spans)
            }),
    );
    frame.render_widget(Paragraph::new(lines), area);
    let body = Rect {
        y: area.y + 1,
        height: rows as u16,
        ..area
    };
    panel::render_scrollbar(frame, t, body, n, rows, start);
}

/// Highlights the mouse selection: full rows in the middle, clipped at the
/// ends, in `moon` / `on-moon`.
fn render_selection(app: &App, frame: &mut Frame, conv: Rect) {
    let Some(sel) = app.selection else { return };
    let ((r1, c1), (r2, c2)) = sel.bounds();
    let style = app.theme.selected();
    let buf = frame.buffer_mut();
    for y in conv.y..conv.bottom() {
        let row = app.scroll_offset + (y - conv.y) as usize;
        if row < r1 || row > r2 {
            continue;
        }
        let from = if row == r1 { c1 } else { 0 };
        let to = if row == r2 {
            c2
        } else {
            conv.width.saturating_sub(1) as usize
        };
        for col in from..=to.min(conv.width.saturating_sub(1) as usize) {
            buf[(conv.x + col as u16, y)].set_style(style);
        }
    }
}

/// Chip centered over the last row of the conversation when the user has
/// scrolled up: there is more text below. A click follows it.
fn render_jump_to_bottom(app: &mut App, frame: &mut Frame, conv: Rect) {
    let t = &app.theme;
    let text = if app.is_streaming() {
        " ↓ Reply in progress · jump to bottom "
    } else {
        " ↓ Jump to bottom "
    };
    let w = (width(text) as u16).min(conv.width);
    let rect = Rect {
        x: conv.x + (conv.width - w) / 2,
        y: conv.bottom() - 1,
        width: w,
        height: 1,
    };
    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(text, t.raised_accent()))),
        rect,
    );
    app.jump_rect = Some(rect);
}

fn render_status(app: &App, frame: &mut Frame, area: Rect) {
    let t = &app.theme;
    let right = app.status_spans();
    let right_w: usize = right.iter().map(|s| width(&s.content)).sum();
    let avail = area.width as usize;
    // on the left, the notice if there is one; otherwise, the activity in progress
    let left: Vec<Span<'static>> = match (&app.notice, app.activity_spans()) {
        (Some((notice, _)), _) => {
            let style = if notice.starts_with('✗') {
                t.error()
            } else {
                t.muted()
            };
            vec![Span::styled(notice.replace('\n', " "), style)]
        }
        (None, Some(activity)) => activity,
        (None, None) => Vec::new(),
    };
    let left = truncate_spans(left, avail.saturating_sub(right_w + 3));
    let left_w: usize = left.iter().map(|s| width(&s.content)).sum();
    let mut spans = vec![Span::raw(" ")];
    spans.extend(left);
    spans.push(Span::raw(
        " ".repeat(avail.saturating_sub(left_w + right_w + 2)),
    ));
    spans.extend(right);
    spans.push(Span::raw(" "));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Bottom row: the hints on the left; on the right, the active model and,
/// if they fit, the machine readings.
fn render_hints(app: &App, frame: &mut Frame, area: Rect) {
    let t = &app.theme;
    let mut right = app.model_spans();
    let stats = app.stats_spans(area.width);
    if !stats.is_empty() {
        if !right.is_empty() {
            right.push(Span::styled(" · ", t.muted()));
        }
        right.extend(stats);
    }
    let right_w: usize = right.iter().map(|s| width(&s.content)).sum();
    let avail = area.width as usize;
    let left = truncate_spans(
        vec![Span::styled(app.hints(), t.muted())],
        avail.saturating_sub(right_w + 2),
    );
    let left_w: usize = left.iter().map(|s| width(&s.content)).sum();
    let mut spans = left;
    spans.push(Span::raw(
        " ".repeat(avail.saturating_sub(left_w + right_w + 1)),
    ));
    spans.extend(right);
    spans.push(Span::raw(" "));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Truncates a list of spans to `max` columns, with `…` at the end.
fn truncate_spans(spans: Vec<Span<'static>>, max: usize) -> Vec<Span<'static>> {
    if max < 4 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut used = 0;
    for span in spans {
        let w = width(&span.content);
        if used + w <= max {
            used += w;
            out.push(span);
        } else {
            let text = truncate(&span.content, max - used);
            out.push(Span::styled(text, span.style));
            break;
        }
    }
    out
}
