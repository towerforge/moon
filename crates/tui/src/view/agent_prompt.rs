//! An agent's description and prompt being edited: the title, each field
//! under a dashed rule with its name — the one the keys go to in `moon` —
//! and the footer. Like `/machine`, it draws its own area: the fields are
//! the box the conversation types in, not lines.

use super::*;
use crate::app::{Panel, PromptField};

/// Rows around the prompt: the title, the description's rule and its row,
/// the prompt's rule, the blank before the footer and the footer.
const AROUND: u16 = 6;

pub(super) fn render(app: &mut App, frame: &mut Frame, area: Rect) {
    let t = app.theme.clone();
    let keys = app.panel_keys();
    let w = area.width as usize;
    let Some(Panel::AgentPrompt(e)) = app.panel.as_mut() else {
        return;
    };
    let n = e.prompt.text().split('\n').count();
    let info = format!(
        "{}{n} {}",
        if e.changed() { "unsaved · " } else { "" },
        if n == 1 { "line" } else { "lines" }
    );
    let row = |y: u16| Rect {
        y: area.y + y,
        height: 1,
        ..area
    };
    let line = |frame: &mut Frame, y: u16, l: Line<'static>| {
        if y < area.height {
            frame.render_widget(Paragraph::new(l), row(y));
        }
    };
    line(
        frame,
        0,
        panel::head_line(&t, &format!("Agent › {}", e.name), &[], &info, w),
    );
    if area.height < AROUND + 1 {
        return;
    }
    let desc_on = e.field == PromptField::Description;
    line(frame, 1, rule(&t, "description", desc_on, w));
    let desc_cursor = e.description.render(row(2), frame.buffer_mut(), &t, 0);
    line(frame, 3, rule(&t, "prompt", !desc_on, w));
    let prompt_area = Rect {
        y: area.y + 4,
        height: area.height - AROUND,
        ..area
    };
    let prompt_cursor = e.prompt.render(prompt_area, frame.buffer_mut(), &t, 0);
    line(
        frame,
        area.height - 1,
        panel::panel_footer(&t, &keys, w, 0, 0, 0),
    );
    // the terminal cursor stays hidden; moon draws the block itself, in the
    // field the keys go to
    let (x, y) = if desc_on { desc_cursor } else { prompt_cursor };
    if x < area.right() && y < area.bottom() {
        frame.buffer_mut()[(x, y)].set_style(t.selected());
    }
}

/// ` prompt ╌╌╌╌`: the field's name, in `moon` when it has the keys.
fn rule(t: &Theme, label: &str, on: bool, w: usize) -> Line<'static> {
    let label = format!(" {label}");
    let fill = w.saturating_sub(width(&label) + 2);
    Line::from(vec![
        Span::styled(label, if on { t.accent_bold() } else { t.muted() }),
        Span::styled(format!(" {}", "╌".repeat(fill)), t.line()),
    ])
}

#[cfg(test)]
mod tests {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use super::super::tests::{app, screen};
    use super::*;
    use crate::app::PromptEdit;

    #[test]
    fn the_prompt_editor() {
        let mut app = app();
        app.loading = false;
        let file = moon_agent::AgentFile {
            description: "stages and commits".into(),
            max_steps: None,
            prompt: Some("# Committing\n\nYou prepare commits.\n".into()),
            permissions: Default::default(),
        };
        app.panel = Some(Panel::AgentPrompt(Box::new(PromptEdit::new(
            "committer",
            file,
        ))));
        let mut term = Terminal::new(TestBackend::new(90, 30)).unwrap();
        term.draw(|f| view(&mut app, f)).unwrap();
        let s = screen(&term);
        let title = s
            .iter()
            .position(|l| l.contains("Agent › committer"))
            .expect("title row");
        assert!(s[title].trim_end().ends_with("3 lines"), "{}", s[title]);
        // each field under its rule, the text as the box draws it
        assert!(s[title + 1].starts_with(" description ╌"), "{s:?}");
        assert!(s[title + 2].contains("❯ stages and commits"), "{s:?}");
        assert!(s[title + 3].starts_with(" prompt ╌"), "{s:?}");
        assert!(s[title + 4].contains("❯ # Committing"), "{s:?}");
        assert!(s[title + 6].contains("You prepare commits."), "{s:?}");
        assert!(
            s[29].contains("tab description") && s[29].contains("esc save & close"),
            "{}",
            s[29]
        );
        // the rule of the field with the keys in `moon`, the other muted
        let buf = term.backend().buffer();
        let prompt_row = (title + 3) as u16;
        let desc_row = (title + 1) as u16;
        assert_eq!(buf[(1, prompt_row)].fg, app.theme.accent().fg.unwrap());
        assert_ne!(buf[(1, desc_row)].fg, app.theme.accent().fg.unwrap());
        // changed, the title says so
        if let Some(Panel::AgentPrompt(e)) = app.panel.as_mut() {
            e.prompt.insert_char('!');
        }
        term.draw(|f| view(&mut app, f)).unwrap();
        let s = screen(&term);
        assert!(s[title].contains("unsaved · 3 lines"), "{}", s[title]);
    }
}
