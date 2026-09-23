//! A diff painted with the theme: old and new line numbers, then `+` in
//! `ok` or `-` in `alert`, then the line. A gap between hunks is a `···`
//! row in `night-line`.

use moon_agent::{Diff, DiffKind};

use super::*;

pub(super) fn lines(t: &Theme, d: &Diff, w: usize) -> Vec<Line<'static>> {
    let max = d
        .lines
        .iter()
        .filter_map(|l| l.old.max(l.new))
        .max()
        .unwrap_or(1);
    let nw = max.to_string().len();
    let num = |n: Option<usize>| match n {
        Some(n) => format!("{n:>nw$}"),
        None => " ".repeat(nw),
    };
    d.lines
        .iter()
        .map(|l| {
            if l.kind == DiffKind::Gap {
                return Line::from(Span::styled(
                    format!(" {} ···", " ".repeat(nw * 2 + 1)),
                    t.line(),
                ));
            }
            let (sign, style) = match l.kind {
                DiffKind::Added => ("+", t.ok()),
                DiffKind::Removed => ("-", t.alert()),
                _ => (" ", t.muted()),
            };
            let head = format!(" {} {} ", num(l.old), num(l.new));
            let text = l.text.replace('\t', "    ");
            let text = truncate(&text, w.saturating_sub(width(&head) + 2));
            Line::from(vec![
                Span::styled(head, t.muted()),
                Span::styled(format!("{sign} "), style),
                Span::styled(text, style),
            ])
        })
        .collect()
}
