//! Tests of the context, the status rows, cancellation and the formatting helpers.

use super::tests::{app, ctrl, key, type_text};
use super::*;
use ratatui::style::Modifier;

#[tokio::test]
async fn the_context_window_is_the_one_ollama_loads() {
    let (mut app, _tx, _rx) = app();
    app.providers = vec![
        ProviderState {
            id: "ollama".into(),
            kind: "ollama",
            base_url: "http://localhost:11434".into(),
            health: None,
        },
        ProviderState {
            id: "lm".into(),
            kind: "openai",
            base_url: "http://localhost:1234/v1".into(),
            health: None,
        },
    ];
    app.current = Some(Current {
        provider: "ollama".into(),
        model: "m".into(),
    });
    app.ctx_len = Some(32_768);
    // with nothing asked for, what the model declares
    assert_eq!(app.ctx_window(), Some(32_768));
    // `num_ctx` is what ollama loads: it is the one that counts, and the
    // status line stops promising room that is not there
    app.params.num_ctx = Some(8_192);
    assert_eq!(app.ctx_window(), Some(8_192));
    app.last_usage = Some(Usage {
        prompt_tokens: Some(4_000),
        completion_tokens: Some(96),
        ..Default::default()
    });
    let status: String = app
        .status_spans()
        .iter()
        .map(|s| s.content.to_string())
        .collect();
    assert!(status.contains("context 50%"), "{status}");
    // asking for more than the model has does not invent window
    app.params.num_ctx = Some(200_000);
    assert_eq!(app.ctx_window(), Some(32_768));
    // and `num_ctx` never reaches an openai-compatible provider: there the
    // declared window is the truth
    app.params.num_ctx = Some(8_192);
    app.current = Some(Current {
        provider: "lm".into(),
        model: "m".into(),
    });
    assert_eq!(app.ctx_window(), Some(32_768));
}

#[tokio::test]
async fn machine_and_model_detail_in_context() {
    let (mut app, tx, _rx) = app();
    app.current = Some(Current {
        provider: "p".into(),
        model: "m".into(),
    });
    // the model list knows the weights; the rest is context cache
    let mut mi = ModelInfo::new("p", "m");
    mi.size_bytes = Some(2_470_000_000);
    app.models.push(mi);
    app.loaded = LoadedState::Loaded(LoadedModel {
        id: "m".into(),
        size_bytes: 5_329_597_235,
        size_vram_bytes: 5_329_597_235,
        context_length: Some(32_768),
        expires_at: None,
    });
    app.sys
        .push(crate::sysmon::Sample::new(34.0, 18 << 30, 32 << 30, 0));
    let info = |app: &mut App| {
        type_text(app, &tx, "/context");
        app.update(key(KeyCode::Enter), &tx);
        match app.items.last() {
            Some(Item::Info(s)) => s.clone(),
            other => panic!("expected Info, got {other:?}"),
        }
    };
    let out = info(&mut app);
    assert!(
        out.contains(
            "model: m · loaded 5.0 GB (2.3 GB weights + 2.7 GB context at 32.8k) · 100% gpu"
        ),
        "{out}"
    );
    assert!(
        out.contains("machine: cpu 34% · ram 18.0 / 32.0 GB (56%)"),
        "{out}"
    );
    // not loaded is stated, so it is known that the next request loads it
    app.loaded = LoadedState::NotLoaded;
    let out = info(&mut app);
    assert!(
        out.contains("model: m · not loaded (the next request loads it)"),
        "{out}"
    );
    // a provider that cannot tell does not clutter the output
    app.loaded = LoadedState::Unknown;
    let out = info(&mut app);
    assert!(!out.contains("model: m"), "{out}");
}

#[tokio::test]
async fn attachments_context_and_budget() {
    let (mut app, tx, _rx) = app();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    std::fs::write(dir.path().join("MOON.md"), "Rust project.\n").unwrap();
    app.root = dir.path().to_path_buf();
    app.load_context_file();
    assert_eq!(
        app.context_file.as_ref().map(|(n, _)| n.as_str()),
        Some("MOON.md")
    );

    assert!(app.attach("a.rs"));
    assert_eq!(app.live.len(), 1);
    assert!(app
        .notice
        .as_ref()
        .is_some_and(|(n, _)| n.starts_with("attached: a.rs")));
    type_text(&mut app, &tx, "/context");
    app.update(key(KeyCode::Enter), &tx);
    let info = match app.items.last() {
        Some(Item::Info(s)) => s.clone(),
        other => panic!("expected Info, got {other:?}"),
    };
    assert!(info.contains("context file: MOON.md"), "{info}");
    assert!(info.contains("  a.rs · "), "{info}");

    // full system prompt: base + MOON.md + live attachment
    let live = app.read_live().unwrap();
    let sp = app.system_prompt_for(&live).unwrap();
    assert!(sp.contains("# Project context (MOON.md)"));
    assert!(sp.contains("<file path=\"a.rs\">"));

    // snapshot with @ in the message
    app.current = Some(Current {
        provider: "p".into(),
        model: "m".into(),
    });
    type_text(&mut app, &tx, "explain @a.rs please");
    app.update(key(KeyCode::Enter), &tx);
    let user = app
        .messages()
        .find(|m| m.role == Role::User)
        .expect("mensaje enviado");
    assert_eq!(user.attachments.len(), 1);
    assert_eq!(user.attachments[0].path, "a.rs");
    assert!(user.wire_content().starts_with("<file path=\"a.rs\">"));

    // budget: with 10 tokens of context it is not sent
    app.gen = Generation::Idle;
    app.ctx_len = Some(10);
    type_text(&mut app, &tx, "more @a.rs");
    app.update(key(KeyCode::Enter), &tx);
    assert!(app
        .notice
        .as_ref()
        .is_some_and(|(n, _)| n.starts_with("context budget")));
    assert_eq!(app.input.text(), "more @a.rs");
    app.input.clear();

    // missing file: a notice, and the text is kept
    app.ctx_len = None;
    type_text(&mut app, &tx, "see @nope.rs");
    app.update(key(KeyCode::Enter), &tx);
    assert!(app
        .notice
        .as_ref()
        .is_some_and(|(n, _)| n == "@nope.rs: not found"));
    app.input.clear();

    // path completion after @
    type_text(&mut app, &tx, "read @a");
    app.update(key(KeyCode::Tab), &tx);
    assert_eq!(app.input.text(), "read @a.rs");
    app.input.clear();

    app.detach("a.rs");
    assert!(app.live.is_empty());
    assert_eq!(
        common_prefix(&["moon.rs".into(), "moon.md".into()]),
        "moon."
    );
}

#[tokio::test]
async fn the_files_panel_attaches_and_detaches() {
    let (mut app, tx, _rx) = app();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "fn lib() {}\n").unwrap();
    app.root = dir.path().to_path_buf();

    // ctrl+f opens it; with nothing attached only the button to add is there
    app.update(ctrl('f'), &tx);
    let Some(Panel::Files(p)) = &app.panel else {
        panic!("the files panel should be open")
    };
    assert_eq!(p.len(), 1);
    assert_eq!(p.title_info, "nothing attached");

    // enter on `Add files…` walks into the tree, and a folder goes deeper
    app.update(key(KeyCode::Enter), &tx);
    let Some(Panel::Browse { picker, dir: at }) = &app.panel else {
        panic!("the tree should be open")
    };
    assert_eq!(at.as_os_str(), "");
    assert_eq!(
        picker.current().map(|i| i.label.clone()),
        Some("src/".into())
    );
    app.update(key(KeyCode::Enter), &tx);
    let Some(Panel::Browse { picker, dir: at }) = &app.panel else {
        panic!("it should be inside src")
    };
    assert_eq!(at.to_string_lossy(), "src");
    // `..` comes first, then what is inside
    assert_eq!(picker.current().map(|i| i.label.clone()), Some("..".into()));
    app.update(key(KeyCode::Down), &tx);
    app.update(key(KeyCode::Enter), &tx);
    assert_eq!(app.live.len(), 1);
    assert_eq!(app.live[0].path, "src/lib.rs");
    // it stays in the tree, and the row it attached now carries the check
    let Some(Panel::Browse { picker, .. }) = &app.panel else {
        panic!("it should still be in the tree")
    };
    assert!(picker.current().is_some_and(|i| i.active));
    // enter again on the same row takes it out
    app.update(key(KeyCode::Enter), &tx);
    assert!(app.live.is_empty());
    app.update(key(KeyCode::Enter), &tx);
    assert_eq!(app.live.len(), 1);

    // esc goes back to the panel, which now lists the file with its cost
    app.update(key(KeyCode::Esc), &tx);
    let Some(Panel::Files(p)) = &app.panel else {
        panic!("it should be back in the panel")
    };
    assert!(p.title_info.starts_with("1 file · "));
    let labels: Vec<String> = p.visible().map(|(i, ..)| i.label.clone()).collect();
    assert_eq!(labels[0], "src/lib.rs");
    assert!(labels.contains(&"Detach all".to_string()));

    // enter on the file detaches it, and the button disappears with it
    app.update(key(KeyCode::Enter), &tx);
    assert!(app.live.is_empty());
    let Some(Panel::Files(p)) = &app.panel else {
        panic!("the panel should still be open")
    };
    assert_eq!(p.len(), 1);
    assert!(app
        .notice
        .as_ref()
        .is_some_and(|(n, _)| n == "detached src/lib.rs"));
    app.update(key(KeyCode::Esc), &tx);
    assert!(app.panel.is_none());

    // and the old commands open the same panel instead of failing
    type_text(&mut app, &tx, "/add");
    app.update(key(KeyCode::Enter), &tx);
    assert!(matches!(app.panel, Some(Panel::Files(_))));
}

#[test]
fn the_shimmer_sweeps_the_verb_in_step_with_the_star() {
    let (mut app, tx, _rx) = app();
    app.loading = true;
    // spans: star, space and one letter per span of "checking providers…"
    let bright = |app: &App| -> Vec<usize> {
        app.activity_spans().unwrap()[2..]
            .iter()
            .enumerate()
            .filter(|(_, s)| s.style.add_modifier.contains(Modifier::BOLD))
            .map(|(i, _)| i)
            .collect()
    };
    assert_eq!(bright(&app), Vec::<usize>::new());
    app.update(Action::Tick, &tx);
    assert_eq!(bright(&app), vec![0]);
    app.update(Action::Tick, &tx);
    assert_eq!(bright(&app), vec![1]);
    let spans = app.activity_spans().unwrap();
    assert_eq!(
        spans[0].content, SPINNER[2],
        "the star moves on the same counter"
    );
    assert_eq!(spans[3].style.fg, Some(app.theme.moon_soft), "head");
    assert_eq!(
        spans[2].style.fg,
        Some(app.theme.moon_soft),
        "vecina izquierda"
    );
    assert!(!spans[2].style.add_modifier.contains(Modifier::BOLD));
    assert_eq!(
        spans[4].style.fg,
        Some(app.theme.moon_soft),
        "vecina derecha"
    );
    assert_eq!(
        spans[7].style.fg,
        Some(app.theme.moon),
        "el resto en `moon`"
    );
    let text: String = spans.iter().map(|s| s.content.to_string()).collect();
    assert_eq!(text, "✳ checking providers…");
    // after running through the verb it rests and starts over
    let period = "checking providers…".chars().count() + SHIMMER_REST;
    for _ in 2..period + 1 {
        app.update(Action::Tick, &tx);
    }
    assert_eq!(bright(&app), vec![0]);
}

#[tokio::test]
async fn cancelling_closes_the_turn_with_a_cross() {
    let (mut app, tx, _rx) = app();
    app.loading = false;
    app.current = Some(Current {
        provider: "p".into(),
        model: "m".into(),
    });
    let summary = |app: &App| -> String {
        app.activity_spans()
            .unwrap()
            .iter()
            .map(|s| s.content.to_string())
            .collect()
    };
    // cancelled while thinking: no message, but with a closing line and no notice
    app.push_item(Item::Message(Message::user("hello")));
    app.gen = Generation::Streaming {
        cancel: CancellationToken::new(),
        started: Instant::now(),
        first_at: None,
        deltas: 0,
        sent: 30,
    };
    app.cancel_generation();
    assert!(!app.is_streaming());
    assert_eq!(summary(&app), "✗ cancelled (0s · ↑ ~30 · ↓ 0 tokens)");
    assert!(app.notice.is_none());
    assert!(matches!(app.items.last(), Some(Item::Message(m)) if m.role == Role::User));
    // cancelled halfway: the partial reply stays and the closing line counts what was received
    app.gen_id = 9;
    app.gen = Generation::Streaming {
        cancel: CancellationToken::new(),
        started: Instant::now(),
        first_at: None,
        deltas: 0,
        sent: 30,
    };
    app.update(Action::Stream(9, StreamEvent::Delta("hel".into())), &tx);
    app.update(Action::Stream(9, StreamEvent::Delta("lo".into())), &tx);
    app.cancel_generation();
    let s = summary(&app);
    assert!(
        s.starts_with("✗ cancelled (0s · ↑ ~30 · ↓ 2 tokens · "),
        "{s}"
    );
    assert!(s.ends_with(" tok/s)"), "{s}");
    assert!(app.notice.is_none());
    assert!(
        matches!(app.items.last(), Some(Item::Message(m)) if m.partial && m.content == "hello")
    );
    // the cross goes in `alert`
    let head = &app.activity_spans().unwrap()[0];
    assert_eq!(head.style.fg, Some(app.theme.alert));
}

#[test]
fn the_star_takes_one_column() {
    for f in SPINNER.into_iter().chain(["✓", "✗"]) {
        assert_eq!(unicode_width::UnicodeWidthStr::width(f), 1, "{f}");
    }
}

#[test]
fn formats() {
    assert_eq!(fmt_dur(Duration::from_secs(12)), "12s");
    assert_eq!(fmt_dur(Duration::from_secs(80)), "1m 20s");
    assert_eq!(fmt_dur(Duration::from_secs(3725)), "1h 2m");
    assert_eq!(fmt_k(32768), "32.8k");
    assert_eq!(fmt_k(8000), "8k");
    assert_eq!(fmt_k(512), "512");
    assert_eq!(fmt_size(8_988_124_069), "9.0 GB");
    assert_eq!(fmt_size(500_000_000), "500 MB");
}

#[tokio::test]
async fn the_context_lists_the_gpu_when_there_is_a_card() {
    let (mut app, tx, _rx) = app();
    app.loading = false;
    app.sys
        .push(crate::sysmon::Sample::new(34.0, 18 << 30, 32 << 30, 0).with_gpu(6 << 30, 8 << 30));
    type_text(&mut app, &tx, "/context");
    app.update(key(KeyCode::Enter), &tx);
    let Some(Item::Info(out)) = app.items.last() else {
        panic!("expected Info, got {:?}", app.items.last())
    };
    assert!(
        out.contains("machine: cpu 34% · ram 18.0 / 32.0 GB (56%) · gpu 6.0 / 8.0 GB (75%)"),
        "{out}"
    );
    assert!(
        out.contains("3m peak: cpu 34% · ram 56% · gpu 75%"),
        "{out}"
    );
}
