//! A diff painted with the theme, in the approval panel and under the step
//! of an edit once it is applied: the line number — in the file as it is for
//! a removed line, as it will be for the rest — then `-` or `+`, then the
//! line colored by its kind of file. A removed row goes on `alert-bg` and an
//! added one on `ok-bg`, the whole width; a gap between hunks is a `...`
//! row.

use moon_agent::{Diff, DiffKind};
use ratatui::style::Style;

use super::*;
use crate::markdown::Renderer;

/// The diff of `path`, `w` columns wide.
pub(crate) fn lines(
    t: &Theme,
    md: &Renderer,
    path: &str,
    d: &Diff,
    w: usize,
) -> Vec<Line<'static>> {
    let max = d
        .lines
        .iter()
        .filter_map(|l| l.old.max(l.new))
        .max()
        .unwrap_or(1);
    let nw = max.to_string().len();
    let texts: Vec<String> = d
        .lines
        .iter()
        .map(|l| l.text.replace('\t', "    "))
        .collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    let colored = md.code_lines(path, &refs, t);
    d.lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            if l.kind == DiffKind::Gap {
                return Line::from(Span::styled(format!(" {}...", " ".repeat(nw)), t.muted()));
            }
            let (num, sign, row, mark) = match l.kind {
                DiffKind::Added => (l.new, "+", t.added(), t.added().fg(t.ok)),
                DiffKind::Removed => (l.old, "-", t.removed(), t.removed().fg(t.alert)),
                _ => (l.new, " ", t.text(), t.muted()),
            };
            let num = num.map_or_else(|| " ".repeat(nw), |n| format!("{n:>nw$}"));
            let head = format!(" {num} {sign} ");
            let pieces = match &colored {
                Some(c) => c[i].clone(),
                None => vec![(texts[i].clone(), t.ink)],
            };
            let mut spans = vec![Span::styled(head.clone(), mark)];
            spans.extend(cut(pieces, w.saturating_sub(width(&head)), row));
            let used: usize = spans.iter().map(|s| width(&s.content)).sum();
            // the colored rows reach the right edge
            if l.kind != DiffKind::Context && used < w {
                spans.push(Span::styled(" ".repeat(w - used), row));
            }
            Line::from(spans)
        })
        .collect()
}

/// The colored pieces of a line, cut to `max` columns, each one on the
/// row's background.
fn cut(pieces: Vec<(String, ratatui::style::Color)>, max: usize, row: Style) -> Vec<Span<'static>> {
    let total: usize = pieces.iter().map(|(p, _)| width(p)).sum();
    let mut left = max;
    let mut out = Vec::new();
    for (text, fg) in pieces {
        if left == 0 {
            break;
        }
        let text = if total > max && width(&text) >= left {
            truncate(&text, left)
        } else {
            text
        };
        left = left.saturating_sub(width(&text));
        out.push(Span::styled(text, row.fg(fg)));
    }
    out
}
