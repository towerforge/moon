//! The machine drawn: cpu and ram over the window `sysmon` keeps, filled in
//! braille (2×4 dots per cell) from the curve down. The panel splits down
//! the middle: cpu on the left, ram on the right. It is the only panel that
//! is not made of lines: `Chart` wants an area of its own, so the chrome is
//! drawn around it and the plots go in the middle.

use ratatui::style::Style;
use ratatui::symbols::Marker;
use ratatui::widgets::{Axis, Chart, Dataset, GraphType};

use super::*;
use crate::app::LoadedState;
use crate::sysmon::{fmt_gib, WINDOW};

/// Rows a plot takes. Side by side they share one heading row, so each can
/// be taller than when they were stacked.
const PLOT_ROWS: u16 = 6;
/// Columns the gutter between the two halves takes, separator included.
const GUTTER: u16 = 3;
/// Rows the panel draws: the title, the shared heading row, the plots, a
/// blank one, the row of totals and the footer.
const ROWS: usize = 1 + 1 + PLOT_ROWS as usize + 3;
/// What it asks `panel::height` for, which adds the chrome back on top.
pub(super) const BODY: usize = ROWS - panel::CHROME as usize;

/// `cpu 42%` and, to the right, the peak and whatever else there is to say.
fn heading(t: &Theme, label: &str, now: f32, peak: f32, right: &str, w: usize) -> Line<'static> {
    let pct = format!("{now:.0}%");
    let peak = format!("▲{peak:.0}%");
    let left = format!("  {label} ");
    let pad = w
        .saturating_sub(width(&left) + width(&pct) + width(&peak) + width(right) + 4)
        .max(1);
    let mut spans = vec![
        Span::styled(left, t.muted()),
        Span::styled(format!("{pct:>4}"), t.text()),
        Span::styled("  ", t.muted()),
        Span::styled(peak, t.muted()),
    ];
    if !right.is_empty() {
        spans.push(Span::raw(" ".repeat(pad)));
        spans.push(Span::styled(right.to_string(), t.muted()));
    }
    Line::from(spans)
}

/// One plot: the curve and everything under it, no axes or legend. The `x`
/// bounds are the whole window, so the trace grows from the right as samples
/// pile up instead of stretching to fill.
fn plot(frame: &mut Frame, area: Rect, points: &[(f64, f64)], style: Style) {
    if area.height == 0 {
        return;
    }
    let datasets = vec![Dataset::default()
        .marker(Marker::Braille)
        .graph_type(GraphType::Area)
        .style(style)
        .data(points)];
    let chart = Chart::new(datasets)
        .x_axis(Axis::default().bounds([-(WINDOW.as_secs_f64()), 0.0]))
        .y_axis(Axis::default().bounds([0.0, 100.0]));
    frame.render_widget(chart, area);
}

/// The whole panel: title, the two plots with their headings, and the row of
/// totals. Draws nothing but the title while there is no sample yet.
pub(super) fn render(app: &App, frame: &mut Frame, area: Rect) {
    let t = &app.theme;
    let w = area.width as usize;
    let n = app.sys.len();
    let info = format!(
        "{} · {n} {}",
        fmt_span(app.sys.span()),
        if n == 1 { "sample" } else { "samples" }
    );
    let title = panel::head_line(t, "Machine", &[], &info, w);
    let keys = app.panel_keys();
    let Some(s) = app.sys.current().copied() else {
        let lines = vec![
            title,
            Line::from(""),
            Line::styled("  waiting for the first reading…", t.muted()),
        ];
        frame.render_widget(Paragraph::new(lines), area);
        return;
    };

    // chrome first, so the plots can be laid over the rows left in between
    let plot_h = area.height.saturating_sub(4).min(PLOT_ROWS);
    let [title_a, head_a, plots_a, rest] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(plot_h),
        Constraint::Min(0),
    ])
    .areas(area);
    // cpu on the left, ram on the right, with the gutter in between
    let halves = |r: Rect| -> [Rect; 3] {
        Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Length(GUTTER),
            Constraint::Fill(1),
        ])
        .areas(r)
    };
    let [cpu_head, gutter_head, ram_head] = halves(head_a);
    let [cpu_plot, gutter_plot, ram_plot] = halves(plots_a);
    let half_w = cpu_head.width as usize;

    // the model in memory, the same reading the status line shows
    let model = match &app.loaded {
        LoadedState::Loaded(m) => {
            let cpu = m.cpu_percent();
            let on_cpu = if cpu > 0 {
                format!(" · {cpu}% on cpu")
            } else {
                String::new()
            };
            format!("{} loaded{on_cpu}", fmt_gib(m.size_bytes) + "G")
        }
        _ => String::new(),
    };
    let swap = if s.swap_used > 0 {
        format!("swap {}G", fmt_gib(s.swap_used))
    } else {
        "no swap".to_string()
    };

    frame.render_widget(Paragraph::new(title), title_a);
    frame.render_widget(
        Paragraph::new(heading(t, "cpu", s.cpu, app.sys.peak_cpu(), "", half_w)),
        cpu_head,
    );
    frame.render_widget(
        Paragraph::new(heading(
            t,
            "ram",
            s.ram,
            app.sys.peak_ram(),
            &format!("{} / {}G", fmt_gib(s.ram_used), fmt_gib(s.ram_total)),
            half_w,
        )),
        ram_head,
    );
    // the rule that tells the two halves apart, from the heading down
    let rule: Vec<Line> = (0..(1 + plot_h))
        .map(|_| Line::styled(" │ ", t.line()))
        .collect();
    frame.render_widget(
        Paragraph::new(rule),
        Rect {
            height: gutter_head.height + gutter_plot.height,
            ..gutter_head
        },
    );

    let inner = |r: Rect| Rect {
        x: r.x + 2,
        width: r.width.saturating_sub(2),
        ..r
    };
    let cpu = app.sys.series(|s| s.cpu);
    let ram = app.sys.series(|s| s.ram);
    plot(frame, inner(cpu_plot), &cpu, t.accent());
    plot(frame, inner(ram_plot), &ram, t.soft());

    // the footer always sits on the last row, as in every other panel
    let mut tail = vec![Line::from("")];
    if rest.height > 1 {
        tail.push(Line::from(vec![
            Span::styled("  ", t.muted()),
            Span::styled(swap, t.muted()),
            Span::styled(if model.is_empty() { "" } else { " · " }, t.muted()),
            Span::styled(model, t.soft()),
        ]));
    }
    tail.resize(rest.height.saturating_sub(1) as usize, Line::from(""));
    tail.push(panel::panel_footer(t, &keys, w, 0, 1, 0));
    frame.render_widget(Paragraph::new(tail), rest);
}

/// `3 min`, `45 s`: the span the samples cover, rounded the way a person reads
/// it.
fn fmt_span(secs: f64) -> String {
    if secs >= 60.0 {
        format!("{:.0} min", secs / 60.0)
    } else {
        format!("{secs:.0} s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_span_reads_in_minutes_from_a_minute_on() {
        assert_eq!(fmt_span(0.0), "0 s");
        assert_eq!(fmt_span(45.0), "45 s");
        assert_eq!(fmt_span(60.0), "1 min");
        assert_eq!(fmt_span(180.0), "3 min");
    }
}
