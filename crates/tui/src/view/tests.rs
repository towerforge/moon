//! Rendering tests on a `TestBackend`: what each row of the screen shows.

use std::sync::Arc;

use moon_core::{Config, ConfigSource, Message, Registry, Role};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

use super::*;
use crate::app::{Action, HelpState, HelpTab, Item, RunOptions};
use crossterm::event::KeyCode;

pub(super) fn app() -> App {
    app_at(std::env::temp_dir())
}

/// The app with the project standing on `root`, for what reads the tree.
fn app_at(root: std::path::PathBuf) -> App {
    let mut app = App::new(RunOptions {
        config: Config::default(),
        config_source: ConfigSource::Default(std::path::PathBuf::from("/config.toml")),
        registry: Arc::new(Registry::new()),
        store: None,
        resume: None,
        model: None,
        version: "0.1.0".into(),
        cwd: "~/Towerforge/moon".into(),
        root,
        state_dir: None,
        agents_dir: None,
    });
    // the top row has a test of its own; the rest go without it
    app.cfg.general.system_stats = false;
    app
}

pub(super) fn screen(term: &Terminal<TestBackend>) -> Vec<String> {
    let buf = term.backend().buffer();
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect()
}

#[test]
fn the_top_row_has_moon_the_directory_and_the_machine() {
    use crate::sysmon::Sample;
    let mut app = app();
    app.loading = false;
    // moon, its version and the directory on the left, no rule under them:
    // the conversation's first row follows; with the readings off nothing
    // at the right
    let mut term = Terminal::new(TestBackend::new(120, 30)).unwrap();
    term.draw(|f| view(&mut app, f)).unwrap();
    let s = screen(&term);
    assert_eq!(
        s[0].trim_end(),
        " moon v0.1.0 · ~/Towerforge/moon",
        "{:?}",
        s[0]
    );
    assert!(!s[1].contains('─'), "{:?}", s[1]);
    // the directory has left the bottom row
    assert!(
        s[29].starts_with(" ⏵⏵ default · /agent · /model  "),
        "{}",
        s[29]
    );
    assert!(!s[29].contains("Towerforge"), "{}", s[29]);
    let buf = term.backend().buffer();
    assert_eq!(buf[(1, 0)].fg, app.theme.moon_soft);

    // the readings flush right, the value and the peak of each
    app.cfg.general.system_stats = true;
    app.sys.push(Sample::new(61.0, 24 << 30, 32 << 30, 0));
    app.sys.push(Sample::new(34.0, 18 << 30, 32 << 30, 0));
    term.draw(|f| view(&mut app, f)).unwrap();
    let s = screen(&term);
    assert!(
        s[0].starts_with(" moon v0.1.0 · ~/Towerforge/moon "),
        "{:?}",
        s[0]
    );
    assert!(s[0].ends_with(" cpu 34% ▲61 · ram 56% ▲75 "), "{:?}", s[0]);
    assert!(!s[29].contains("cpu"), "{}", s[29]);

    // the gpu with a card to read, the swap only when in use, and a newer
    // moon after the version
    app.sys
        .push(Sample::new(34.0, 18 << 30, 32 << 30, 1288490189).with_gpu(6 << 30, 8 << 30));
    app.update_available = Some("0.2.0".into());
    term.draw(|f| view(&mut app, f)).unwrap();
    let s = screen(&term);
    assert!(
        s[0].starts_with(" moon v0.1.0 ↑ v0.2.0 · ~/Towerforge/moon "),
        "{:?}",
        s[0]
    );
    assert!(
        s[0].ends_with(" cpu 34% ▲61 · ram 56% ▲75 · gpu 75% ▲75 · swap 1.2G "),
        "{:?}",
        s[0]
    );

    // narrow: the directory gives way, the readings stay whole
    let mut term = Terminal::new(TestBackend::new(70, 30)).unwrap();
    term.draw(|f| view(&mut app, f)).unwrap();
    let s = screen(&term);
    assert!(s[0].contains('…'), "{:?}", s[0]);
    assert!(s[0].ends_with("swap 1.2G "), "{:?}", s[0]);

    // on a short terminal the rows go to the conversation
    let mut term = Terminal::new(TestBackend::new(120, 10)).unwrap();
    term.draw(|f| view(&mut app, f)).unwrap();
    assert!(!screen(&term).iter().any(|r| r.contains("moon v0.1.0")));
}

#[test]
fn the_turn_tools_go_under_the_reply() {
    let mut app = app();
    app.loading = false;
    app.items.clear();
    app.push_item(Item::Message(Message::new(Role::User, "look")));
    app.push_item(Item::Message(Message::new(Role::Assistant, "done")));
    app.push_item(Item::Tools(vec![
        ("read".into(), 3),
        ("git diff".into(), 2),
        ("edit".into(), 1),
    ]));
    let lines: Vec<String> = app
        .visible_lines(80, 20)
        .iter()
        .map(|l| l.to_string())
        .collect();
    let at = lines
        .iter()
        .position(|l| l.contains(" tools ── "))
        .expect("summary");
    assert_eq!(
        lines[at], "─── 6 tools ── read ×3 · git diff ×2 · edit ×1",
        "{lines:?}"
    );
    // a blank row between the reply and the summary
    assert_eq!(lines[at - 1], "", "{lines:?}");

    // narrow: what does not fit goes on the next line, under the first tool
    let lines: Vec<String> = app
        .visible_lines(40, 20)
        .iter()
        .map(|l| l.to_string())
        .collect();
    let at = lines.iter().position(|l| l.contains(" tools ── ")).unwrap();
    assert_eq!(
        lines[at], "─── 6 tools ── read ×3 · git diff ×2",
        "{lines:?}"
    );
    assert_eq!(lines[at + 1], "               edit ×1", "{lines:?}");

    // one call, singular
    app.items.pop();
    app.push_item(Item::Tools(vec![("read".into(), 1)]));
    let lines: Vec<String> = app
        .visible_lines(80, 20)
        .iter()
        .map(|l| l.to_string())
        .collect();
    assert!(
        lines.iter().any(|l| l == "─── 1 tool ── read ×1"),
        "{lines:?}"
    );
}

#[test]
fn startup_screen() {
    let mut app = app();
    app.loading = false;
    let mut term = Terminal::new(TestBackend::new(90, 24)).unwrap();
    term.draw(|f| view(&mut app, f)).unwrap();
    let s = screen(&term);
    // the top row, then the conversation with the notes of the start — the
    // default configuration among them — and nothing else
    assert!(
        s[0].starts_with(" moon v0.1.0 · ~/Towerforge/moon"),
        "{s:?}"
    );
    assert!(
        s.iter().any(|r| r.contains("default configuration")),
        "{s:?}"
    );
    assert!(!s.iter().any(|r| r.contains("▄█")), "{s:?}");
    assert!(s[20].starts_with("─────"));
    assert!(s[21].starts_with("❯ "));
    // bottom left, the agent's name and the two commands that change it
    assert_eq!(
        s[23].trim_end(),
        " ⏵⏵ default · /agent · /model                                                    no model"
    );
    // the empty box suggests what to ask, in grey, the cursor on its first
    // letter
    assert_eq!(
        s[21].trim_end(),
        "❯ Try \"how do I log an error?\"",
        "{}",
        s[21]
    );
    let buf = term.backend().buffer();
    // the cursor is a `moon` block on the typing cell, and the terminal's stays hidden
    assert_eq!(buf[(2, 21)].bg, app.theme.moon);
    assert_eq!(buf[(2, 21)].fg, app.theme.on_moon);
    assert_eq!(buf[(3, 21)].fg, app.theme.ink_muted);

    // the first key typed takes it away
    app.update(key(KeyCode::Char('h')), &tx_dummy());
    term.draw(|f| view(&mut app, f)).unwrap();
    assert_eq!(screen(&term)[21].trim_end(), "❯ h");
    // and once something is sent — here a command, with no model to ask —
    // it does not come back
    app.update(key(KeyCode::Backspace), &tx_dummy());
    for c in "/version".chars() {
        app.update(key(KeyCode::Char(c)), &tx_dummy());
    }
    app.update(key(KeyCode::Enter), &tx_dummy());
    term.draw(|f| view(&mut app, f)).unwrap();
    assert_eq!(screen(&term)[21].trim_end(), "❯", "{:?}", screen(&term));

    // nor in a session picked up with a question in it
    let mut app = super::tests::app();
    app.loading = false;
    app.items
        .push(Item::Message(Message::new(Role::User, "earlier question")));
    term.draw(|f| view(&mut app, f)).unwrap();
    assert_eq!(screen(&term)[21].trim_end(), "❯");
}

#[test]
fn grouped_model_picker() {
    use crate::app::{Current, ProviderState};
    use moon_core::{Health, ModelInfo};
    let mut app = app();
    app.loading = false;
    app.providers = vec![
        ProviderState {
            id: "ollama".into(),
            kind: "ollama",
            base_url: "http://localhost:11434".into(),
            health: Some(Ok(Health::default())),
        },
        ProviderState {
            id: "openai".into(),
            kind: "openai",
            base_url: "https://api.openai.com/v1".into(),
            health: Some(Err("no API key".into())),
        },
    ];
    for i in 0..20 {
        let mut m = ModelInfo::new("ollama", format!("model-{i:02}"));
        m.context_length = Some(32_768);
        app.models.push(m);
    }
    app.current = Some(Current {
        provider: "ollama".into(),
        model: "model-03".into(),
    });
    app.recent = vec!["ollama/model-07".into(), "ollama/model-03".into()];
    app.panel = Some(Panel::Models(app.model_picker("")));
    let mut term = Terminal::new(TestBackend::new(100, 30)).unwrap();
    term.draw(|f| view(&mut app, f)).unwrap();
    let rows = screen(&term);
    let s = rows.join("\n");
    // no frame: the panel unfolds at the bottom, over the conversation's own
    // background, with the title on the first row and the counts on its right
    assert!(!s.contains("┌") && !s.contains("└"), "{s}");
    assert!(s.contains(" Select model "), "{s}");
    assert!(s.contains(" 20 models · 2 providers "), "{s}");
    assert!(s.contains("type to filter"), "{s}");
    assert!(s.contains(" Recent ─"), "{s}");
    assert!(s.contains("─ 2 models "), "{s}");
    assert!(s.contains(" ollama ─"), "{s}");
    assert!(s.contains("─ ● localhost:11434 "), "{s}");
    // recents carry the provider in front of the detail
    assert!(
        s.contains("model-07") && s.contains("ollama · ctx 32.8k"),
        "{s}"
    );
    // not everything fits: the bar on the right and the position in the footer
    assert!(s.contains("1-14/27"), "{s}");
    let bar: String = rows
        .iter()
        .filter_map(|r| r.chars().nth(99))
        .filter(|c| *c == '│' || *c == '█')
        .collect();
    assert!(bar.starts_with('█') && bar.contains('│'), "{bar:?}\n{s}");
    // every row carries its number while there is a digit left for it, the
    // cursor marks one with `❯` and the model in use closes with `✓`; under
    // the cursor the name goes in moon-soft and bold
    let buf = term.backend().buffer();
    let y = rows
        .iter()
        .position(|r| r.contains("❯  2. model-03 ✓"))
        .unwrap() as u16;
    let row = &rows[y as usize];
    let x = row[..row.find("model-03").unwrap()].chars().count() as u16;
    assert_eq!(buf[(x, y)].style().fg, Some(app.theme.moon_soft));
    assert!(buf[(x, y)]
        .modifier
        .contains(ratatui::style::Modifier::BOLD));
    // the panel paints no surface of its own: the background is the one below
    assert_eq!(buf[(x, y)].bg, ratatui::style::Color::Reset);
    assert_eq!(buf[(0, y)].bg, ratatui::style::Color::Reset);
    // cursor and check, both in moon, one at each end of the name
    let col = |row: &str, pat: &str| row[..row.find(pat).unwrap()].chars().count() as u16;
    assert_eq!(buf[(col(row, "❯"), y)].style().fg, Some(app.theme.moon));
    assert_eq!(buf[(col(row, "✓"), y)].style().fg, Some(app.theme.moon));
    assert!(col(row, "✓") > col(row, "model-03"));
    // its copy under `ollama` carries the check but not the cursor: moon, no bold
    let y2 = rows
        .iter()
        .position(|r| !r.contains('❯') && r.contains("model-03 ✓"))
        .unwrap() as u16;
    assert_eq!(buf[(x, y2)].style().fg, Some(app.theme.moon));
    assert!(!buf[(x, y2)]
        .modifier
        .contains(ratatui::style::Modifier::BOLD));
    // a model that is neither under the cursor nor in use stays in ink
    let y3 = rows.iter().position(|r| r.contains(". model-00")).unwrap() as u16;
    assert_eq!(buf[(x, y3)].style().fg, Some(app.theme.ink));
    // going down to the last model (22 items, the cursor starts on the
    // second) the view follows the cursor and openai's empty section shows
    for _ in 0..20 {
        app.update(
            Action::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Down,
                crossterm::event::KeyModifiers::NONE,
            )),
            &tx_dummy(),
        );
    }
    term.draw(|f| view(&mut app, f)).unwrap();
    let s = screen(&term).join("\n");
    assert!(s.contains(" openai ─"), "{s}");
    assert!(s.contains("─ ✗ no API key "), "{s}");
    assert!(!s.contains(" Recent ─"), "{s}");
}

#[test]
fn session_dialogs() {
    use crate::app::{Choice, SessionAction};
    use crate::picker::Picker;
    let mut app = app();
    app.loading = false;
    let mut picker = Picker::new("Resume a session", Vec::new(), "");
    picker.keys = vec![("↑↓", "move"), ("ctrl+d", "delete"), ("esc", "close")];
    app.panel = Some(Panel::SessionAction {
        picker: Box::new(picker),
        action: SessionAction::Delete {
            id: "x".into(),
            title: "explain the sessions module".into(),
            choice: Choice::Delete,
            open: false,
        },
    });
    let mut term = Terminal::new(TestBackend::new(90, 26)).unwrap();
    term.draw(|f| view(&mut app, f)).unwrap();
    let rows = screen(&term);
    let s = rows.join("\n");
    assert!(s.contains(" Delete session"), "{s}");
    assert!(s.contains("«explain the sessions module»"), "{s}");
    // two options under the cursor, like any other list; no buttons
    assert!(s.contains(" ❯ Delete"), "{s}");
    assert!(s.contains("   Keep"), "{s}");
    assert!(s.contains("↑↓ choose · enter confirm · esc keep"), "{s}");
    // the option under the cursor goes in moon-soft and bold; the other one
    // does not
    let buf = term.backend().buffer();
    let y = rows.iter().position(|r| r.contains("❯ Delete")).unwrap();
    let yk = rows.iter().position(|r| r.contains("Keep")).unwrap();
    let x = rows[y][..rows[y].find("Delete").unwrap()].chars().count() as u16;
    assert!(buf[(x, y as u16)]
        .modifier
        .contains(ratatui::style::Modifier::BOLD));
    assert_eq!(buf[(x, y as u16)].style().fg, Some(app.theme.moon_soft));
    assert!(!buf[(x, yk as u16)]
        .modifier
        .contains(ratatui::style::Modifier::BOLD));
    // the footer keys go in moon and the action in ink
    let y = rows
        .iter()
        .position(|r| r.contains("enter confirm"))
        .unwrap();
    let row = &rows[y];
    let xk = row[..row.find("enter").unwrap()].chars().count() as u16;
    assert_eq!(buf[(xk, y as u16)].style().fg, Some(app.theme.moon));
    assert_eq!(buf[(xk + 6, y as u16)].style().fg, Some(app.theme.ink));

    let Some(Panel::SessionAction { picker, .. }) = app.panel.take() else {
        unreachable!()
    };
    app.panel = Some(Panel::SessionAction {
        picker,
        action: SessionAction::Rename {
            id: "x".into(),
            input: "new title".into(),
        },
    });
    term.draw(|f| view(&mut app, f)).unwrap();
    let s = screen(&term).join("\n");
    assert!(s.contains(" Rename session"), "{s}");
    assert!(s.contains("❯ new title█"), "{s}");
    assert!(s.contains("enter save · esc cancel"), "{s}");
}

#[test]
fn jump_to_bottom_indicator() {
    use crate::app::Item;
    let mut app = app();
    app.loading = false;
    for i in 0..40 {
        app.items.push(Item::Info(format!("line {i}")));
    }
    let mut term = Terminal::new(TestBackend::new(80, 20)).unwrap();
    term.draw(|f| view(&mut app, f)).unwrap();
    assert!(
        app.jump_rect.is_none(),
        "following the bottom there is no indicator"
    );
    app.scroll_by(-5);
    term.draw(|f| view(&mut app, f)).unwrap();
    let s = screen(&term);
    let rect = app.jump_rect.expect("the indicator shows once scrolled up");
    // last row of the conversation: 20 rows − 5 lower zones − the blank one − 1
    assert_eq!(rect.y, 13);
    assert!(s[13].contains("↓ Jump to bottom"), "{}", s[13]);
    app.update(Action::MouseDown(rect.x + 2, rect.y), &tx_dummy());
    assert!(app.follow);
}

fn tx_dummy() -> crate::app::Tx {
    tokio::sync::mpsc::unbounded_channel().0
}

fn key(code: crossterm::event::KeyCode) -> Action {
    Action::Key(crossterm::event::KeyEvent::new(
        code,
        crossterm::event::KeyModifiers::NONE,
    ))
}

fn ctrl_key(code: crossterm::event::KeyCode) -> Action {
    Action::Key(crossterm::event::KeyEvent::new(
        code,
        crossterm::event::KeyModifiers::CONTROL,
    ))
}

#[test]
fn activity_on_the_left_and_status_on_the_right() {
    use crate::app::{Current, Generation};
    use std::time::Instant;
    let mut app = app();
    app.loading = false;
    app.current = Some(Current {
        provider: "ollama".into(),
        model: "m".into(),
    });
    app.gen = Generation::Streaming {
        cancel: tokio_util::sync::CancellationToken::new(),
        started: Instant::now(),
        first_at: None,
        deltas: 0,
        sent: 0,
    };
    let mut term = Terminal::new(TestBackend::new(80, 12)).unwrap();
    term.draw(|f| view(&mut app, f)).unwrap();
    let s = screen(&term);
    // 12 rows: 6 of conversation (0-5), the blank one that separates it (6)
    // and the status on row 7, a column in, like the hints below
    assert_eq!(s[6].trim(), "", "{}", s[6]);
    let status = &s[7];
    assert!(
        status.starts_with(" · thinking… (0s) · esc to cancel"),
        "{status}"
    );
    assert!(!status.contains('●'), "{status}");
    // the model goes at the very bottom, to the right of the hints
    let hints = &s[11];
    assert!(hints.starts_with(" ⏵⏵ default "), "{hints}");
    assert!(hints.trim_end().ends_with(" m"), "{hints}");
}

#[test]
fn the_loaded_models_size_goes_after_its_name() {
    use crate::app::{Current, LoadedState};
    use crate::sysmon::Sample;
    use moon_core::LoadedModel;
    let mut app = app();
    app.loading = false;
    app.current = Some(Current {
        provider: "ollama".into(),
        model: "m".into(),
    });
    app.sys.push(Sample::new(34.0, 18 << 30, 32 << 30, 0));
    let hints_at = |app: &mut App, cols: u16| {
        let mut term = Terminal::new(TestBackend::new(cols, 12)).unwrap();
        term.draw(|f| view(app, f)).unwrap();
        screen(&term)[11].trim_end().to_string()
    };
    let model = |size: u64, vram: u64| {
        LoadedState::Loaded(LoadedModel {
            id: "m".into(),
            size_bytes: size,
            size_vram_bytes: vram,
            context_length: Some(32_768),
            expires_at: None,
        })
    };
    // not loaded: no size, and its absence is what tells it apart
    app.loaded = LoadedState::NotLoaded;
    let h = hints_at(&mut app, 130);
    assert!(h.ends_with(" m"), "{h}");
    // loaded and fully on the GPU: the size, with no split
    app.loaded = model(13_000_000_000, 13_000_000_000);
    let h = hints_at(&mut app, 130);
    assert!(h.ends_with("m · 12.1G"), "{h}");
    // partly off the GPU: it is flagged, because generation crawls
    app.loaded = model(13_000_000_000, 9_100_000_000);
    let h = hints_at(&mut app, 130);
    assert!(h.ends_with("m · 12.1G · 30% cpu"), "{h}");
    // on a narrow terminal the size drops off
    let h = hints_at(&mut app, 100);
    assert!(h.ends_with(" m"), "{h}");
}

#[test]
fn the_selection_is_highlighted() {
    use crate::app::{Item, Selection};
    let mut app = app();
    app.loading = false;
    app.items.clear(); // with no providers, App::new leaves an error in the conversation
    app.items.push(Item::Info("one two four".into()));
    let mut term = Terminal::new(TestBackend::new(60, 14)).unwrap();
    term.draw(|f| view(&mut app, f)).unwrap();
    // a blank row, then row 1 = "one two four"; select "two". On screen it
    // is row 2, under the top row
    app.selection = Some(Selection {
        anchor: (1, 4),
        head: (1, 6),
        dragging: false,
    });
    term.draw(|f| view(&mut app, f)).unwrap();
    let buf = term.backend().buffer();
    assert_eq!(buf[(4, 2)].bg, app.theme.moon);
    assert_eq!(buf[(6, 2)].bg, app.theme.moon);
    assert_ne!(buf[(7, 2)].bg, app.theme.moon);
    assert_ne!(buf[(3, 2)].bg, app.theme.moon);
    assert_eq!(app.selection_text(), "two");
}

#[test]
fn the_file_count_goes_in_the_status_row() {
    use moon_core::Spec;
    let mut app = app();
    app.loading = false;
    app.context_file = Some(("MOON.md".into(), "x".into()));
    app.live = vec![
        Spec::parse("a.rs").unwrap(),
        Spec::parse("b.rs:1-80").unwrap(),
    ];
    let mut term = Terminal::new(TestBackend::new(80, 16)).unwrap();
    term.draw(|f| view(&mut app, f)).unwrap();
    let s = screen(&term);
    // 16 rows: conversation 0-10, status 11, separator 12, input 13. No chips
    // row any more: the count goes on the right of the status row
    assert!(s[11].contains("2 files"), "{}", s[11]);
    assert!(!s.join("\n").contains("@a.rs ×"), "{}", s[11]);
    assert!(s[12].starts_with("─────"), "{}", s[12]);
    assert!(s[13].starts_with("❯ "), "{}", s[13]);
    // with one file the word is singular, and with none nothing is shown
    app.live.truncate(1);
    term.draw(|f| view(&mut app, f)).unwrap();
    assert!(
        screen(&term)[11].contains("1 file"),
        "{}",
        screen(&term)[11]
    );
    app.live.clear();
    term.draw(|f| view(&mut app, f)).unwrap();
    let s = screen(&term);
    assert!(!s[11].contains("file"), "{}", s[11]);
    assert!(s[13].starts_with("❯ "), "{}", s[13]);
}

#[test]
fn files_panel_and_tree() {
    use moon_core::Spec;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "fn lib() {}\n").unwrap();
    let mut app = app_at(dir.path().to_path_buf());
    app.loading = false;
    app.context_file = Some(("MOON.md".into(), "x".into()));
    app.live = vec![Spec::parse("a.rs").unwrap()];
    app.open_files_panel();
    let mut term = Terminal::new(TestBackend::new(90, 30)).unwrap();
    term.draw(|f| view(&mut app, f)).unwrap();
    let rows = screen(&term);
    let s = rows.join("\n");
    // the attachment with what it costs, the project file apart, and the buttons
    assert!(s.contains(" Files "), "{s}");
    assert!(s.contains(" Attached ─"), "{s}");
    assert!(s.contains("a.rs"), "{s}");
    assert!(s.contains(" Project ─") && s.contains("MOON.md"), "{s}");
    assert!(s.contains("Add files…") && s.contains("Detach all"), "{s}");
    let footer = rows
        .iter()
        .find(|r| r.contains("esc close"))
        .expect("the panel footer");
    assert!(footer.contains("enter detach") && footer.contains("ctrl+a add"));

    // ctrl+a opens the tree at the root: folders first, then files
    let tx = tx_dummy();
    app.update(ctrl_key(KeyCode::Char('a')), &tx);
    term.draw(|f| view(&mut app, f)).unwrap();
    let rows = screen(&term);
    let s = rows.join("\n");
    assert!(s.contains(" Add files "), "{s}");
    assert!(s.contains("project root"), "{s}");
    assert!(s.contains(" Folders ─") && s.contains("src/"), "{s}");
    assert!(s.contains(" Files ─"), "{s}");
    // a.rs is already attached: it carries the check
    let row = rows.iter().find(|r| r.contains("a.rs")).expect("fila a.rs");
    assert!(row.contains("✓"), "{row}");
    assert!(rows.iter().any(|r| r.contains("esc back")), "{s}");

    // esc steps back into the files panel instead of closing the bottom
    app.update(key(KeyCode::Esc), &tx);
    assert!(matches!(app.panel, Some(Panel::Files(_))));
    app.update(key(KeyCode::Esc), &tx);
    assert!(app.panel.is_none());
}

#[test]
fn help_panel() {
    let mut app = app();
    app.panel = Some(Panel::Help(HelpState::default()));
    let mut term = Terminal::new(TestBackend::new(100, 70)).unwrap();
    term.draw(|f| view(&mut app, f)).unwrap();
    let rows = screen(&term);
    let s = rows.join("\n");
    // the title row carries the sections, and it opens on the first one
    let title = rows
        .iter()
        .find(|r| r.contains("Help"))
        .expect("the title row");
    assert!(title.contains("General"), "{title}");
    assert!(title.contains("Commands"), "{title}");
    assert!(title.contains("Keys"), "{title}");
    assert!(title.contains("moon v0.1.0"), "{title}");
    // no frame of any kind: the help is one more panel
    assert!(
        !s.contains("┌") && !s.contains("╭") && !s.contains("│"),
        "{s}"
    );
    // General is a read, not a list: no command rows in it
    assert!(s.contains("Essentials ─"), "{s}");
    assert!(!s.contains("/model [name]"), "{s}");
    // Files is rows too, same as Essentials: a short label, then the text
    assert!(
        s.contains("MOON.md") && s.contains("Sessions") && s.contains("Updates"),
        "{s}"
    );
    // no config file yet: says where `moon config init` would write one
    assert!(
        s.contains("Config") && s.contains("writes one at /config.toml"),
        "{s}"
    );
    let footer = rows
        .iter()
        .find(|r| r.contains("esc close"))
        .expect("the panel footer");
    assert!(footer.contains("tab section"), "{footer}");
    assert!(footer.contains("↑↓ scroll"), "{footer}");
    // on a tall terminal the section fits whole: no position in the footer
    assert!(!footer.contains('/'), "{footer}");

    // tab walks to the commands: the whole list, with its count
    let tx = tx_dummy();
    app.update(key(KeyCode::Tab), &tx);
    term.draw(|f| view(&mut app, f)).unwrap();
    let rows = screen(&term);
    let s = rows.join("\n");
    assert!(s.contains("/provider [id]"), "{s}");
    let title = rows
        .iter()
        .find(|r| r.contains("Help"))
        .expect("the title row");
    assert!(
        title.contains(&format!("{} commands", SPECS.len())),
        "{title}"
    );

    // and once more to the keys, which are not mixed in with the commands
    app.update(key(KeyCode::Tab), &tx);
    term.draw(|f| view(&mut app, f)).unwrap();
    let rows = screen(&term);
    let s = rows.join("\n");
    assert!(s.contains("ctrl+p"), "{s}");
    assert!(!s.contains("/model [name]"), "{s}");
    let title = rows
        .iter()
        .find(|r| r.contains("Help"))
        .expect("the title row");
    assert!(title.contains(&format!("{} keys", KEYS.len())), "{title}");

    // shift+tab wraps back round to General
    app.update(
        Action::Key(crossterm::event::KeyEvent::new(
            KeyCode::BackTab,
            crossterm::event::KeyModifiers::SHIFT,
        )),
        &tx,
    );
    app.update(key(KeyCode::Left), &tx);
    term.draw(|f| view(&mut app, f)).unwrap();
    assert!(screen(&term).join("\n").contains("Essentials ─"));

    // narrow enough, the long descriptions wrap: the continuation lines up
    // under the description, it is not stuck to the left edge
    app.update(key(KeyCode::Tab), &tx);
    let mut term = Terminal::new(TestBackend::new(74, 70)).unwrap();
    term.draw(|f| view(&mut app, f)).unwrap();
    let rows = screen(&term);
    let head = rows
        .iter()
        .position(|r| r.contains("/params"))
        .expect("the /params row");
    let col = rows[head][..rows[head].find("generation").unwrap()]
        .chars()
        .count();
    let cont = &rows[head + 1];
    let indent = cont.chars().take_while(|c| *c == ' ').count();
    assert!(
        !cont.trim().is_empty(),
        "the /params description is cut off"
    );
    assert_eq!(indent, col, "{cont}");
    assert!(col >= 10, "{cont}");
}

#[test]
fn the_help_scrolls() {
    let mut app = app();
    app.panel = Some(Panel::Help(HelpState {
        tab: HelpTab::Commands,
        ..Default::default()
    }));
    let mut term = Terminal::new(TestBackend::new(90, 24)).unwrap();
    term.draw(|f| view(&mut app, f)).unwrap();
    let s = screen(&term).join("\n");
    assert!(s.contains("/provider [id]"));
    assert!(
        !s.contains("/quit"),
        "the commands do not fit without scrolling"
    );
    assert!(s.contains("↑↓ scroll · esc close"));
    assert!(s.contains("1-"), "position in the footer");
    let Some(Panel::Help(h)) = app.panel else {
        unreachable!()
    };
    assert!(h.rows > 0 && h.total > h.rows);

    app.update(Action::ScrollBy(100), &tx_dummy());
    term.draw(|f| view(&mut app, f)).unwrap();
    let s = screen(&term).join("\n");
    assert!(s.contains("/quit"));
    assert!(!s.contains("/model [name]"));
    assert!(s.contains(&format!("/{} ", h.total)));
    // changing section starts the new one from the top
    app.update(key(KeyCode::Tab), &tx_dummy());
    let Some(Panel::Help(h)) = app.panel else {
        unreachable!()
    };
    assert_eq!((h.tab, h.scroll), (HelpTab::Keys, 0));
}

#[test]
fn suggestions_above_the_box() {
    let mut app = app();
    app.loading = false;
    let tx = tx_dummy();
    for c in "/s".chars() {
        app.update(
            Action::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(c),
                crossterm::event::KeyModifiers::NONE,
            )),
            &tx,
        );
    }
    let mut term = Terminal::new(TestBackend::new(80, 20)).unwrap();
    term.draw(|f| view(&mut app, f)).unwrap();
    let rows = screen(&term);
    let s = rows.join("\n");
    assert!(s.contains("/system [text]"));
    assert!(s.contains("/sessions "));
    assert!(!s.contains("/quit"));
    // titled and counted like a panel, and with no surface under it
    assert!(s.contains(" Commands"), "{s}");
    assert!(s.contains("3 commands"), "{s}");
    // the first one carries the cursor and the tail of its name goes in
    // moon-soft; the second one, in ink-muted
    let buf = term.backend().buffer();
    let y_model = rows
        .iter()
        .position(|r| r.contains("/system [text]"))
        .unwrap() as u16;
    let y_models = rows.iter().position(|r| r.contains("/sessions ")).unwrap() as u16;
    assert_eq!(y_models, y_model + 1);
    assert!(rows[y_model as usize].starts_with(" ❯ /system"), "{s}");
    assert!(rows[y_models as usize].starts_with("   /sessions"), "{s}");
    let bold = |x: u16, y: u16| {
        buf[(x, y)]
            .style()
            .add_modifier
            .contains(ratatui::style::Modifier::BOLD)
    };
    // `/s`, what is already typed, goes in moon and bold on every row, as in
    // the box; the rest of the name is moon-soft under the cursor and plain
    // ink-muted on the others
    assert_eq!(buf[(3, y_model)].style().fg, Some(app.theme.moon));
    assert!(bold(3, y_model));
    assert_eq!(buf[(4, y_model)].style().fg, Some(app.theme.moon));
    assert!(bold(4, y_model));
    assert_eq!(buf[(4, y_models)].style().fg, Some(app.theme.moon));
    assert!(bold(4, y_models));
    assert_eq!(buf[(5, y_model)].style().fg, Some(app.theme.moon_soft));
    assert!(!bold(5, y_model));
    assert_eq!(buf[(5, y_models)].style().fg, Some(app.theme.ink_muted));
    assert!(!bold(5, y_models));
    assert_eq!(buf[(4, y_model)].bg, ratatui::style::Color::Reset);
    assert_eq!(buf[(4, y_models)].bg, ratatui::style::Color::Reset);
    // the typed command goes in moon and bold in the box
    let y_input = rows.iter().position(|r| r.starts_with("❯ /s")).unwrap() as u16;
    let st = buf[(2, y_input)].style();
    assert_eq!(st.fg, Some(app.theme.moon));
    assert!(st.add_modifier.contains(ratatui::style::Modifier::BOLD));
    assert_eq!(buf[(1, y_input)].style().fg, Some(app.theme.moon));
    assert!(!buf[(1, y_input)]
        .style()
        .add_modifier
        .contains(ratatui::style::Modifier::BOLD));

    // ↓ moves the highlight
    app.update(
        Action::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Down,
            crossterm::event::KeyModifiers::NONE,
        )),
        &tx,
    );
    term.draw(|f| view(&mut app, f)).unwrap();
    let buf = term.backend().buffer();
    let rows = screen(&term);
    assert!(
        rows[y_models as usize].starts_with(" ❯ /sessions"),
        "{}",
        rows[y_models as usize]
    );
    assert!(buf[(4, y_models)]
        .style()
        .add_modifier
        .contains(ratatui::style::Modifier::BOLD));
}

#[test]
fn panel_de_la_maquina_con_braille() {
    use std::time::{Duration, Instant};
    let mut app = app();
    app.loading = false;
    let tx = tx_dummy();
    // three minutes of samples: a sawtooth for the cpu, something flat for ram
    let base = Instant::now() - Duration::from_secs(180);
    for i in 0..36u64 {
        let cpu = (i % 12) as f32 * 8.0;
        app.sys.push_at(
            base + Duration::from_secs(i * 5),
            crate::sysmon::Sample::new(cpu, 16 << 30, 32 << 30, 0),
        );
    }
    for c in "/machine".chars() {
        app.update(key(crossterm::event::KeyCode::Char(c)), &tx);
    }
    app.update(key(crossterm::event::KeyCode::Enter), &tx);
    assert!(matches!(app.panel, Some(Panel::Machine)));

    let mut term = Terminal::new(TestBackend::new(78, 24)).unwrap();
    term.draw(|f| view(&mut app, f)).unwrap();
    let rows = screen(&term);
    let s = rows.join("\n");
    // titled and counted like any other panel, and with its own footer
    assert!(s.contains(" Machine"), "{s}");
    assert!(s.contains("3 min · 36 samples"), "{s}");
    assert!(s.contains("esc close"), "{s}");
    // the readings: the current value, the peak of the window and the totals
    assert!(s.contains("cpu"), "{s}");
    assert!(s.contains("▲88%"), "{s}");
    assert!(s.contains("ram"), "{s}");
    assert!(s.contains("16.0 / 32.0G"), "{s}");
    assert!(s.contains("no swap"), "{s}");
    // drawn with braille, and with the input box out of the way
    assert!(
        s.chars()
            .filter(|c| ('\u{2800}'..='\u{28ff}').contains(c))
            .count()
            > 100,
        "{s}"
    );
    assert!(!rows.iter().any(|r| r.starts_with("❯ ")), "{s}");

    // the cpu trace goes in moon and the ram one in moon-soft, over a grid in
    // night-line
    // both headings share a row, cpu on the left of the rule and ram on its
    // right, and each plot stays in its half
    let buf = term.backend().buffer();
    let head_y = rows.iter().position(|r| r.contains("ram")).unwrap();
    let head = &rows[head_y];
    let rule = head.chars().position(|c| c == '│').unwrap();
    assert!(head.find("cpu").unwrap() < rule, "{head}");
    assert!(head.find("ram").unwrap() > rule, "{head}");
    let plots = head_y + 1..rows.iter().position(|r| r.contains("no swap")).unwrap();
    let colors = |cols: std::ops::Range<u16>| {
        let mut v = Vec::new();
        for y in plots.clone() {
            for x in cols.clone() {
                if buf[(x, y as u16)].symbol() != " " {
                    v.push(buf[(x, y as u16)].style().fg);
                }
            }
        }
        v
    };
    let left = colors(0..rule as u16);
    let right = colors(rule as u16 + 1..78);
    assert!(left.contains(&Some(app.theme.moon)), "traza de la cpu");
    assert!(
        !left.contains(&Some(app.theme.moon_soft)),
        "la ram invade la cpu"
    );
    assert!(
        right.contains(&Some(app.theme.moon_soft)),
        "traza de la ram"
    );
    // filled from the curve down: the floor of the plot comes out solid
    let floor = plots
        .clone()
        .rfind(|y| {
            rows[*y]
                .chars()
                .any(|c| ('\u{2800}'..='\u{28ff}').contains(&c))
        })
        .unwrap();
    let solid = (0..rule as u16)
        .filter(|x| buf[(*x, floor as u16)].symbol() == "⣿")
        .count();
    assert!(solid > 20, "suelo relleno: {solid}\n{}", rows[floor]);

    // esc closes it and the box comes back
    app.update(key(crossterm::event::KeyCode::Esc), &tx);
    assert!(app.panel.is_none());
}

#[test]
fn the_machine_panel_grows_a_gpu_column_and_draws_the_model_under_the_ram() {
    use crate::app::{LoadedState, Panel};
    use crate::sysmon::Sample;
    use moon_core::LoadedModel;
    use std::time::{Duration, Instant};
    let mut app = app();
    app.loading = false;
    let tx = tx_dummy();
    // a 12G model, 8G of it in an 8G card: 4G in ram, a third on the cpu
    app.loaded = LoadedState::Loaded(LoadedModel {
        id: "m".into(),
        size_bytes: 12 << 30,
        size_vram_bytes: 8 << 30,
        context_length: None,
        expires_at: None,
    });
    let base = Instant::now() - Duration::from_secs(60);
    for i in 0..12u64 {
        app.sys.push_at(
            base + Duration::from_secs(i * 5),
            Sample::new(20.0, 16 << 30, 32 << 30, 0)
                .with_model(4 << 30, false)
                .with_gpu(6 << 30, 8 << 30),
        );
    }
    for c in "/machine".chars() {
        app.update(key(crossterm::event::KeyCode::Char(c)), &tx);
    }
    app.update(key(crossterm::event::KeyCode::Enter), &tx);
    assert!(matches!(app.panel, Some(Panel::Machine)));

    let mut term = Terminal::new(TestBackend::new(78, 24)).unwrap();
    term.draw(|f| view(&mut app, f)).unwrap();
    let rows = screen(&term);
    let s = rows.join("\n");
    // three headings on one row, cpu · ram · gpu, with a rule between each two
    let head_y = rows.iter().position(|r| r.contains("gpu")).unwrap();
    let head = &rows[head_y];
    assert_eq!(head.matches('│').count(), 2, "{head}");
    let (c, r, g) = (
        head.find("cpu").unwrap(),
        head.find("ram").unwrap(),
        head.find("gpu").unwrap(),
    );
    assert!(c < r && r < g, "{head}");
    assert!(head.contains("75%"), "{head}");
    // at 78 columns the totals do not fit next to the headings: they go
    // under the plots, with the swap and the model, marked as the band
    assert!(
        s.contains("ram 16.0 / 32.0G · gpu 6.0 / 8.0G · no swap · ▮ 12.0G loaded · 33% on cpu"),
        "{s}"
    );
    // the model's share is a band along the floor of the ram plot, in
    // ink-muted, under the ram trace in moon-soft; the gpu trace goes in ink
    let buf = term.backend().buffer();
    let rule1 = head.chars().position(|c| c == '│').unwrap() as u16;
    let rule2 = head.chars().count() - 1 - head.chars().rev().position(|c| c == '│').unwrap();
    let rule2 = rule2 as u16;
    let floor = head_y as u16 + 6;
    let colors = |cols: std::ops::Range<u16>, y: u16| -> Vec<Option<ratatui::style::Color>> {
        cols.filter(|x| buf[(*x, y)].symbol() != " ")
            .map(|x| buf[(x, y)].style().fg)
            .collect()
    };
    let ram_floor = colors(rule1 + 1..rule2, floor);
    assert!(ram_floor.contains(&Some(app.theme.ink_muted)), "{s}");
    let ram_above = colors(rule1 + 1..rule2, floor - 2);
    assert!(ram_above.contains(&Some(app.theme.moon_soft)), "{s}");
    assert!(!ram_above.contains(&Some(app.theme.ink_muted)), "{s}");
    let gpu_floor = colors(rule2 + 1..78, floor);
    assert!(gpu_floor.contains(&Some(app.theme.ink)), "{s}");
    assert!(!gpu_floor.contains(&Some(app.theme.moon_soft)), "{s}");
}

#[test]
fn the_approval_panel_and_the_edits_marker() {
    use crate::app::{Approval, Panel};
    use moon_agent::{editor, tools, Eol, Harness, Limits, PendingEdit, Sandbox, Tool};
    let mut app = app();
    app.loading = false;
    // the editor behind the switch: reading, editing and creating
    let sandbox = Sandbox::new(&std::env::temp_dir(), 10_000, &[]).unwrap();
    app.harness = Some(Harness::new(editor(), sandbox, Limits::default()));
    app.tools_on = true;
    let mut term = Terminal::new(TestBackend::new(90, 24)).unwrap();
    term.draw(|f| view(&mut app, f)).unwrap();
    let s = screen(&term);
    // the sign sits on the bottom row, on the left, listing what is on; the
    // welcome banner does not repeat it
    assert!(s[23].trim_start().starts_with("⏵⏵ default "), "{}", s[23]);
    assert!(s[23].trim_end().ends_with("no model"), "{}", s[23]);
    assert!(!s.iter().any(|l| l.contains("✎ edits on")), "{s:?}");

    let before = "fn main() {\n    hi();\n}\n";
    let after = "fn main() {\n    bye();\n}\n";
    let edit = PendingEdit {
        tool: Tool::EditFile,
        path: "src/a.rs".into(),
        to: None,
        before: before.into(),
        after: after.into(),
        eol: Eol::Lf,
        bom: false,
        expect: Some(1),
        diff: tools::diff(before, after),
    };
    app.panel = Some(Panel::Approval(Box::new(Approval::new(
        moon_agent::Pending::Edit(edit),
    ))));
    term.draw(|f| view(&mut app, f)).unwrap();
    let s = screen(&term);
    let title = s
        .iter()
        .position(|l| l.contains("Edit file"))
        .expect("title row");
    assert!(s[title].contains("+1 −1"), "{}", s[title]);
    assert_eq!(s[title + 1].trim(), "src/a.rs");
    // the question and the choices pinned over the footer's blank row
    assert!(s[18].starts_with(" ╌╌╌"), "{}", s[18]);
    assert_eq!(s[19].trim_end(), " Do you want to make this edit to a.rs?");
    assert!(s[20].starts_with(" ❯ 1. Yes"), "{}", s[20]);
    assert!(s[21].starts_with("   2. No"), "{}", s[21]);
    assert!(
        s.iter().any(|l| l.contains("- ") && l.contains("hi();")),
        "{s:?}"
    );
    assert!(
        s.iter().any(|l| l.contains("+ ") && l.contains("bye();")),
        "{s:?}"
    );
    assert!(s[23].contains("esc cancel turn"), "{}", s[23]);
    let buf = term.backend().buffer();
    // one number, the file's line, then the sign: the removed row on
    // `alert-bg` with its sign in `alert`, the added one on `ok-bg` with its
    // sign in `ok`, both out to the right edge; the context row has no
    // background
    let row = |needle: &str| s.iter().position(|l| l.contains(needle)).unwrap();
    let minus = row("hi();");
    let plus = row("bye();");
    assert!(
        s[minus].trim_start().starts_with("2 -     hi();"),
        "{}",
        s[minus]
    );
    assert!(
        s[plus].trim_start().starts_with("2 +     bye();"),
        "{}",
        s[plus]
    );
    let at = |line: usize, needle: &str| (s[line].find(needle).unwrap() as u16, line as u16);
    assert_eq!(buf[at(minus, "-")].fg, app.theme.alert);
    assert_eq!(buf[at(minus, "hi();")].bg, app.theme.alert_bg);
    assert_eq!(buf[at(plus, "+")].fg, app.theme.ok);
    assert_eq!(buf[at(plus, "bye();")].bg, app.theme.ok_bg);
    assert_eq!(buf[(85, minus as u16)].bg, app.theme.alert_bg);
    // the code keeps its colors: `fn` is a keyword, in `moon-soft`
    let context = row("fn main()");
    assert_eq!(buf[at(context, "fn")].fg, app.theme.moon_soft);
    assert_ne!(buf[at(context, "fn")].bg, app.theme.ok_bg);
}

#[test]
fn a_long_applied_diff_is_cut_in_the_conversation() {
    use moon_agent::{tools, Outcome, Step, Tool};
    let mut app = app();
    app.loading = false;
    app.items.clear();
    let before: String = (1..=40).map(|i| format!("old {i}\n")).collect();
    let after: String = (1..=40).map(|i| format!("new {i}\n")).collect();
    let diff = tools::diff(&before, &after);
    app.push_item(Item::Message(Message::new(Role::User, "rewrite it")));
    app.push_item(Item::Step(Step {
        tool: Tool::WriteFile,
        path: "notes.txt".into(),
        command: None,
        added: diff.added,
        removed: diff.removed,
        outcome: Outcome::Applied,
        diff: Some(diff),
    }));
    let lines: Vec<String> = app
        .visible_lines(80, 60)
        .iter()
        .map(|l| l.to_string())
        .collect();
    let at = lines.iter().position(|l| l.contains("✎ write")).unwrap();
    // 30 rows of the 80, then how many are left
    assert!(lines[at + 1].starts_with("       1 - old 1"), "{lines:?}");
    assert_eq!(lines[at + 31].trim(), "… 50 more lines", "{lines:?}");
}

#[test]
fn the_run_approval_panel() {
    use crate::app::{Approval, Panel};
    use moon_agent::{Exec, Pending};
    let mut app = app();
    app.loading = false;
    app.tools_on = true;
    let exec = Exec {
        id: "git commit",
        name: "git commit".into(),
        program: "/usr/bin/git".into(),
        args: vec!["commit".into(), "-m".into(), "fix: a".into()],
        dir: "/p/src".into(),
        rel_dir: "src".into(),
        asks: true,
        help: "commit what is staged",
        line: "git commit -m \"fix: a\"".into(),
    };
    app.panel = Some(Panel::Approval(Box::new(Approval::new(Pending::Run(exec)))));
    let mut term = Terminal::new(TestBackend::new(90, 24)).unwrap();
    term.draw(|f| view(&mut app, f)).unwrap();
    let s = screen(&term);
    let title = s
        .iter()
        .position(|l| l.contains("Run command"))
        .expect("title row");
    // the folder on the right, the command under it, the line and the help
    // below, the question over the choices
    assert!(s[title].contains("in src"), "{}", s[title]);
    assert_eq!(s[title + 1].trim(), "git commit");
    assert!(
        s.iter()
            .any(|l| l.trim_end() == " Do you want to run git commit?"),
        "{s:?}"
    );
    assert!(s.iter().any(|l| l.starts_with(" ❯ 1. Yes")), "{s:?}");
    assert!(
        s.iter().any(|l| l.contains("$ git commit -m \"fix: a\"")),
        "{s:?}"
    );
    assert!(s.iter().any(|l| l.contains("in src")), "{s:?}");
    assert!(
        s.iter().any(|l| l.contains("commit what is staged")),
        "{s:?}"
    );
    assert!(
        s[23].contains("1/2 yes/no") && s[23].contains("esc cancel turn"),
        "{}",
        s[23]
    );
}
