//! Rendering tests on a `TestBackend`: what each row of the screen shows.

use std::sync::Arc;

use moon_core::{Config, ConfigSource, Registry};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

use super::*;
use crate::app::{Action, HelpState, HelpTab, RunOptions};
use crossterm::event::KeyCode;

fn app() -> App {
    app_at(std::env::temp_dir())
}

/// The app with the project standing on `root`, for what reads the tree.
fn app_at(root: std::path::PathBuf) -> App {
    App::new(RunOptions {
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
    })
}

fn screen(term: &Terminal<TestBackend>) -> Vec<String> {
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
fn startup_screen() {
    let mut app = app();
    app.loading = false;
    let mut term = Terminal::new(TestBackend::new(90, 24)).unwrap();
    term.draw(|f| view(&mut app, f)).unwrap();
    let s = screen(&term);
    // a blank row keeps the moon off the top edge
    assert_eq!(s[0].trim(), "");
    assert!(s[1].starts_with("   ▄█        moon v0.1.0"), "{}", s[1]);
    assert!(
        s[2].starts_with("  ███        no model · /model"),
        "{}",
        s[2]
    );
    assert!(
        s[3].starts_with("  ████▄▄▄█   ~/Towerforge/moon"),
        "{}",
        s[3]
    );
    assert!(
        s[4].starts_with("   ▀████▀    default configuration"),
        "{}",
        s[4]
    );
    assert_eq!(s[5].trim(), "");
    assert!(s[20].starts_with("─────"));
    assert!(s[21].starts_with("❯ "));
    assert!(s[23].contains("/help"));
    let buf = term.backend().buffer();
    // the moon's color is `moon`, and the name `moon` goes in `moon` and bold
    assert_eq!(buf[(3, 1)].fg, app.theme.moon);
    assert_eq!(buf[(13, 1)].fg, app.theme.moon);
    assert!(buf[(13, 1)]
        .modifier
        .contains(ratatui::style::Modifier::BOLD));
    assert_eq!(buf[(18, 1)].fg, app.theme.ink_muted);
    // the cursor is a `moon` block on the typing cell, and the terminal's stays hidden
    assert_eq!(buf[(2, 21)].bg, app.theme.moon);
    assert_eq!(buf[(2, 21)].fg, app.theme.on_moon);
}

#[test]
fn the_model_row_is_dimmed() {
    use crate::app::{Current, ProviderState};
    let mut app = app();
    app.loading = false;
    app.current = Some(Current {
        provider: "ollama".into(),
        model: "qwen2.5-coder:14b".into(),
    });
    app.providers.push(ProviderState {
        id: "ollama".into(),
        kind: "ollama",
        base_url: "http://localhost:11434".into(),
        health: None,
    });
    app.ctx_len = Some(32_768);
    let mut term = Terminal::new(TestBackend::new(90, 24)).unwrap();
    term.draw(|f| view(&mut app, f)).unwrap();
    let s = screen(&term);
    let row = "qwen2.5-coder:14b (ctx 32.8k) · ollama · http://localhost:11434";
    assert!(s[2].starts_with(&format!("  ███        {row}")), "{}", s[2]);
    // the whole model row goes in `ink-muted`, name included
    let buf = term.backend().buffer();
    for x in 13..13 + row.chars().count() {
        assert_eq!(buf[(x as u16, 2)].fg, app.theme.ink_muted, "column {x}");
    }
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
    assert!(s.contains("1-15/27"), "{s}");
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
    assert!(hints.starts_with(" esc to cancel"), "{hints}");
    assert!(hints.trim_end().ends_with(" m"), "{hints}");
}

#[test]
fn machine_readings_at_the_bottom_right() {
    use crate::app::Current;
    use crate::sysmon::Sample;
    let mut app = app();
    app.loading = false;
    app.current = Some(Current {
        provider: "ollama".into(),
        model: "m".into(),
    });
    app.sys.push(Sample::new(61.0, 24 << 30, 32 << 30, 0));
    app.sys.push(Sample::new(34.0, 18 << 30, 32 << 30, 0));
    let hints_at = |app: &mut App, cols: u16| {
        let mut term = Terminal::new(TestBackend::new(cols, 12)).unwrap();
        term.draw(|f| view(app, f)).unwrap();
        screen(&term)[11].trim_end().to_string()
    };
    // with width to spare: the current value and the window's peak
    let h = hints_at(&mut app, 130);
    assert!(h.ends_with("m · cpu 34% ▲61 · ram 56% ▲75"), "{h}");
    // below 120 columns the peaks drop off
    let h = hints_at(&mut app, 100);
    assert!(h.ends_with("m · cpu 34% · ram 56%"), "{h}");
    // and below 90, the readings altogether
    let h = hints_at(&mut app, 80);
    assert!(h.ends_with(" m"), "{h}");
    // swap only shows up if there is any
    app.sys
        .push(Sample::new(34.0, 18 << 30, 32 << 30, 1288490189));
    let h = hints_at(&mut app, 130);
    assert!(h.ends_with("· ram 56% ▲75 · swap 1.2G"), "{h}");
    // and the gpu, with a card to read, between the ram and the swap
    app.sys
        .push(Sample::new(34.0, 18 << 30, 32 << 30, 1288490189).with_gpu(6 << 30, 8 << 30));
    let h = hints_at(&mut app, 130);
    assert!(
        h.ends_with("· ram 56% ▲75 · gpu 75% ▲75 · swap 1.2G"),
        "{h}"
    );
    let h = hints_at(&mut app, 100);
    assert!(h.ends_with("· ram 56% · gpu 75% · swap 1.2G"), "{h}");
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
    assert!(h.ends_with("m · cpu 34% ▲34 · ram 56% ▲56"), "{h}");
    // loaded and fully on the GPU: the size, with no split
    app.loaded = model(13_000_000_000, 13_000_000_000);
    let h = hints_at(&mut app, 130);
    assert!(h.ends_with("m · 12.1G · cpu 34% ▲34 · ram 56% ▲56"), "{h}");
    // partly off the GPU: it is flagged, because generation crawls
    app.loaded = model(13_000_000_000, 9_100_000_000);
    let h = hints_at(&mut app, 130);
    assert!(h.contains("m · 12.1G · 30% cpu · cpu 34%"), "{h}");
    // on a narrow terminal the size drops off along with the peaks
    let h = hints_at(&mut app, 100);
    assert!(h.ends_with("m · cpu 34% · ram 56%"), "{h}");
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
    // row 6 = "one two four"; select "two"
    app.selection = Some(Selection {
        anchor: (6, 4),
        head: (6, 6),
        dragging: false,
    });
    term.draw(|f| view(&mut app, f)).unwrap();
    let buf = term.backend().buffer();
    assert_eq!(buf[(4, 6)].bg, app.theme.moon);
    assert_eq!(buf[(6, 6)].bg, app.theme.moon);
    assert_ne!(buf[(7, 6)].bg, app.theme.moon);
    assert_ne!(buf[(3, 6)].bg, app.theme.moon);
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
    assert!(s.contains("tab or enter to complete"));
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
    use moon_agent::{tools, Eol, PendingEdit, Tool};
    let mut app = app();
    app.loading = false;
    app.tools_on = true;
    let mut term = Terminal::new(TestBackend::new(90, 24)).unwrap();
    term.draw(|f| view(&mut app, f)).unwrap();
    let s = screen(&term);
    // the sign sits on the bottom row, on the left, listing what is on; the
    // welcome banner does not repeat it
    assert!(
        s[23].trim_start().starts_with("⏵⏵ Read · Edit · Create"),
        "{}",
        s[23]
    );
    assert!(s[23].trim_end().ends_with("no model"), "{}", s[23]);
    assert!(!s.iter().any(|l| l.contains("✎ edits on")), "{s:?}");

    let before = "fn main() {\n    hi();\n}\n";
    let after = "fn main() {\n    bye();\n}\n";
    let edit = PendingEdit {
        tool: Tool::EditFile,
        path: "src/a.rs".into(),
        before: before.into(),
        after: after.into(),
        eol: Eol::Lf,
        bom: false,
        expect: Some(1),
        diff: tools::diff(before, after),
    };
    app.panel = Some(Panel::Approval(Box::new(Approval::new(edit))));
    term.draw(|f| view(&mut app, f)).unwrap();
    let s = screen(&term);
    let title = s
        .iter()
        .position(|l| l.contains("Edit src/a.rs"))
        .expect("title row");
    assert!(s[title].contains("+1 −1"), "{}", s[title]);
    assert!(
        s[title + 1].contains(" Apply ") && s[title + 1].contains(" Skip "),
        "{}",
        s[title + 1]
    );
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
    // the removed line in `alert`, the added one in `ok`
    let row = |needle: &str| s.iter().position(|l| l.contains(needle)).unwrap() as u16;
    let col = |line: &str, needle: &str| line.find(needle).unwrap() as u16;
    let minus = row("hi();");
    assert_eq!(
        buf[(col(&s[minus as usize], "hi();"), minus)].fg,
        app.theme.alert
    );
    let plus = row("bye();");
    assert_eq!(
        buf[(col(&s[plus as usize], "bye();"), plus)].fg,
        app.theme.ok
    );
}

#[test]
fn the_tools_panel() {
    use crate::app::{Panel, ToolsDialog};
    let mut app = app();
    app.loading = false;
    app.panel = Some(Panel::Tools(ToolsDialog {
        on: true,
        edit: true,
        create: false,
        rounds: 8,
        row: ToolsDialog::CREATE,
    }));
    let mut term = Terminal::new(TestBackend::new(90, 24)).unwrap();
    term.draw(|f| view(&mut app, f)).unwrap();
    let s = screen(&term);
    let title = s
        .iter()
        .position(|l| l.contains("Let the model use files?"))
        .expect("title row");
    assert!(s[title].contains("~/Towerforge/moon"), "{}", s[title]);
    assert!(
        s[title + 1].contains("only under this directory"),
        "{}",
        s[title + 1]
    );
    let row = |needle: &str| {
        s.iter()
            .find(|l| l.contains(needle))
            .unwrap_or_else(|| panic!("no row with {needle}: {s:?}"))
            .clone()
    };
    assert!(row("Read files").trim_end().ends_with("[✓]"));
    assert!(row("Edit existing files").trim_end().ends_with("[✓]"));
    let create = row("Create new files");
    assert!(
        create.starts_with(" ❯ ") && create.trim_end().ends_with("[ ]"),
        "{create}"
    );
    assert!(row("Max steps per message").trim_end().ends_with("◀ 8 ▶"));
    // the line under the rows explains the one the cursor is on
    assert!(s.iter().any(|l| l.contains("proposes a new file")), "{s:?}");
    assert!(!s.iter().any(|l| l.contains("before it has to answer")));
    assert!(!s.iter().any(|l| l.contains("Continue")), "{s:?}");
    assert!(
        s[23].contains("enter tick") && s[23].contains("esc save"),
        "{}",
        s[23]
    );
}
