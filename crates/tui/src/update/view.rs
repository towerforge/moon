//! The update panel: one box, the moon on the left with what is about to
//! happen, the release notes on the right, and a line at the bottom that says
//! where the update is.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Widget};
use unicode_width::UnicodeWidthStr;

use super::{Phase, State, Step};
use crate::app::SPINNER;
use crate::logo::logo_rows;
use crate::theme::Theme;

/// Widest the box goes; on a narrower terminal it takes what there is.
pub const MAX_WIDTH: u16 = 76;
/// Rows of the box, borders included.
pub const HEIGHT: u16 = 11;
/// What the inline viewport takes: the box and a blank line above it.
pub const VIEWPORT: u16 = HEIGHT + 1;
/// Below this the notes column is dropped and only the left one is drawn.
const TWO_COLUMN_MIN: u16 = 64;
/// Columns of the left one, the moon included.
const LEFT_WIDTH: u16 = 35;
const BAR_WIDTH: usize = 22;
const NOTES_MAX: usize = 3;

/// Square corners with dashed sides: the Moon box, one step quieter.
const DASHED: border::Set = border::Set {
    top_left: "┌",
    top_right: "┐",
    bottom_left: "└",
    bottom_right: "┘",
    vertical_left: "┆",
    vertical_right: "┆",
    horizontal_top: "┄",
    horizontal_bottom: "┄",
};

pub fn render(state: &State, theme: &Theme, area: Rect, buf: &mut Buffer) {
    if area.width < 24 || area.height < 3 {
        return;
    }
    let w = area.width.min(MAX_WIDTH);
    let h = area.height.min(HEIGHT);
    // the box sits at the bottom of the viewport: the blank line is above it
    let outer = Rect::new(area.x, area.y + area.height.saturating_sub(h), w, h);
    let block = Block::bordered()
        .border_set(DASHED)
        .border_style(theme.line())
        .title(Line::from(vec![
            Span::styled("┄ ", theme.line()),
            Span::styled("moon update", theme.accent_bold()),
            Span::styled(" ┄", theme.line()),
        ]));
    let inner = block.inner(outer);
    block.render(outer, buf);
    if inner.height < 6 {
        return;
    }

    let two_columns = inner.width >= TWO_COLUMN_MIN;
    let left_w = if two_columns {
        LEFT_WIDTH
    } else {
        inner.width.saturating_sub(2)
    };
    // rows: blank · four of moon and facts · blank · state · blank · keys
    let head = Rect::new(inner.x + 1, inner.y + 1, left_w, 4);
    head_column(state, theme, head, buf);
    if two_columns {
        let x = inner.x + left_w + 2;
        let sep = Rect::new(x, inner.y + 1, 1, 4);
        for y in sep.y..sep.y + sep.height {
            buf[(sep.x, y)].set_symbol("┆").set_style(theme.line());
        }
        let notes = Rect::new(x + 2, inner.y + 1, inner.right().saturating_sub(x + 2), 4);
        notes_column(state, theme, notes, buf);
    }

    let status_y = inner.y + 6;
    if status_y < inner.bottom() {
        let row = Rect::new(inner.x + 1, status_y, inner.width.saturating_sub(2), 1);
        status_line(state, theme).render(row, buf);
    }
    let keys_y = inner.y + 8;
    if keys_y < inner.bottom() {
        let row = Rect::new(inner.x + 1, keys_y, inner.width.saturating_sub(2), 1);
        keys_line(state, theme).render(row, buf);
    }
}

/// The moon, and next to it the versions, the platform and where it installs.
fn head_column(state: &State, theme: &Theme, area: Rect, buf: &mut Buffer) {
    let versions = match state.next_version() {
        Some(next) => Line::from(vec![
            Span::styled(state.current.to_string(), theme.muted()),
            Span::styled("  →  ", theme.line()),
            Span::styled(next.to_string(), theme.accent_bold()),
        ]),
        None => Line::from(vec![
            Span::styled("moon ", theme.accent_bold()),
            Span::styled(state.current.to_string(), theme.muted()),
        ]),
    };
    let size = state
        .release
        .as_ref()
        .and_then(|r| r.asset_for(&state.target).ok())
        .map(|a| format!(" · {}", fmt_mb(a.size)))
        .unwrap_or_default();
    let rows = [
        versions,
        Line::styled(format!("{}{size}", state.target), theme.muted()),
        Line::styled(
            shorten(&state.dest_display(), area.width as usize),
            theme.muted(),
        ),
        Line::styled(String::new(), theme.muted()),
    ];
    let logo = logo_rows();
    for (i, row) in rows.into_iter().enumerate() {
        let y = area.y + i as u16;
        if y >= area.bottom() {
            break;
        }
        let mark = logo.get(i).cloned().unwrap_or_default();
        buf.set_string(area.x, y, &mark, theme.accent());
        let x = area.x + mark.width() as u16 + 2;
        let w = area.right().saturating_sub(x);
        row.render(Rect::new(x, y, w, 1), buf);
    }
}

/// What the release brings, as GitHub tells it.
fn notes_column(state: &State, theme: &Theme, area: Rect, buf: &mut Buffer) {
    let Some(release) = &state.release else {
        return;
    };
    buf.set_string(area.x, area.y, "What's new", theme.soft_bold());
    let notes = release.highlights(NOTES_MAX);
    let lines: Vec<String> = if notes.is_empty() {
        vec![format!("v{} · no release notes", release.version)]
    } else {
        notes
    };
    for (i, note) in lines.iter().enumerate() {
        let y = area.y + 1 + i as u16;
        if y >= area.bottom() {
            break;
        }
        let text = shorten(note, area.width.saturating_sub(2) as usize);
        buf.set_string(area.x, y, "• ", theme.line());
        buf.set_string(area.x + 2, y, text, theme.muted());
    }
}

/// The one row that says what is happening: the check, the bar, the result.
fn status_line<'a>(state: &State, theme: &'a Theme) -> Line<'a> {
    let spin = SPINNER[state.spinner % SPINNER.len()];
    match &state.phase {
        Phase::Checking => Line::from(vec![
            Span::styled(format!("{spin} "), theme.accent()),
            Span::styled("checking for updates…", theme.muted()),
        ]),
        Phase::UpToDate => Line::from(vec![
            Span::styled("✓ ", theme.ok()),
            Span::styled(
                format!("moon {} is the latest version", state.current),
                theme.text(),
            ),
        ]),
        Phase::Available => Line::from(vec![
            Span::styled("↑ ", theme.accent()),
            Span::styled(
                match state.next_version() {
                    Some(v) => format!("moon {v} is available"),
                    None => "a new version is available".to_string(),
                },
                theme.text(),
            ),
        ]),
        Phase::Confirm => Line::from(vec![Span::styled(
            match state.asset_size() {
                Some(size) => format!("{} · {}", state.asset_name(), fmt_mb(size)),
                None => state.asset_name(),
            },
            theme.muted(),
        )]),
        Phase::Working(Step::Download { got, total }) => {
            let mut spans = vec![Span::styled(bar(*got, *total), theme.accent())];
            spans.push(Span::styled(
                match total {
                    Some(t) => format!("  {} / {}", fmt_mb(*got), fmt_mb(*t)),
                    None => format!("  {}", fmt_mb(*got)),
                },
                theme.muted(),
            ));
            Line::from(spans)
        }
        Phase::Working(step) => Line::from(vec![
            Span::styled(format!("{spin} "), theme.accent()),
            Span::styled(
                match step {
                    Step::Verify => "verifying the download".to_string(),
                    Step::Install => format!("installing to {}", state.dest_display()),
                    Step::Download { .. } => unreachable!("handled above"),
                },
                theme.muted(),
            ),
        ]),
        Phase::Done => {
            let what = match state.next_version() {
                Some(v) => format!("moon {v} installed"),
                None => "installed".to_string(),
            };
            let mut spans = vec![
                Span::styled("✓ ", theme.ok()),
                Span::styled(what, theme.text()),
            ];
            if !state.verified {
                spans.push(Span::styled(
                    " · no checksums.txt: not verified",
                    theme.muted(),
                ));
            }
            Line::from(spans)
        }
        Phase::Cancelled => Line::from(vec![
            Span::styled("✗ ", theme.alert()),
            Span::styled("cancelled · nothing was touched", theme.muted()),
        ]),
        Phase::Refused(r) => Line::from(vec![
            Span::styled("✗ ", theme.alert()),
            Span::styled(r.why, theme.text()),
            Span::styled(" · --force overwrites it", theme.muted()),
        ]),
        Phase::Failed(e) => Line::from(vec![
            Span::styled("✗ ", theme.alert()),
            Span::styled(shorten(e, 68), theme.text()),
        ]),
    }
}

/// The keys, or what is left to do once there are none.
fn keys_line<'a>(state: &State, theme: &'a Theme) -> Line<'a> {
    let key = |k: &'a str, what: &'a str| {
        vec![
            Span::styled(k, theme.soft_bold()),
            Span::raw("  "),
            Span::styled(what, theme.muted()),
            Span::raw("      "),
        ]
    };
    match &state.phase {
        Phase::Confirm => Line::from([key("enter", "update"), key("esc", "cancel")].concat()),
        Phase::Available => Line::from(vec![
            Span::styled("moon update", theme.soft_bold()),
            Span::raw("  "),
            Span::styled("install it", theme.muted()),
        ]),
        Phase::Checking | Phase::Working(_) => Line::from(key("esc", "cancel")),
        Phase::Done => Line::from(vec![
            Span::styled("moon", theme.soft_bold()),
            Span::raw("  "),
            Span::styled("start chatting with the new version", theme.muted()),
        ]),
        Phase::UpToDate | Phase::Cancelled => Line::default(),
        // the command that updates this moon, whole: it is what to type next
        Phase::Refused(r) => Line::from(Span::styled(r.fix, theme.soft_bold())),
        Phase::Failed(_) => Line::from(vec![Span::styled(
            format!("install.sh with MOON_FORCE=1: {}", super::INSTALL_DOCS),
            theme.muted(),
        )]),
    }
}

/// `████████░░░░░░  61%`, or a bar that only fills up if the size is unknown.
fn bar(got: u64, total: Option<u64>) -> String {
    let frac = match total {
        Some(t) if t > 0 => (got as f64 / t as f64).clamp(0.0, 1.0),
        _ => 0.0,
    };
    let filled = (frac * BAR_WIDTH as f64).round() as usize;
    format!(
        "{}{}  {:>3.0}%",
        "█".repeat(filled),
        "░".repeat(BAR_WIDTH - filled),
        frac * 100.0
    )
}

pub fn fmt_mb(bytes: u64) -> String {
    let mb = bytes as f64 / 1e6;
    if mb >= 1000.0 {
        format!("{:.1} GB", mb / 1000.0)
    } else {
        format!("{mb:.1} MB")
    }
}

/// Cuts with an ellipsis, counting columns and not bytes.
fn shorten(s: &str, max: usize) -> String {
    if max == 0 || s.width() <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = c.to_string().width();
        if w + cw > max.saturating_sub(1) {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

/// Styles nothing: the plain text of a rendered box, for the tests.
#[cfg(test)]
pub fn plain(state: &State, theme: &Theme, width: u16) -> Vec<String> {
    let area = Rect::new(0, 0, width, HEIGHT);
    let mut buf = Buffer::empty(area);
    render(state, theme, area, &mut buf);
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

#[cfg(test)]
#[allow(unused_imports)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn state(phase: Phase) -> State {
        let mut s = State::new(
            "0.1.1".parse().unwrap(),
            moon_updater::Target::new("macos", "aarch64", false),
            PathBuf::from("/usr/local/bin/moon"),
        );
        s.release = Some(moon_updater::Release {
            version: "0.1.2".parse().unwrap(),
            tag: "v0.1.2".into(),
            notes: "* feat: model picker by @towerforge in http://x\n* fix: narrow terminals\n"
                .into(),
            url: String::new(),
            published_at: None,
            assets: vec![moon_updater::Asset {
                name: "moon-macos-aarch64.tar.gz".into(),
                url: String::new(),
                size: 4_200_000,
            }],
        });
        s.phase = phase;
        s
    }

    fn dump(phase: Phase, width: u16) -> String {
        plain(&state(phase), &Theme::moon(), width).join("\n")
    }

    #[test]
    fn the_box_says_from_which_version_to_which_and_what_it_brings() {
        let out = dump(Phase::Confirm, 80);
        assert!(out.contains("moon update"), "{out}");
        assert!(out.contains("0.1.1  →  0.1.2"), "{out}");
        assert!(out.contains("macos · aarch64 · 4.2 MB"), "{out}");
        assert!(out.contains("/usr/local/bin/moon"), "{out}");
        assert!(out.contains("What's new"), "{out}");
        assert!(out.contains("feat: model picker"), "{out}");
        // the author and the pull request are noise in a box this size
        assert!(!out.contains("@towerforge"), "{out}");
        assert!(out.contains("enter  update"), "{out}");
    }

    #[test]
    fn the_bar_advances_with_the_download() {
        let out = dump(
            Phase::Working(Step::Download {
                got: 2_100_000,
                total: Some(4_200_000),
            }),
            80,
        );
        assert!(out.contains("50%"), "{out}");
        assert!(out.contains("2.1 MB / 4.2 MB"), "{out}");
        assert!(out.contains("esc  cancel"), "{out}");
    }

    #[test]
    fn up_to_date_shows_neither_bar_nor_keys() {
        let mut s = state(Phase::UpToDate);
        s.release = None;
        let out = plain(&s, &Theme::moon(), 80).join("\n");
        assert!(out.contains("moon 0.1.1"), "{out}");
        assert!(out.contains("is the latest version"), "{out}");
        assert!(!out.contains("What's new"), "{out}");
        assert!(!out.contains("enter"), "{out}");
    }

    #[test]
    fn a_refusal_shows_the_whole_command_that_updates_it_instead() {
        let cargo = moon_updater::InstallKind::Cargo.refusal().unwrap();
        let out = dump(Phase::Refused(cargo), 80);
        assert!(out.contains("0.1.1  →  0.1.2"), "{out}");
        assert!(out.contains("✗ this moon came from cargo"), "{out}");
        assert!(out.contains("--force overwrites it"), "{out}");
        // the command is not cut short: it is the one thing to take away
        assert!(out.contains(cargo.fix), "{out}");
        // and install.sh is not the way back in from a cargo install
        assert!(!out.contains("install.sh"), "{out}");
        let dev = moon_updater::InstallKind::Dev.refusal().unwrap();
        let out = dump(Phase::Refused(dev), 80);
        assert!(out.contains("local build under target/"), "{out}");
        assert!(out.contains("┆ make build"), "{out}");
    }

    #[test]
    fn on_a_narrow_terminal_it_fits_without_overflowing() {
        for width in [40, 60, 66, 80, 120] {
            for row in plain(&state(Phase::Confirm), &Theme::moon(), width) {
                assert!(
                    row.width() <= width as usize,
                    "row `{row}` ({}) wider than {width}",
                    row.width()
                );
            }
        }
        // no room for two columns: the notes give way, the essentials stay
        let narrow = dump(Phase::Confirm, 50);
        assert!(!narrow.contains("What's new"), "{narrow}");
        assert!(narrow.contains("0.1.1  →  0.1.2"), "{narrow}");
    }

    #[test]
    fn the_failure_reads_and_points_at_the_installer() {
        let out = dump(Phase::Failed("checksum mismatch".into()), 80);
        assert!(out.contains("checksum mismatch"), "{out}");
        assert!(out.contains("install.sh"), "{out}");
    }

    #[test]
    fn sizes_read_in_mb() {
        assert_eq!(fmt_mb(4_200_000), "4.2 MB");
        assert_eq!(fmt_mb(0), "0.0 MB");
        assert_eq!(fmt_mb(2_500_000_000), "2.5 GB");
    }
}
