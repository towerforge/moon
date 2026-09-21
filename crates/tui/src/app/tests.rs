//! Tests of the application state: keys, commands, streaming, pickers and the mouse.

use super::models::load_recent;
use super::render::slice_columns;
use super::*;

pub(super) fn app() -> (App, Tx, mpsc::UnboundedReceiver<Action>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let app = App::new(RunOptions {
        config: Config::default(),
        config_source: ConfigSource::Default,
        registry: Arc::new(Registry::new()),
        store: None,
        resume: None,
        model: None,
        version: "0.0.0".into(),
        cwd: "~/x".into(),
        root: std::env::temp_dir(),
        state_dir: None,
    });
    (app, tx, rx)
}

pub(super) fn key(code: KeyCode) -> Action {
    Action::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

pub(super) fn ctrl(c: char) -> Action {
    Action::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
}

pub(super) fn type_text(app: &mut App, tx: &Tx, s: &str) {
    for c in s.chars() {
        app.update(key(KeyCode::Char(c)), tx);
    }
}

#[test]
fn sampling_runs_fast_while_the_model_works() {
    let (mut app, tx, _rx) = app();
    assert!(!app.sys_pace.is_fast());
    app.gen = Generation::Streaming {
        cancel: tokio_util::sync::CancellationToken::new(),
        started: Instant::now(),
        first_at: None,
        deltas: 0,
        sent: 0,
    };
    app.update(Action::Tick, &tx);
    assert!(app.sys_pace.is_fast());
    app.gen = Generation::Idle;
    app.update(Action::Tick, &tx);
    assert!(!app.sys_pace.is_fast());
}

#[tokio::test]
async fn basic_commands() {
    let (mut app, tx, _rx) = app();
    type_text(&mut app, &tx, "/help");
    app.update(key(KeyCode::Enter), &tx);
    assert!(matches!(app.panel, Some(Panel::Help(_))));
    app.update(key(KeyCode::Esc), &tx);
    assert!(app.panel.is_none());

    type_text(&mut app, &tx, "/nada");
    app.update(key(KeyCode::Enter), &tx);
    assert!(app
        .notice
        .as_ref()
        .is_some_and(|(n, _)| n.contains("unknown")));

    type_text(&mut app, &tx, "/params temperature=0.3");
    app.update(key(KeyCode::Enter), &tx);
    assert_eq!(app.params.temperature, Some(0.3));

    type_text(&mut app, &tx, "/mod");
    app.update(key(KeyCode::Tab), &tx);
    assert_eq!(app.input.text(), "/model ");
    app.input.clear();
    type_text(&mut app, &tx, "/qu");
    app.update(key(KeyCode::Tab), &tx);
    assert_eq!(app.input.text(), "/quit ");
    app.update(key(KeyCode::Enter), &tx);
    assert!(app.should_quit);
}

#[tokio::test]
async fn with_no_model_it_does_not_send_and_keeps_the_text() {
    let (mut app, tx, _rx) = app();
    type_text(&mut app, &tx, "hello");
    app.update(key(KeyCode::Enter), &tx);
    assert_eq!(app.input.text(), "hello");
    assert!(app.messages().next().is_none());
}

#[tokio::test]
async fn streaming_undo_and_render() {
    let (mut app, tx, _rx) = app();
    app.current = Some(Current {
        provider: "p".into(),
        model: "m".into(),
    });
    app.loading = false;
    app.gen_id = 7;
    app.push_item(Item::Message(Message::user("hello")));
    app.gen = Generation::Streaming {
        cancel: CancellationToken::new(),
        started: Instant::now(),
        first_at: None,
        deltas: 0,
        sent: 30,
    };
    app.update(
        Action::Stream(7, StreamEvent::Delta("# Hello\n\nhow are ".into())),
        &tx,
    );
    app.update(Action::Stream(7, StreamEvent::Delta("you".into())), &tx);
    app.update(Action::Stream(1, StreamEvent::Delta("stale".into())), &tx); // stale id: ignored
    assert_eq!(
        app.messages().last().unwrap().content,
        "# Hello\n\nhow are you"
    );
    let streaming: String = app
        .activity_spans()
        .unwrap()
        .iter()
        .map(|s| s.content.to_string())
        .collect();
    assert!(
        streaming.contains("generating (0s · ↑ ~30 · ↓ 2 tokens · "),
        "{streaming}"
    );
    app.update(
        Action::Stream(
            7,
            StreamEvent::Done(Usage {
                prompt_tokens: Some(30),
                completion_tokens: Some(3),
                total_duration_ms: Some(1_500),
                ..Default::default()
            }),
        ),
        &tx,
    );
    assert!(!app.is_streaming());
    // once done the summary stays, with the provider's real count
    let summary: String = app
        .activity_spans()
        .unwrap()
        .iter()
        .map(|s| s.content.to_string())
        .collect();
    assert!(
        summary.starts_with("✓ done (0s · ↑ 30 · ↓ 3 tokens · "),
        "{summary}"
    );
    assert!(summary.ends_with(" tok/s)"), "{summary}");
    // the check mark goes in `ok`
    assert_eq!(
        app.activity_spans().unwrap()[0].style.fg,
        Some(app.theme.ok)
    );
    let status: String = app
        .status_spans()
        .iter()
        .map(|s| s.content.to_string())
        .collect();
    assert_eq!(status, "context 33 tok · session 33 tok (1s · ↑ 30 · ↓ 3)");
    assert_eq!(app.session_tokens_split(), (30, 3));
    assert_eq!(app.session_time(), Duration::from_millis(1_500));
    app.ctx_len = Some(40);
    let status: String = app
        .status_spans()
        .iter()
        .map(|s| s.content.to_string())
        .collect();
    assert_eq!(status, "context 82% · session 33 tok (1s · ↑ 30 · ↓ 3)");
    assert_eq!(app.status_spans()[0].style.fg, Some(app.theme.moon_soft));
    let model: String = app
        .model_spans()
        .iter()
        .map(|s| s.content.to_string())
        .collect();
    // only the model name: no status dot, no provider
    assert_eq!(model, "m");
    assert_eq!(
        app.messages()
            .last()
            .unwrap()
            .usage
            .as_ref()
            .unwrap()
            .completion_tokens,
        Some(3)
    );

    let lines = app.visible_lines(60, 40);
    let text: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    assert!(text[1].contains("moon"));
    assert!(text.iter().any(|l| l == "▌ hello"));
    assert!(text.iter().any(|l| l == "Hello"));
    assert!(text.iter().any(|l| l == "how are you"));
    // the request opens the turn with a full-width divider in night-line
    // and carries the moon bar in front of each line
    let at = text.iter().position(|l| l == "▌ hello").unwrap();
    assert_eq!(lines[at].spans[0].style.fg, Some(app.theme.moon));
    assert_eq!(text[at - 1], "─".repeat(60));
    assert_eq!(lines[at - 1].spans[0].style.fg, Some(app.theme.night_line));
    assert_eq!(text[at - 2], "");
    // the reply has no divider
    let at = text.iter().position(|l| l == "Hello").unwrap();
    assert!(!text[at - 1].contains('─'));
    assert_eq!(app.total_lines, text.len());

    type_text(&mut app, &tx, "/undo");
    app.update(key(KeyCode::Enter), &tx);
    assert!(app.messages().next().is_none());
}

#[tokio::test]
async fn scroll_and_history() {
    let (mut app, tx, _rx) = app();
    for i in 0..30 {
        app.push_item(Item::Info(format!("line {i}")));
    }
    let _ = app.visible_lines(60, 10);
    assert!(app.follow);
    app.scroll_by(-5);
    assert!(!app.follow);
    assert_eq!(app.scroll_offset, app.total_lines - 10 - 5);
    app.scroll_by(100);
    assert!(app.follow);
    app.scroll_by(-3);
    assert_eq!(app.lines_below(), 3);
    app.jump_rect = Some(ratatui::layout::Rect::new(10, 8, 20, 1));
    app.update(Action::MouseDown(5, 8), &tx);
    assert!(!app.follow);
    app.update(Action::MouseDown(15, 8), &tx);
    assert!(app.follow);

    type_text(&mut app, &tx, "/clear");
    app.update(key(KeyCode::Enter), &tx);
    // blank row + medium moon (4 rows); the default-configuration notice goes
    // on the fourth
    assert_eq!(app.visible_lines(60, 10).len(), 5);
    app.update(key(KeyCode::Up), &tx);
    assert_eq!(app.input.text(), "/clear");
    app.update(key(KeyCode::Down), &tx);
    assert_eq!(app.input.text(), "");
}

#[tokio::test]
async fn command_suggestions() {
    let (mut app, tx, _rx) = app();
    assert!(app.suggestions().is_none());
    type_text(&mut app, &tx, "/");
    let (specs, sel) = app.suggestions().unwrap();
    assert_eq!((specs.len(), sel), (commands::SPECS.len(), 0));
    assert_eq!(app.command_span(), 0);
    // enter on a prefix of a command without arguments runs it
    type_text(&mut app, &tx, "hel");
    assert_eq!(app.suggestions().unwrap().0[0].name, "help");
    assert_eq!(app.command_span(), 4);
    assert!(app.hints().contains("tab or enter"));
    app.update(key(KeyCode::Enter), &tx);
    assert!(matches!(app.panel, Some(Panel::Help(_))));
    assert!(app.suggestions().is_none()); // no list while a panel is open
    app.update(key(KeyCode::Esc), &tx);
    assert_eq!(app.input.text(), "");

    // ↓ walks the list and tab completes the highlighted one
    type_text(&mut app, &tx, "/s");
    assert_eq!(app.suggestions().unwrap().0.len(), 3); // system, sessions, save
    app.update(key(KeyCode::Down), &tx);
    assert_eq!(app.suggestions().unwrap().1, 1);
    app.update(key(KeyCode::Down), &tx);
    app.update(key(KeyCode::Down), &tx); // wraps around
    assert_eq!(app.suggestions().unwrap().1, 0);
    app.update(key(KeyCode::Up), &tx);
    assert_eq!(app.suggestions().unwrap().1, 2);
    app.update(key(KeyCode::Tab), &tx);
    assert_eq!(app.input.text(), "/save ");
    assert!(app.suggestions().is_none()); // there is a space now
    assert_eq!(app.command_span(), 5);
    app.update(key(KeyCode::Esc), &tx);

    // typing further resets the selection to the first one
    type_text(&mut app, &tx, "/s");
    app.update(key(KeyCode::Down), &tx);
    type_text(&mut app, &tx, "a");
    assert_eq!(app.suggestions().unwrap().1, 0);
    // enter on a command with arguments only completes it
    app.update(key(KeyCode::Enter), &tx);
    assert_eq!(app.input.text(), "/save ");
    assert!(app.panel.is_none());
    app.update(key(KeyCode::Esc), &tx);

    // an exact alias is sent as is
    type_text(&mut app, &tx, "/q");
    assert_eq!(app.suggestions().unwrap().0[0].name, "quit");
    app.update(key(KeyCode::Enter), &tx);
    assert!(app.should_quit);
    app.should_quit = false;

    // while walking the history the arrows still belong to the history,
    // even if the input has several candidates
    app.history.push("/s".into());
    app.update(key(KeyCode::Up), &tx);
    assert_eq!(app.input.text(), "/s");
    assert_eq!(app.suggestions().unwrap().0.len(), 3);
    assert!(!app.suggest_navigable());
    assert!(!app.hints().contains("choose"));
    app.update(key(KeyCode::Up), &tx);
    assert_eq!(app.input.text(), "/q");
    app.update(key(KeyCode::Down), &tx);
    assert_eq!(app.input.text(), "/s");
    app.update(key(KeyCode::Down), &tx);
    assert_eq!(app.input.text(), "");

    // something that is not a command is neither highlighted nor suggested
    type_text(&mut app, &tx, "/zzz");
    assert!(app.suggestions().is_none());
    assert_eq!(app.command_span(), 0);
    app.update(key(KeyCode::Esc), &tx);
    type_text(&mut app, &tx, "text /help");
    assert_eq!(app.command_span(), 0);
}

#[tokio::test]
async fn grouped_picker_and_recents() {
    let (mut app, tx, _rx) = app();
    let dir = tempfile::tempdir().unwrap();
    app.recent_file = Some(dir.path().join("state").join(RECENT_MODELS_FILE));
    app.providers = vec![
        ProviderState {
            id: "ollama".into(),
            kind: "ollama",
            base_url: "http://localhost:11434/".into(),
            health: Some(Ok(Health::default())),
        },
        ProviderState {
            id: "openai".into(),
            kind: "openai",
            base_url: "https://api.openai.com/v1".into(),
            health: Some(Err("no API key".into())),
        },
    ];
    let model = |p: &str, id: &str| ModelInfo::new(p, id);
    app.models = vec![
        model("ollama", "qwen2.5-coder:14b"),
        model("ollama", "llama3.1:8b"),
    ];
    app.current = Some(Current {
        provider: "ollama".into(),
        model: "llama3.1:8b".into(),
    });
    // no recents: one section per provider, openai empty with its error
    let p = app.model_picker("");
    assert_eq!(p.title_info, "2 models · 2 providers");
    let titles: Vec<&str> = p.groups.iter().map(|g| g.title.as_str()).collect();
    assert_eq!(titles, vec!["ollama", "openai"]);
    assert_eq!(p.groups[0].info, "localhost:11434");
    assert_eq!(p.groups[0].mark, Some(true));
    assert_eq!(p.groups[1].info, "no API key");
    assert_eq!(p.groups[1].mark, Some(false));
    assert_eq!(p.rows().len(), 1 + 2 + 1 + 1); // header, 2 models, blank, header
    assert_eq!(p.current().unwrap().label, "llama3.1:8b"); // the active one starts selected

    // choosing a model is remembered and saved to disk, but with a handful of
    // models `Recent` would only repeat what is already in view
    app.set_model("ollama".into(), "qwen2.5-coder:14b".into(), &tx);
    assert_eq!(app.recent, vec!["ollama/qwen2.5-coder:14b"]);
    assert_eq!(
        std::fs::read_to_string(app.recent_file.as_ref().unwrap()).unwrap(),
        "ollama/qwen2.5-coder:14b"
    );
    let p = app.model_picker("");
    assert_eq!(p.groups[0].title, "ollama");
    assert!(p.groups.iter().all(|g| g.title != "Recent"));

    // from ten models on the list no longer fits at a glance and the section
    // earns its place, with the last one chosen on top and under the cursor
    for i in 0..10 {
        app.models.push(model("ollama", &format!("m{i}")));
    }
    let p = app.model_picker("");
    assert_eq!(p.groups[0].title, "Recent");
    assert_eq!(p.groups[0].info, "1 model");
    let first = p.rows();
    let crate::picker::Row::Item(i, selected, _) = &first[1] else {
        panic!("the recent one goes right after the header");
    };
    assert!(*selected && i.active && i.detail.starts_with("ollama"));
    // a recent that no longer exists is not shown, and the saved list is trimmed
    app.recent.insert(0, "ollama/deleted".into());
    assert_eq!(app.model_picker("").groups[0].info, "1 model");
    for i in 0..10 {
        app.set_model("ollama".into(), format!("m{i}"), &tx);
    }
    assert_eq!(app.recent.len(), RECENT_MAX);
    assert_eq!(load_recent(app.recent_file.as_deref()), app.recent);
    // the wheel moves the cursor one at a time and does not wrap around
    app.panel = Some(Panel::Models(app.model_picker("")));
    let sel = |app: &App| match &app.panel {
        Some(Panel::Models(p)) => p.selected,
        _ => unreachable!(),
    };
    let start = sel(&app);
    app.update(Action::ScrollBy(3), &tx);
    assert_eq!(sel(&app), start + 1);
    for _ in 0..20 {
        app.update(Action::ScrollBy(-3), &tx);
    }
    assert_eq!(sel(&app), 0);
    app.panel = None;
    // the model is not typed in: `/model` opens the list, and a name after it
    // changes nothing
    app.models.push(model("ollama", "qwen2.5-coder:14b"));
    let before = app.current.clone();
    type_text(&mut app, &tx, "/model qwen2.5-coder:14b");
    app.update(key(KeyCode::Enter), &tx);
    assert!(matches!(app.panel, Some(Panel::Models(_))));
    assert_eq!(app.current, before);
}

#[tokio::test]
async fn the_numbers_go_straight_through() {
    use moon_core::{Health, ModelInfo};
    let (mut app, tx, _rx) = app();
    app.providers = vec![ProviderState {
        id: "ollama".into(),
        kind: "ollama",
        base_url: "http://localhost:11434".into(),
        health: Some(Ok(Health::default())),
    }];
    app.models = vec![
        ModelInfo::new("ollama", "gemma3:12b"),
        ModelInfo::new("ollama", "llama3.1:8b"),
        ModelInfo::new("ollama", "qwen2.5-coder:14b"),
    ];
    let open = |app: &mut App| app.panel = Some(Panel::Models(app.model_picker("")));

    // the number is the row as it is drawn, sections aside, and it picks it
    open(&mut app);
    app.update(key(KeyCode::Char('2')), &tx);
    assert_eq!(app.current.as_ref().unwrap().model, "llama3.1:8b");
    assert!(app.panel.is_none());

    // with a filter being typed the digits are part of it: `3.1` is a model,
    // not a row
    open(&mut app);
    type_text(&mut app, &tx, "qwen2.5");
    let Some(Panel::Models(p)) = &app.panel else {
        panic!("the panel should still be open")
    };
    assert_eq!(p.query, "qwen2.5");
    assert_eq!(p.len(), 1);
    // and alt jumps anyway, filter or no filter
    app.update(
        Action::Key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::ALT)),
        &tx,
    );
    assert_eq!(app.current.as_ref().unwrap().model, "qwen2.5-coder:14b");
    assert!(app.panel.is_none());

    // a number nobody has does nothing, and it does not start a filter
    // either: with nothing typed the digits are shortcuts
    open(&mut app);
    app.update(key(KeyCode::Char('9')), &tx);
    let Some(Panel::Models(p)) = &app.panel else {
        panic!("the panel should still be open")
    };
    assert!(p.query.is_empty());
    // with three models there is no `Recent` to repeat them: `1` is the first
    // one of the provider's section
    open(&mut app);
    app.update(key(KeyCode::Char('1')), &tx);
    assert_eq!(app.current.as_ref().unwrap().model, "gemma3:12b");

    // with more rows than digits the number takes two: the first one moves
    // the cursor and waits, because a longer number still reaches the list
    for i in 0..12 {
        app.models
            .push(ModelInfo::new("ollama", format!("m{i:02}")));
    }
    // fifteen models: `Recent` is back on top, with the three picked so far,
    // and the provider's fifteen below
    open(&mut app);
    app.update(key(KeyCode::Char('1')), &tx);
    let Some(Panel::Models(p)) = &app.panel else {
        panic!("espera a la segunda cifra")
    };
    assert_eq!(p.pending, Some(1));
    assert_eq!(p.current().unwrap().label, "gemma3:12b");
    // enter settles what the number left under the cursor
    app.update(key(KeyCode::Enter), &tx);
    assert_eq!(app.current.as_ref().unwrap().model, "gemma3:12b");

    // the second digit lands on row 12 and, with no room for a third, picks it
    open(&mut app);
    app.update(key(KeyCode::Char('1')), &tx);
    app.update(key(KeyCode::Char('2')), &tx);
    assert_eq!(app.current.as_ref().unwrap().model, "m05");
    assert!(app.panel.is_none());

    // and a digit that no longer number can reach picks straight away
    open(&mut app);
    app.update(key(KeyCode::Char('4')), &tx);
    assert_eq!(app.current.as_ref().unwrap().model, "llama3.1:8b");
    assert!(app.panel.is_none());
}

#[tokio::test]
async fn session_picker_in_alphabetical_order() {
    use moon_core::session::SessionStore;
    let (mut app, tx, _rx) = app();
    let dir = tempfile::tempdir().unwrap();
    let store = SessionStore::new(dir.path());
    let zulu = store
        .create("zulu last", Some("ollama/m".into()), None)
        .unwrap();
    let alpha = store.create("alpha first", None, None).unwrap();
    let middle = store.create("Middle, capitalized", None, None).unwrap();
    app.store = Some(store);
    app.recent_sessions_file = Some(dir.path().join("state").join(RECENT_SESSIONS_FILE));
    app.session = Some(zulu.clone());
    app.update(
        Action::Key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
        &tx,
    );
    let Some(Panel::Sessions(p)) = &app.panel else {
        panic!("ctrl+s should open the session picker");
    };
    assert_eq!(p.title_info, "3 sessions");
    // with nothing opened yet there is a single section, in alphabetical
    // order and ignoring case
    let groups: Vec<(&str, &str)> = p
        .groups
        .iter()
        .map(|g| (g.title.as_str(), g.info.as_str()))
        .collect();
    assert_eq!(groups, vec![("All", "3 sessions")]);
    let labels: Vec<&str> = p.visible().map(|(i, _, _)| i.label.as_str()).collect();
    assert_eq!(
        labels,
        vec!["alpha first", "Middle, capitalized", "zulu last"]
    );
    // no dates anywhere: the detail is the model, or nothing
    let cur = p.current().unwrap();
    assert!(cur.active && cur.label == "zulu last");
    assert_eq!(cur.detail, "ollama/m");
    assert!(p.visible().all(|(i, _, _)| !i.detail.contains(':')));

    // opening one is remembered, but with three sessions `Recent` would only
    // repeat what is already in view
    app.load_session(&alpha.id, &tx);
    app.load_session(&middle.id, &tx);
    app.open_sessions_picker();
    let Some(Panel::Sessions(p)) = &app.panel else {
        panic!("the picker should still be open");
    };
    assert!(p.groups.iter().all(|g| g.title != "Recent"));
    // and the order is remembered anyway, run to run
    assert_eq!(
        load_recent(app.recent_sessions_file.as_deref()),
        vec![middle.id.clone(), alpha.id.clone()]
    );

    // from ten sessions on the list no longer fits at a glance and the
    // section earns its place, with the last one opened on top
    let store = app.store.as_ref().unwrap();
    for i in 0..7 {
        store.create(&format!("filler {i}"), None, None).unwrap();
    }
    app.open_sessions_picker();
    let Some(Panel::Sessions(p)) = &app.panel else {
        panic!("the picker should still be open");
    };
    let groups: Vec<(&str, &str)> = p
        .groups
        .iter()
        .map(|g| (g.title.as_str(), g.info.as_str()))
        .collect();
    assert_eq!(
        groups,
        vec![("Recent", "2 sessions"), ("All", "10 sessions")]
    );
    let labels: Vec<&str> = p
        .visible()
        .map(|(i, _, _)| i.label.as_str())
        .take(3)
        .collect();
    assert_eq!(
        labels,
        vec!["Middle, capitalized", "alpha first", "alpha first"]
    );
    // rows: header, 2, blank, header, 10
    assert_eq!(p.rows().len(), 15);
}

#[tokio::test]
async fn ctrl_d_and_ctrl_r_do_not_close_the_other_panels() {
    use moon_core::ModelInfo;
    let (mut app, tx, _rx) = app();
    app.models = vec![
        ModelInfo::new("ollama", "m1"),
        ModelInfo::new("ollama", "m2"),
    ];
    // delete and rename belong to the sessions list; anywhere else they do
    // nothing, and above all they do not close the panel
    app.panel = Some(Panel::Models(app.model_picker("")));
    app.update(ctrl('d'), &tx);
    assert!(matches!(app.panel, Some(Panel::Models(_))));
    app.update(key(KeyCode::Delete), &tx);
    assert!(matches!(app.panel, Some(Panel::Models(_))));
    app.update(ctrl('r'), &tx);
    assert!(matches!(app.panel, Some(Panel::Models(_))));
}

#[tokio::test]
async fn delete_and_rename_from_the_picker() {
    use moon_core::session::SessionStore;
    let (mut app, tx, _rx) = app();
    let dir = tempfile::tempdir().unwrap();
    let store = SessionStore::new(dir.path());
    let open = store.create("active", None, None).unwrap();
    let old = store.create("old", None, None).unwrap();
    app.store = Some(store);
    app.session = Some(open.clone());
    let list = |app: &App| app.store.as_ref().unwrap().list().unwrap();
    app.open_sessions_picker();
    // the cursor starts on the open one, and it can be deleted like any
    // other: the dialog says it is the one you are in
    app.update(ctrl('d'), &tx);
    let Some(Panel::SessionAction {
        action: SessionAction::Delete { open: is_open, .. },
        ..
    }) = &app.panel
    else {
        panic!("the delete dialog should be open");
    };
    assert!(*is_open);
    app.update(key(KeyCode::Esc), &tx);
    // on «old», ctrl+d opens the dialog with «Delete» highlighted; esc goes back
    app.update(key(KeyCode::Down), &tx);
    app.update(ctrl('d'), &tx);
    let Some(Panel::SessionAction { action, .. }) = &app.panel else {
        panic!("the delete dialog should be open");
    };
    assert_eq!(
        *action,
        SessionAction::Delete {
            id: old.id.clone(),
            title: "old".into(),
            choice: Choice::Delete,
            open: false,
        }
    );
    assert!(app.panel_keys().contains(&("esc", "keep")));
    app.update(key(KeyCode::Esc), &tx);
    assert!(matches!(app.panel, Some(Panel::Sessions(_))));
    assert_eq!(list(&app).len(), 2);
    // ↓ moves to «Keep» and enter keeps the session; Del, ↑ and enter delete it
    app.update(key(KeyCode::Delete), &tx);
    app.update(key(KeyCode::Down), &tx);
    app.update(key(KeyCode::Enter), &tx);
    assert_eq!(list(&app).len(), 2);
    assert!(matches!(app.panel, Some(Panel::Sessions(_))));
    app.update(key(KeyCode::Delete), &tx);
    app.update(key(KeyCode::Down), &tx);
    app.update(key(KeyCode::Up), &tx);
    app.update(key(KeyCode::Enter), &tx);
    assert_eq!(list(&app).len(), 1);
    assert!(app
        .notice
        .as_ref()
        .is_some_and(|(n, _)| n.contains("session deleted: old")));
    let Some(Panel::Sessions(p)) = &app.panel else {
        panic!("the picker should still be open");
    };
    assert_eq!((p.len(), p.title_info.as_str()), (1, "1 session"));

    // ctrl+r opens the rename with the current title; enter saves and
    // updates the open session as well
    app.update(ctrl('r'), &tx);
    let Some(Panel::SessionAction {
        action: SessionAction::Rename { input, .. },
        ..
    }) = &app.panel
    else {
        panic!("the rename dialog should be open");
    };
    assert_eq!(input, "active");
    assert!(app.panel_keys().contains(&("enter", "save")));
    type_text(&mut app, &tx, " and renamed");
    app.update(key(KeyCode::Enter), &tx);
    assert_eq!(list(&app)[0].title, "active and renamed");
    assert_eq!(app.session.as_ref().unwrap().title, "active and renamed");
    let Some(Panel::Sessions(p)) = &app.panel else {
        panic!("the picker should still be open");
    };
    assert_eq!(p.current().unwrap().label, "active and renamed");
    // empty does not save; esc cancels without touching anything
    app.update(ctrl('r'), &tx);
    app.update(ctrl('u'), &tx);
    app.update(key(KeyCode::Enter), &tx);
    assert!(matches!(app.panel, Some(Panel::SessionAction { .. })));
    app.update(key(KeyCode::Esc), &tx);
    assert_eq!(list(&app)[0].title, "active and renamed");
    assert!(matches!(app.panel, Some(Panel::Sessions(_))));

    // and the one you are in can be deleted too: the file goes and what is on
    // screen simply stops being saved
    app.update(ctrl('d'), &tx);
    app.update(key(KeyCode::Enter), &tx);
    assert!(app.session.is_none());
    assert!(app
        .notice
        .as_ref()
        .is_some_and(|(n, _)| n.contains("no longer saved")));
    assert!(list(&app).is_empty());
}

#[tokio::test]
async fn help_scroll() {
    let (mut app, tx, _rx) = app();
    app.panel = Some(Panel::Help(HelpState {
        tab: HelpTab::Commands,
        scroll: 0,
        rows: 10,
        total: 50,
    }));
    let help = |app: &App| match app.panel {
        Some(Panel::Help(h)) => h,
        _ => unreachable!(),
    };
    let scroll = |app: &App| help(app).scroll;
    app.update(Action::ScrollBy(3), &tx);
    assert_eq!(scroll(&app), 3);
    app.update(key(KeyCode::PageDown), &tx);
    assert_eq!(scroll(&app), 12);
    app.update(key(KeyCode::End), &tx);
    assert_eq!(scroll(&app), 40);
    app.update(key(KeyCode::Down), &tx);
    assert_eq!(scroll(&app), 40);
    app.update(key(KeyCode::Up), &tx);
    assert_eq!(scroll(&app), 39);
    app.update(key(KeyCode::Home), &tx);
    assert_eq!(scroll(&app), 0);
    app.update(key(KeyCode::Up), &tx);
    assert_eq!(scroll(&app), 0);
    assert!(app.panel_keys().contains(&("↑↓", "scroll")));
    // tab walks the sections and each one opens at the top
    app.update(Action::ScrollBy(5), &tx);
    app.update(key(KeyCode::Tab), &tx);
    assert_eq!((help(&app).tab, help(&app).scroll), (HelpTab::Keys, 0));
    app.update(key(KeyCode::Tab), &tx);
    assert_eq!(help(&app).tab, HelpTab::General);
    app.update(key(KeyCode::Left), &tx);
    assert_eq!(help(&app).tab, HelpTab::Keys);
    assert!(app.panel_keys().contains(&("tab", "section")));
    app.update(key(KeyCode::Esc), &tx);
    assert!(app.panel.is_none());
}

#[tokio::test]
async fn mouse_selection_in_the_box() {
    let (mut app, tx, _rx) = app();
    app.loading = false;
    type_text(&mut app, &tx, "some words");
    let area = ratatui::layout::Rect::new(0, 10, 40, 1);
    app.input_area = Some(area);
    let px = crate::input::PROMPT_WIDTH as u16;
    // dragging over «words» and releasing leaves it selected and copied
    app.update(Action::MouseDown(px + 5, 10), &tx);
    app.update(Action::MouseDrag(px + 10, 10), &tx);
    app.update(Action::MouseUp(px + 10, 10), &tx);
    assert_eq!(app.input.selection_text(), "words");
    assert!(app.input.has_selection());
    assert!(
        app.notice
            .as_ref()
            .is_some_and(|(n, _)| n.contains("5 chars")),
        "{:?}",
        app.notice
    );
    // the text is untouched: selecting does not edit
    assert_eq!(app.input.text(), "some words");
    // the next key sweeps it away
    app.update(key(KeyCode::Esc), &tx);
    assert!(!app.input.has_selection());
    assert_eq!(app.input.text(), "some words");
    // and a lone click selects nothing
    app.update(Action::MouseDown(px + 2, 10), &tx);
    app.update(Action::MouseUp(px + 2, 10), &tx);
    assert!(!app.input.has_selection());
}

#[tokio::test]
async fn mouse_selection() {
    let (mut app, tx, _rx) = app();
    app.loading = false;
    type_text(&mut app, &tx, "/new"); // with no providers, App::new leaves an error in the conversation
    app.update(key(KeyCode::Enter), &tx);
    app.push_item(Item::Info("one two four".into()));
    app.push_item(Item::Info("second entry".into()));
    let conv = ratatui::layout::Rect::new(0, 0, 40, 12);
    app.conv_area = Some(conv);
    let _ = app.visible_lines(40, 12);
    // blank + welcome block (4 rows) + blank + "one two four" (row 6) + blank
    // + "second entry" (row 8)
    app.update(Action::MouseDown(4, 6), &tx);
    app.update(Action::MouseDrag(5, 8), &tx);
    assert_eq!(app.selection_text(), "two four\n\nsecond");
    // a click without dragging leaves no selection
    app.update(Action::MouseDown(3, 6), &tx);
    app.update(Action::MouseUp(3, 6), &tx);
    assert!(app.selection.is_none());
    // a click on the input box moves the cursor and does not select
    type_text(&mut app, &tx, "some words");
    app.input_area = Some(ratatui::layout::Rect::new(0, 14, 40, 1));
    app.update(
        Action::MouseDown(crate::input::PROMPT_WIDTH as u16 + 4, 14),
        &tx,
    );
    app.update(
        Action::MouseUp(crate::input::PROMPT_WIDTH as u16 + 4, 14),
        &tx,
    );
    assert!(app.selection.is_none());
    app.update(key(KeyCode::Char('X')), &tx);
    assert_eq!(app.input.text(), "someX words");
    // with a panel open the click does not reach the box
    app.panel = Some(Panel::Help(HelpState::default()));
    app.update(
        Action::MouseDown(crate::input::PROMPT_WIDTH as u16, 14),
        &tx,
    );
    app.panel = None;
    app.update(key(KeyCode::Char('Y')), &tx);
    assert_eq!(app.input.text(), "someXY words");
    assert_eq!(slice_columns("a日本b", 1, 3), "日");
    assert_eq!(slice_columns("text", 2, usize::MAX), "xt");
}

#[tokio::test]
async fn zz_look() {
    use moon_core::session::SessionStore;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let (mut app, _tx, _rx) = app();
    app.loading = false;
    app.items.clear();
    let dir = tempfile::tempdir().unwrap();
    let store = SessionStore::new(dir.path());
    let mut ids = Vec::new();
    for t in [
        "app bootstrap",
        "bulk session delete",
        "cache for markdown",
        "doubts about tokio",
        "errors from clippy",
        "filters in the picker",
        "grouping themes",
        "handling sysmon threads",
        "installer for windows",
        "javascript? no",
        "kernel panic",
        "logs from tracing",
    ] {
        ids.push(
            store
                .create(t, Some("ollama/llama3.1:8b".into()), None)
                .unwrap(),
        );
    }
    app.store = Some(store);
    app.recent_sessions = ids[10..].iter().rev().map(|m| m.id.clone()).collect();
    app.session = Some(ids[11].clone());
    app.open_sessions_picker();
    let mut term = Terminal::new(TestBackend::new(86, 26)).unwrap();
    term.draw(|f| crate::view::view(&mut app, f)).unwrap();
    let buf = term.backend().buffer();
    for y in 0..26u16 {
        let row: String = (0..86u16)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect();
        println!("|{}|", row.trim_end());
    }
}

#[tokio::test]
async fn the_new_version_notice_shows_in_the_welcome() {
    let (mut app, tx, _rx) = app();
    let text = |app: &App| {
        app.welcome_lines()
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let before = app.welcome_lines().len();
    assert!(!text(&app).contains("available"));

    app.update(Action::UpdateAvailable("0.2.0".into()), &tx);
    let out = text(&app);
    assert!(out.contains("↑ v0.2.0 available"), "{out}");
    assert!(out.contains("moon update"), "{out}");
    // the block grows by that one line and nothing else moves
    assert_eq!(app.welcome_lines().len(), before + 1);
}
