//! The machine drawn: cpu, ram and, on a machine with a card of its own,
//! gpu memory over the window `sysmon` keeps, filled in braille (2×4 dots
//! per cell) from the curve down. The panel splits in columns, cpu first,
//! with a rule between them. Under the ram curve, in `ink-muted`, the share
//! of it that is the loaded model. It is the only panel that is not made of
//! lines: `Chart` wants an area of its own, so the chrome is drawn around it
//! and the plots go in the middle.

use ratatui::style::Style;
use ratatui::symbols::Marker;
use ratatui::widgets::{Axis, Chart, Dataset, GraphType};

use super::*;
use crate::app::LoadedState;
use crate::sysmon::{fmt_gib, WINDOW};

/// Rows a plot takes. Side by side they share one heading row, so each can
/// be taller than when they were stacked.
const PLOT_ROWS: u16 = 6;
/// Columns the gutter between two plots takes, separator included.
const GUTTER: u16 = 3;
/// Rows the panel draws: the title, the shared heading row, the plots, a
/// blank one, the row of totals and the footer.
const ROWS: usize = 1 + 1 + PLOT_ROWS as usize + 3;
/// What it asks `panel::height` for, which adds the chrome back on top.
pub(super) const BODY: usize = ROWS - panel::CHROME as usize;

/// `cpu 42%` and, to the right, the peak and whatever else there is to say.
/// The second value says whether `right` fit: when it did not, it is left
/// out and the caller finds it another place.
fn heading(
    t: &Theme,
    label: &str,
    now: f32,
    peak: f32,
    right: &str,
    w: usize,
) -> (Line<'static>, bool) {
    let pct = format!("{now:.0}%");
    let pct = format!("{pct:>4}");
    let peak = format!("▲{peak:.0}%");
    let left = format!("  {label} ");
    let needed = width(&left) + width(&pct) + width(&peak) + 2;
    let mut spans = vec![
        Span::styled(left, t.muted()),
        Span::styled(pct, t.text()),
        Span::styled("  ", t.muted()),
        Span::styled(peak, t.muted()),
    ];
    // a space at least before it, and one column left free at the edge
    let fits = !right.is_empty() && needed + width(right) + 1 < w;
    if fits {
        let pad = w - needed - width(right) - 1;
        spans.push(Span::raw(" ".repeat(pad)));
        spans.push(Span::styled(right.to_string(), t.muted()));
    }
    (Line::from(spans), fits || right.is_empty())
}

/// One plot: the curve and everything under it, no axes or legend. The `x`
/// bounds are the whole window, so the trace grows from the right as samples
/// pile up instead of stretching to fill. A second series, if any, is drawn
/// on top: a band along the floor, for what part of the first one it is.
fn plot(
    frame: &mut Frame,
    area: Rect,
    points: &[(f64, f64)],
    style: Style,
    band: Option<(&[(f64, f64)], Style)>,
) {
    if area.height == 0 {
        return;
    }
    let series = |data, style| {
        Dataset::default()
            .marker(Marker::Braille)
            .graph_type(GraphType::Area)
            .style(style)
            .data(data)
    };
    let mut datasets = vec![series(points, style)];
    if let Some((points, style)) = band {
        datasets.push(series(points, style));
    }
    let chart = Chart::new(datasets)
        .x_axis(Axis::default().bounds([-(WINDOW.as_secs_f64()), 0.0]))
        .y_axis(Axis::default().bounds([0.0, 100.0]));
    frame.render_widget(chart, area);
}

/// `n` plots side by side with a gutter between each two: the areas of the
/// plots, then those of the gutters.
fn columns(r: Rect, n: usize) -> (Vec<Rect>, Vec<Rect>) {
    let mut constraints = Vec::with_capacity(2 * n);
    for i in 0..n {
        if i > 0 {
            constraints.push(Constraint::Length(GUTTER));
        }
        constraints.push(Constraint::Fill(1));
    }
    let areas = Layout::horizontal(constraints).split(r);
    let plots = areas.iter().step_by(2).copied().collect();
    let gutters = areas.iter().skip(1).step_by(2).copied().collect();
    (plots, gutters)
}

/// The whole panel: title, the plots with their headings, and the row of
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
    let has_gpu = s.gpu_total > 0;
    let cols = if has_gpu { 3 } else { 2 };

    // chrome first, so the plots can be laid over the rows left in between
    let plot_h = area.height.saturating_sub(4).min(PLOT_ROWS);
    let [title_a, head_a, plots_a, rest] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(plot_h),
        Constraint::Min(0),
    ])
    .areas(area);
    let (heads, head_gutters) = columns(head_a, cols);
    let (plots, plot_gutters) = columns(plots_a, cols);
    let col_w = heads[0].width as usize;

    // the totals go to the right of each heading when they fit; if not, to
    // the row under the plots, with the swap and the model
    let mut totals: Vec<String> = Vec::new();
    let ram_total = format!("{} / {}G", fmt_gib(s.ram_used), fmt_gib(s.ram_total));
    let (ram_head, fit) = heading(t, "ram", s.ram, app.sys.peak_ram(), &ram_total, col_w);
    if !fit {
        totals.push(format!("ram {ram_total}"));
    }
    let gpu_head = has_gpu.then(|| {
        let gpu_total = format!("{} / {}G", fmt_gib(s.gpu_used), fmt_gib(s.gpu_total));
        let (line, fit) = heading(t, "gpu", s.gpu, app.sys.peak_gpu(), &gpu_total, col_w);
        if !fit {
            totals.push(format!("gpu {gpu_total}"));
        }
        line
    });
    totals.push(if s.swap_used > 0 {
        format!("swap {}G", fmt_gib(s.swap_used))
    } else {
        "no swap".to_string()
    });
    // the model in memory, the same reading the status line shows; the mark
    // in front says its share is the band under the ram curve
    let model = match &app.loaded {
        LoadedState::Loaded(m) => {
            let cpu = m.cpu_percent();
            let on_cpu = if cpu > 0 {
                format!(" · {cpu}% on cpu")
            } else {
                String::new()
            };
            let mark = if s.model_ram > 0 { "▮ " } else { "" };
            format!("{mark}{}G loaded{on_cpu}", fmt_gib(m.size_bytes))
        }
        _ => String::new(),
    };

    frame.render_widget(Paragraph::new(title), title_a);
    let (cpu_head, _) = heading(t, "cpu", s.cpu, app.sys.peak_cpu(), "", col_w);
    let mut head_lines = vec![cpu_head, ram_head];
    head_lines.extend(gpu_head);
    for (line, area) in head_lines.into_iter().zip(&heads) {
        frame.render_widget(Paragraph::new(line), *area);
    }
    // the rules that tell the columns apart, from the heading down
    for (head, plot) in head_gutters.iter().zip(&plot_gutters) {
        let rule: Vec<Line> = (0..(1 + plot_h))
            .map(|_| Line::styled(" │ ", t.line()))
            .collect();
        frame.render_widget(
            Paragraph::new(rule),
            Rect {
                height: head.height + plot.height,
                ..*head
            },
        );
    }

    let inner = |r: Rect| Rect {
        x: r.x + 2,
        width: r.width.saturating_sub(2),
        ..r
    };
    let cpu = app.sys.series(|s| s.cpu);
    let ram = app.sys.series(|s| s.ram);
    let model_band = app.sys.series(|s| {
        if s.ram_total > 0 {
            (s.model_ram as f64 / s.ram_total as f64 * 100.0) as f32
        } else {
            0.0
        }
    });
    let band = (s.model_ram > 0).then_some((model_band.as_slice(), t.muted()));
    plot(frame, inner(plots[0]), &cpu, t.accent(), None);
    plot(frame, inner(plots[1]), &ram, t.soft(), band);
    if has_gpu {
        let gpu = app.sys.series(|s| s.gpu);
        plot(frame, inner(plots[2]), &gpu, t.text(), None);
    }

    // the footer always sits on the last row, as in every other panel
    let mut tail = vec![Line::from("")];
    if rest.height > 1 {
        tail.push(Line::from(vec![
            Span::styled("  ", t.muted()),
            Span::styled(totals.join(" · "), t.muted()),
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

    #[test]
    fn the_heading_drops_the_totals_when_they_do_not_fit() {
        let t = Theme::resolve(&Default::default()).0;
        let (line, fit) = heading(&t, "ram", 56.0, 75.0, "18.0 / 32.0G", 37);
        assert!(fit);
        let line = line.to_string();
        assert!(line.starts_with("  ram  56%  ▲75%  "), "{line}");
        assert!(line.ends_with("18.0 / 32.0G"), "{line}");
        // pushed to the right, one column short of the edge
        assert_eq!(width(&line), 36);
        let (line, fit) = heading(&t, "ram", 56.0, 75.0, "18.0 / 32.0G", 24);
        assert!(!fit);
        assert_eq!(line.to_string(), "  ram  56%  ▲75%");
        // nothing to say on the right: nothing missing either
        let (_, fit) = heading(&t, "cpu", 56.0, 75.0, "", 10);
        assert!(fit);
    }

    #[test]
    fn columns_alternate_plots_and_gutters() {
        let (plots, gutters) = columns(Rect::new(0, 0, 78, 6), 3);
        assert_eq!(plots.len(), 3);
        assert_eq!(gutters.len(), 2);
        assert!(plots.iter().all(|p| p.width >= 24));
        assert!(gutters.iter().all(|g| g.width == GUTTER));
        assert_eq!(plots[0].x, 0);
        assert!(gutters[0].x > plots[0].x && plots[1].x > gutters[0].x);
        let (plots, gutters) = columns(Rect::new(0, 0, 78, 6), 2);
        assert_eq!((plots.len(), gutters.len()), (2, 1));
    }
}
