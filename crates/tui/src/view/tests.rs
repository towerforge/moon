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
        config_source: ConfigSource::Default,
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
fn pantalla_de_arranque() {
    let mut app = app();
    app.loading = false;
    let mut term = Terminal::new(TestBackend::new(90, 24)).unwrap();
    term.draw(|f| view(&mut app, f)).unwrap();
    let s = screen(&term);
    assert!(s[0].starts_with("   ▄█        moon v0.1.0"), "{}", s[0]);
    assert!(
        s[1].starts_with("  ███        no model · /model"),
        "{}",
        s[1]
    );
    assert!(
        s[2].starts_with("  ████▄▄▄█   ~/Towerforge/moon"),
        "{}",
        s[2]
    );
    assert!(
        s[3].starts_with("   ▀████▀    default configuration"),
        "{}",
        s[3]
    );
    assert_eq!(s[4].trim(), "");
    assert!(s[20].starts_with("─────"));
    assert!(s[21].starts_with("❯ "));
    assert!(s[23].contains("/help"));
    let buf = term.backend().buffer();
    // the moon's color is `moon`, and the name `moon` goes in `moon` and bold
    assert_eq!(buf[(3, 0)].fg, app.theme.moon);
    assert_eq!(buf[(13, 0)].fg, app.theme.moon);
    assert!(buf[(13, 0)]
        .modifier
        .contains(ratatui::style::Modifier::BOLD));
    assert_eq!(buf[(18, 0)].fg, app.theme.ink_muted);
    // the cursor is a `moon` block on the typing cell, and the terminal's stays hidden
    assert_eq!(buf[(2, 21)].bg, app.theme.moon);
    assert_eq!(buf[(2, 21)].fg, app.theme.on_moon);
}

#[test]
fn fila_del_modelo_atenuada() {
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
    assert!(s[1].starts_with(&format!("  ███        {row}")), "{}", s[1]);
    // the whole model row goes in `ink-muted`, name included
    let buf = term.backend().buffer();
    for x in 13..13 + row.chars().count() {
        assert_eq!(buf[(x as u16, 1)].fg, app.theme.ink_muted, "columna {x}");
    }
}

#[test]
fn selector_de_modelo_agrupado() {
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
    // cursor marks one with `❯` and the model in use closes with `✓`; being
    // both, the name goes in moon and bold
    let buf = term.backend().buffer();
    let y = rows
        .iter()
        .position(|r| r.contains("❯  2. model-03 ✓"))
        .unwrap() as u16;
    let row = &rows[y as usize];
    let x = row[..row.find("model-03").unwrap()].chars().count() as u16;
    assert_eq!(buf[(x, y)].style().fg, Some(app.theme.moon));
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
fn dialogos_de_sesion() {
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
            title: "explica el módulo de sesiones".into(),
            choice: Choice::Delete,
            open: false,
        },
    });
    let mut term = Terminal::new(TestBackend::new(90, 26)).unwrap();
    term.draw(|f| view(&mut app, f)).unwrap();
    let rows = screen(&term);
    let s = rows.join("\n");
    assert!(s.contains(" Delete session"), "{s}");
    assert!(s.contains("«explica el módulo de sesiones»"), "{s}");
    // two options under the cursor, like any other list; no buttons
    assert!(s.contains(" ❯ Delete"), "{s}");
    assert!(s.contains("   Keep"), "{s}");
    assert!(s.contains("↑↓ choose · enter confirm · esc keep"), "{s}");
    // the option under the cursor goes in ink and bold; the other one does not
    let buf = term.backend().buffer();
    let y = rows.iter().position(|r| r.contains("❯ Delete")).unwrap();
    let yk = rows.iter().position(|r| r.contains("Keep")).unwrap();
    let x = rows[y][..rows[y].find("Delete").unwrap()].chars().count() as u16;
    assert!(buf[(x, y as u16)]
        .modifier
        .contains(ratatui::style::Modifier::BOLD));
    assert_eq!(buf[(x, y as u16)].style().fg, Some(app.theme.ink));
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
            input: "nuevo título".into(),
        },
    });
    term.draw(|f| view(&mut app, f)).unwrap();
    let s = screen(&term).join("\n");
    assert!(s.contains(" Rename session"), "{s}");
    assert!(s.contains("❯ nuevo título█"), "{s}");
    assert!(s.contains("enter save · esc cancel"), "{s}");
}

#[test]
fn indicador_de_ir_al_final() {
    use crate::app::Item;
    let mut app = app();
    app.loading = false;
    for i in 0..40 {
        app.items.push(Item::Info(format!("línea {i}")));
    }
    let mut term = Terminal::new(TestBackend::new(80, 20)).unwrap();
    term.draw(|f| view(&mut app, f)).unwrap();
    assert!(
        app.jump_rect.is_none(),
        "siguiendo el final no hay indicador"
    );
    app.scroll_by(-5);
    term.draw(|f| view(&mut app, f)).unwrap();
    let s = screen(&term);
    let rect = app.jump_rect.expect("indicador visible al subir");
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
fn actividad_a_la_izquierda_y_estado_a_la_derecha() {
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
fn mediciones_de_la_maquina_abajo_a_la_derecha() {
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
}

#[test]
fn el_tamano_del_modelo_cargado_va_tras_su_nombre() {
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
fn la_seleccion_se_resalta() {
    use crate::app::{Item, Selection};
    let mut app = app();
    app.loading = false;
    app.items.clear(); // with no providers, App::new leaves an error in the conversation
    app.items.push(Item::Info("uno dos tres".into()));
    let mut term = Terminal::new(TestBackend::new(60, 14)).unwrap();
    term.draw(|f| view(&mut app, f)).unwrap();
    // row 5 = "uno dos tres"; select "dos"
    app.selection = Some(Selection {
        anchor: (5, 4),
        head: (5, 6),
        dragging: false,
    });
    term.draw(|f| view(&mut app, f)).unwrap();
    let buf = term.backend().buffer();
    assert_eq!(buf[(4, 5)].bg, app.theme.moon);
    assert_eq!(buf[(6, 5)].bg, app.theme.moon);
    assert_ne!(buf[(7, 5)].bg, app.theme.moon);
    assert_ne!(buf[(3, 5)].bg, app.theme.moon);
    assert_eq!(app.selection_text(), "dos");
}

#[test]
fn el_recuento_de_ficheros_va_en_la_fila_de_estado() {
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
fn panel_de_ficheros_y_arbol() {
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
        .expect("pie del panel");
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
fn panel_de_ayuda() {
    let mut app = app();
    app.panel = Some(Panel::Help(HelpState::default()));
    let mut term = Terminal::new(TestBackend::new(100, 70)).unwrap();
    term.draw(|f| view(&mut app, f)).unwrap();
    let rows = screen(&term);
    let s = rows.join("\n");
    // the title row carries the sections, and it opens on the first one
    let title = rows.iter().find(|r| r.contains("Help")).expect("título");
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
    let footer = rows
        .iter()
        .find(|r| r.contains("esc close"))
        .expect("pie del panel");
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
    let title = rows.iter().find(|r| r.contains("Help")).expect("título");
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
    let title = rows.iter().find(|r| r.contains("Help")).expect("título");
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
        .expect("la fila de /params");
    let col = rows[head][..rows[head].find("generation").unwrap()]
        .chars()
        .count();
    let cont = &rows[head + 1];
    let indent = cont.chars().take_while(|c| *c == ' ').count();
    assert!(
        !cont.trim().is_empty(),
        "la descripción de /params se parte"
    );
    assert_eq!(indent, col, "{cont}");
    assert!(col >= 10, "{cont}");
}

#[test]
fn la_ayuda_hace_scroll() {
    let mut app = app();
    app.panel = Some(Panel::Help(HelpState {
        tab: HelpTab::Commands,
        ..Default::default()
    }));
    let mut term = Terminal::new(TestBackend::new(90, 24)).unwrap();
    term.draw(|f| view(&mut app, f)).unwrap();
    let s = screen(&term).join("\n");
    assert!(s.contains("/provider [id]"));
    assert!(!s.contains("/quit"), "los comandos no caben sin scroll");
    assert!(s.contains("↑↓ scroll · esc close"));
    assert!(s.contains("1-"), "posición en el pie");
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
fn sugerencias_sobre_la_caja() {
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
    // the first one carries the cursor and goes in moon and bold; the second
    // one is plain moon
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
    assert_eq!(buf[(4, y_model)].style().fg, Some(app.theme.moon));
    assert!(bold(4, y_model));
    assert_eq!(buf[(4, y_models)].style().fg, Some(app.theme.moon));
    assert!(!bold(4, y_models));
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
