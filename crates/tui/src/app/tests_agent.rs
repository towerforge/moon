//! The model editing files, from the interface: the switch, a scripted turn
//! through a fake provider, the approval panel, `Esc` and `/undo`.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use futures_util::stream;
use moon_core::{
    ChatStream, ConfigError, Provider, ProviderConfig, ProviderFactory, ToolCall, ToolSpec,
};
use serde_json::json;

use super::tests::{app, key, type_text};
use super::*;

type Rx = mpsc::UnboundedReceiver<Action>;
type Requests = Arc<Mutex<Vec<ChatRequest>>>;
type Script = Arc<Mutex<VecDeque<Vec<ChatEvent>>>>;

/// A provider that answers from a script, one reply per request, and keeps
/// every request it was sent.
struct Fake {
    requests: Requests,
    script: Script,
}

#[async_trait]
impl Provider for Fake {
    fn id(&self) -> &str {
        "fake"
    }
    fn kind(&self) -> &'static str {
        "fake"
    }
    fn base_url(&self) -> &str {
        "fake://"
    }
    async fn health(&self) -> Result<Health, ProviderError> {
        Ok(Health::default())
    }
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![ModelInfo::new("fake", "m")])
    }
    async fn chat(
        &self,
        req: ChatRequest,
        _cancel: CancellationToken,
    ) -> Result<ChatStream, ProviderError> {
        self.requests.lock().unwrap().push(req);
        let events = self
            .script
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| vec![ChatEvent::Done(Usage::default())]);
        Ok(Box::pin(stream::iter(events.into_iter().map(Ok))))
    }
}

struct FakeFactory {
    requests: Requests,
    script: Script,
}

impl ProviderFactory for FakeFactory {
    fn kind(&self) -> &'static str {
        "fake"
    }
    fn build(&self, _id: &str, _cfg: &ProviderConfig) -> Result<Arc<dyn Provider>, ConfigError> {
        Ok(Arc::new(Fake {
            requests: self.requests.clone(),
            script: self.script.clone(),
        }))
    }
}

fn app_with_fake(root: &Path, script: Vec<Vec<ChatEvent>>) -> (App, Tx, Rx, Requests) {
    let requests: Requests = Arc::new(Mutex::new(Vec::new()));
    let mut registry = Registry::new();
    registry.register(Box::new(FakeFactory {
        requests: requests.clone(),
        script: Arc::new(Mutex::new(script.into())),
    }));
    let mut cfg = Config::default();
    cfg.providers
        .insert("fake".into(), ProviderConfig::new("fake"));
    registry.build_all(&cfg).unwrap();
    let (tx, rx) = mpsc::unbounded_channel();
    let mut app = App::new(RunOptions {
        config: cfg,
        config_source: ConfigSource::Default(PathBuf::from("/config.toml")),
        registry: Arc::new(registry),
        store: None,
        resume: None,
        model: Some("fake/m".into()),
        version: "0.0.0".into(),
        cwd: "~/p".into(),
        root: root.to_path_buf(),
        state_dir: None,
    });
    app.loading = false;
    assert!(app.current.is_some(), "{:?}", app.items);
    (app, tx, rx, requests)
}

/// Feeds the app what the fake provider streams until nothing is streaming:
/// the turn is over, or an edit is waiting on screen.
async fn settle(app: &mut App, tx: &Tx, rx: &mut Rx) {
    while app.is_streaming() {
        let a = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("the provider answers")
            .expect("the channel is open");
        app.update(a, tx);
    }
}

fn call(name: &str, args: serde_json::Value) -> ChatEvent {
    ChatEvent::ToolCall(ToolCall {
        id: None,
        name: name.into(),
        arguments: args,
    })
}

fn done() -> ChatEvent {
    ChatEvent::Done(Usage {
        prompt_tokens: Some(10),
        completion_tokens: Some(2),
        ..Default::default()
    })
}

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.rs"), "hi\n").unwrap();
    dir
}

fn text(spans: &[Span<'static>]) -> String {
    spans.iter().map(|s| s.content.to_string()).collect()
}

#[tokio::test]
async fn the_switch_and_what_it_shows() {
    let dir = project();
    let (mut app, tx, _rx) = app();
    app.root = dir.path().to_path_buf();
    assert!(!app.tools_on);
    assert!(app.edit_mode_span().is_none());
    assert!(!text(&app.model_spans()).contains("edits"));

    // `/tools` is the panel: enter on the first row ticks it, and `esc`
    // applies it — there is no cancel
    type_text(&mut app, &tx, "/tools");
    app.update(key(KeyCode::Enter), &tx);
    assert!(matches!(&app.panel, Some(Panel::Tools(d)) if !d.on));
    app.update(key(KeyCode::Enter), &tx);
    assert!(matches!(&app.panel, Some(Panel::Tools(d)) if d.on && d.row == ToolsDialog::ON));
    // off, the writing boxes start off too; tick them as well
    app.update(key(KeyCode::Down), &tx);
    app.update(key(KeyCode::Enter), &tx);
    app.update(key(KeyCode::Down), &tx);
    app.update(key(KeyCode::Enter), &tx);
    assert!(matches!(&app.panel, Some(Panel::Tools(d)) if d.on && d.edit && d.create));
    assert!(!app.tools_on);
    app.update(key(KeyCode::Esc), &tx);
    assert!(app.panel.is_none());
    assert!(app.tools_on);
    assert!(app.harness.is_some());
    // the permanent sign, in `moon-soft` and bold, at the left of the hints
    // row: all three boxes are on, so it lists all three
    let mode = app.edit_mode_span().expect("edit mode indicator");
    assert_eq!(mode.content, "⏵⏵ Read · Edit · Create");
    assert_eq!(mode.style.fg, Some(app.theme.moon_soft));
    assert!(!text(&app.model_spans()).contains("edits"));
    assert!(app.hints().contains("/tools"));
    // not a git repository: said once, in the conversation
    assert!(matches!(app.items.last(), Some(Item::Info(s)) if s.contains("not a git repository")));
    // the agent's rules reach the system prompt
    assert!(app.system_prompt_for(&[]).unwrap().contains("no shell"));

    // esc with nothing touched still applies, but there is nothing to
    // change: the state stays as it is
    type_text(&mut app, &tx, "/tools");
    app.update(key(KeyCode::Enter), &tx);
    app.update(key(KeyCode::Esc), &tx);
    assert!(app.panel.is_none());
    assert!(app.tools_on);

    // unticking the box and esc turns it off
    type_text(&mut app, &tx, "/tools");
    app.update(key(KeyCode::Enter), &tx);
    app.update(key(KeyCode::Enter), &tx);
    assert!(matches!(&app.panel, Some(Panel::Tools(d)) if !d.on));
    app.update(key(KeyCode::Esc), &tx);
    assert!(app.panel.is_none());
    assert!(!app.tools_on);
    assert!(app.harness.is_none());
    assert!(app.system_prompt_for(&[]).is_none());

    // whatever follows the command is ignored, as with `/model`
    type_text(&mut app, &tx, "/tools on");
    app.update(key(KeyCode::Enter), &tx);
    assert!(matches!(app.panel, Some(Panel::Tools(_))));
}

#[tokio::test]
async fn the_tools_panel_turns_it_on_and_tunes_it() {
    let dir = project();
    let (mut app, tx, _rx) = app();
    app.root = dir.path().to_path_buf();
    type_text(&mut app, &tx, "/tools");
    app.update(key(KeyCode::Enter), &tx);
    let Some(Panel::Tools(d)) = &app.panel else {
        panic!("expected the tools panel")
    };
    assert_eq!(
        (d.on, d.edit, d.create, d.rounds, d.row),
        (false, false, false, 8, ToolsDialog::ON)
    );
    // space ticks the box under the cursor, ↓ walks, ←→ change the number;
    // esc applies it all, there is no cancel and no separate confirm row
    app.update(key(KeyCode::Char(' ')), &tx);
    app.update(key(KeyCode::Down), &tx);
    app.update(key(KeyCode::Char(' ')), &tx);
    app.update(key(KeyCode::Down), &tx);
    app.update(key(KeyCode::Down), &tx);
    app.update(key(KeyCode::Left), &tx);
    app.update(key(KeyCode::Left), &tx);
    let Some(Panel::Tools(d)) = &app.panel else {
        panic!("expected the tools panel")
    };
    assert_eq!(
        (d.on, d.edit, d.create, d.rounds, d.row),
        (true, true, false, 6, ToolsDialog::ROUNDS)
    );
    app.update(key(KeyCode::Esc), &tx);
    assert!(app.panel.is_none());
    assert!(app.tools_on);
    let h = app.harness.as_ref().unwrap();
    assert!(!h.agent().has(moon_agent::Tool::WriteFile));
    assert_eq!(h.limits().rounds, 6);
    assert!(app
        .notice
        .as_ref()
        .is_some_and(|(n, _)| n.contains("not create")));
    // reopened it shows what is set; unticking the first box and esc
    // turns it off
    type_text(&mut app, &tx, "/tools");
    app.update(key(KeyCode::Enter), &tx);
    assert!(
        matches!(&app.panel, Some(Panel::Tools(d)) if d.on && d.edit && !d.create && d.rounds == 6)
    );
    app.update(key(KeyCode::Char(' ')), &tx);
    app.update(key(KeyCode::Esc), &tx);
    assert!(!app.tools_on);
    assert!(app.harness.is_none());
    // the number stays in range and the cursor wraps
    let mut d = ToolsDialog {
        on: true,
        edit: true,
        create: true,
        rounds: 20,
        row: ToolsDialog::ROUNDS,
    };
    d.change(5);
    assert_eq!(d.rounds, 20);
    d.change(-100);
    assert_eq!(d.rounds, 1);
    d.up();
    assert_eq!(d.row, ToolsDialog::CREATE);
    d.up();
    assert_eq!(d.row, ToolsDialog::EDIT);
    d.down();
    d.down();
    assert_eq!(d.row, ToolsDialog::ROUNDS);
    d.down();
    assert_eq!(d.row, ToolsDialog::ON);
}

#[tokio::test]
async fn read_only_is_the_reader_and_says_so() {
    let dir = project();
    let (mut app, tx, _rx) = app();
    app.root = dir.path().to_path_buf();
    // the first box alone: the writing ones start off
    type_text(&mut app, &tx, "/tools");
    app.update(key(KeyCode::Enter), &tx);
    app.update(key(KeyCode::Char(' ')), &tx);
    let Some(Panel::Tools(d)) = &app.panel else {
        panic!("expected the tools panel")
    };
    assert_eq!((d.on, d.edit, d.create), (true, false, false));
    app.update(key(KeyCode::Esc), &tx);
    assert!(app.tools_on);
    assert_eq!(app.tools_scope(), (false, false));
    assert!(!app.tools_write());
    let h = app.harness.as_ref().unwrap();
    assert_eq!(h.agent().name, "reader");
    assert_eq!(h.specs().len(), 2);
    // the marker, the notice and the prompt all say it only reads
    assert_eq!(app.edit_mode_span().unwrap().content, "⏵⏵ Read");
    assert!(app
        .notice
        .as_ref()
        .is_some_and(|(n, _)| n.contains("reads on") && n.contains("change nothing")));
    let prompt = app.system_prompt_for(&[]).unwrap();
    assert!(prompt.contains("cannot change files") && !prompt.contains("edit_file"));
    // and the panel, reopened, shows both boxes off; ticking `edit` back
    // gives the editor without write_file
    type_text(&mut app, &tx, "/tools");
    app.update(key(KeyCode::Enter), &tx);
    assert!(matches!(&app.panel, Some(Panel::Tools(d)) if d.on && !d.edit && !d.create));
    app.update(key(KeyCode::Down), &tx);
    app.update(key(KeyCode::Char(' ')), &tx);
    app.update(key(KeyCode::Esc), &tx);
    assert_eq!(app.tools_scope(), (true, false));
    assert_eq!(app.edit_mode_span().unwrap().content, "⏵⏵ Read · Edit");
    assert_eq!(app.harness.as_ref().unwrap().agent().name, "editor");
}

#[test]
fn the_configuration_turns_it_on_at_startup() {
    let dir = project();
    let at_startup = |tools: moon_core::ToolsConfig, root: &Path| {
        App::new(RunOptions {
            config: Config {
                tools,
                ..Default::default()
            },
            config_source: ConfigSource::Default(PathBuf::from("/config.toml")),
            registry: Arc::new(Registry::new()),
            store: None,
            resume: None,
            model: None,
            version: "0.0.0".into(),
            cwd: "~/p".into(),
            root: root.to_path_buf(),
            state_dir: None,
        })
    };
    let mut tools = moon_core::ToolsConfig {
        enabled: true,
        ..Default::default()
    };
    let app = at_startup(tools.clone(), dir.path());
    assert!(app.tools_on);
    assert!(app.harness.is_some());
    // nothing said about the scope: all four, as before the two keys existed
    assert_eq!(app.tools_scope(), (true, true));

    // what `moon config init` writes: reading, and nothing that writes
    tools.edit = false;
    tools.create = false;
    let app = at_startup(tools.clone(), dir.path());
    assert!(app.tools_on);
    assert_eq!(app.tools_scope(), (false, false));
    assert!(!app.tools_write());
    assert_eq!(app.harness.as_ref().unwrap().agent().name, "reader");
    assert_eq!(app.edit_mode_span().unwrap().content, "⏵⏵ Read");

    // and one box alone
    tools.edit = true;
    let app = at_startup(tools, dir.path());
    assert_eq!(app.tools_scope(), (true, false));
    assert_eq!(app.edit_mode_span().unwrap().content, "⏵⏵ Read · Edit");
}

#[tokio::test]
async fn a_turn_reads_a_file_and_answers() {
    let dir = project();
    let (mut app, tx, mut rx, requests) = app_with_fake(
        dir.path(),
        vec![
            vec![call("read_file", json!({"path": "a.rs"})), done()],
            vec![ChatEvent::Delta("it says hi".into()), done()],
        ],
    );
    app.enable_tools().unwrap();
    type_text(&mut app, &tx, "what does a.rs say");
    app.update(key(KeyCode::Enter), &tx);
    settle(&mut app, &tx, &mut rx).await;
    assert!(!app.turn_active());

    let reqs = requests.lock().unwrap();
    assert_eq!(reqs.len(), 2);
    // both requests offered the tools and carried the agent's rules
    let names: Vec<&str> = reqs[0]
        .tools
        .iter()
        .map(|t: &ToolSpec| t.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec!["read_file", "list_dir", "edit_file", "write_file"]
    );
    assert!(reqs[0].messages[0].content.contains("no shell"));
    // the second one carried the call and its result, in order
    let m = &reqs[1].messages;
    let n = m.len();
    assert_eq!(m[n - 2].role, Role::Assistant);
    assert_eq!(m[n - 2].tool_calls[0].name, "read_file");
    assert_eq!(m[n - 1].role, Role::Tool);
    assert_eq!(m[n - 1].tool_name.as_deref(), Some("read_file"));
    assert!(m[n - 1].content.contains("<file path=\"a.rs\">"));
    drop(reqs);

    // on screen: the step line and the answer; the tool result is hidden
    assert!(app.items.iter().any(
        |i| matches!(i, Item::Step(s) if s.tool == moon_agent::Tool::ReadFile && s.path == "a.rs")
    ));
    assert_eq!(app.messages().next_back().unwrap().content, "it says hi");
    let hidden: Vec<bool> = app.items.iter().map(super::render::hidden).collect();
    assert!(hidden.iter().any(|h| *h));
    let shown: Vec<String> = app
        .visible_lines(80, 40)
        .iter()
        .map(|l| l.to_string())
        .collect();
    assert!(
        shown.iter().any(|l| l.contains("· read  a.rs")),
        "{shown:?}"
    );
    assert!(!shown.iter().any(|l| l.contains("<file")), "{shown:?}");

    // `/undo` takes the whole turn away, not just the last reply
    type_text(&mut app, &tx, "/undo");
    app.update(key(KeyCode::Enter), &tx);
    assert_eq!(app.messages().count(), 0);
    assert!(!app.items.iter().any(|i| matches!(i, Item::Step(_))));
}

#[tokio::test]
async fn a_call_written_as_text_runs_all_the_same() {
    // what qwen2.5-coder does on Ollama: the call as JSON in the reply
    let dir = project();
    let (mut app, tx, mut rx, requests) = app_with_fake(
        dir.path(),
        vec![
            vec![
                ChatEvent::Delta("{\"name\": \"read_file\", ".into()),
                ChatEvent::Delta("\"arguments\": {\"path\": \"a.rs\"}}".into()),
                done(),
            ],
            vec![ChatEvent::Delta("it says hi".into()), done()],
        ],
    );
    app.enable_tools().unwrap();
    type_text(&mut app, &tx, "what does a.rs say");
    app.update(key(KeyCode::Enter), &tx);
    settle(&mut app, &tx, &mut rx).await;
    assert!(!app.turn_active());
    // the JSON never shows as a reply: it became the call
    let shown: Vec<String> = app
        .visible_lines(80, 40)
        .iter()
        .map(|l| l.to_string())
        .collect();
    assert!(!shown.iter().any(|l| l.contains("\"name\"")), "{shown:?}");
    assert!(
        shown.iter().any(|l| l.contains("· read  a.rs")),
        "{shown:?}"
    );
    assert_eq!(app.messages().next_back().unwrap().content, "it says hi");
    // and the history the model gets back is the well-formed one
    let reqs = requests.lock().unwrap();
    let m = &reqs[1].messages;
    let n = m.len();
    assert_eq!(m[n - 2].tool_calls[0].name, "read_file");
    assert!(m[n - 2].content.is_empty());
    assert_eq!(m[n - 1].role, Role::Tool);
}

#[tokio::test]
async fn an_edit_waits_for_the_ok_and_is_applied() {
    let dir = project();
    let (mut app, tx, mut rx, requests) = app_with_fake(
        dir.path(),
        vec![
            vec![call("read_file", json!({"path": "a.rs"})), done()],
            vec![
                call(
                    "edit_file",
                    json!({"path": "a.rs", "old_string": "hi", "new_string": "bye"}),
                ),
                done(),
            ],
            vec![ChatEvent::Delta("done".into()), done()],
        ],
    );
    app.enable_tools().unwrap();
    type_text(&mut app, &tx, "change hi to bye");
    app.update(key(KeyCode::Enter), &tx);
    settle(&mut app, &tx, &mut rx).await;
    // paused on the edit: on screen, not on disk
    assert!(app.waiting_approval());
    assert!(app.turn_active());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.rs")).unwrap(),
        "hi\n"
    );
    assert!(text(&app.activity_spans().unwrap()).contains("waiting for your approval"));
    let Some(Panel::Approval(a)) = &app.panel else {
        panic!("expected the approval panel")
    };
    assert_eq!(a.title(), "Edit a.rs");
    assert_eq!(a.edit.counts(), "+1 −1");
    assert_eq!(a.choice, EditChoice::Apply);
    // the cursor walks the two choices; enter takes the one it is on
    app.update(key(KeyCode::Right), &tx);
    assert!(matches!(&app.panel, Some(Panel::Approval(a)) if a.choice == EditChoice::Skip));
    app.update(key(KeyCode::Left), &tx);
    app.update(key(KeyCode::Enter), &tx);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.rs")).unwrap(),
        "bye\n"
    );
    assert!(app.panel.is_none());
    settle(&mut app, &tx, &mut rx).await;
    assert!(!app.turn_active());
    assert_eq!(app.messages().next_back().unwrap().content, "done");
    assert!(app.items.iter().any(
        |i| matches!(i, Item::Step(s) if s.outcome == moon_agent::Outcome::Applied && s.added == 1)
    ));
    let reqs = requests.lock().unwrap();
    assert_eq!(reqs.len(), 3);
    let last = reqs[2].messages.last().unwrap();
    assert_eq!(last.role, Role::Tool);
    assert!(last.content.starts_with("applied: `a.rs`"));
}

#[tokio::test]
async fn skip_leaves_the_file_and_tells_the_model() {
    let dir = project();
    let (mut app, tx, mut rx, requests) = app_with_fake(
        dir.path(),
        vec![
            vec![
                call("read_file", json!({"path": "a.rs"})),
                call(
                    "edit_file",
                    json!({"path": "a.rs", "old_string": "hi", "new_string": "bye"}),
                ),
                done(),
            ],
            vec![ChatEvent::Delta("ok".into()), done()],
        ],
    );
    app.enable_tools().unwrap();
    type_text(&mut app, &tx, "go");
    app.update(key(KeyCode::Enter), &tx);
    settle(&mut app, &tx, &mut rx).await;
    assert!(app.waiting_approval());
    app.update(key(KeyCode::Char('s')), &tx);
    settle(&mut app, &tx, &mut rx).await;
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.rs")).unwrap(),
        "hi\n"
    );
    assert!(app
        .items
        .iter()
        .any(|i| matches!(i, Item::Step(s) if s.outcome == moon_agent::Outcome::Skipped)));
    let reqs = requests.lock().unwrap();
    let last = reqs[1].messages.last().unwrap();
    assert!(last.content.starts_with("skipped"));
}

#[tokio::test]
async fn esc_cancels_the_turn_and_writes_nothing() {
    let dir = project();
    let (mut app, tx, mut rx, requests) = app_with_fake(
        dir.path(),
        vec![
            vec![call("read_file", json!({"path": "a.rs"})), done()],
            vec![
                call(
                    "edit_file",
                    json!({"path": "a.rs", "old_string": "hi", "new_string": "bye"}),
                ),
                done(),
            ],
        ],
    );
    app.enable_tools().unwrap();
    type_text(&mut app, &tx, "go");
    app.update(key(KeyCode::Enter), &tx);
    settle(&mut app, &tx, &mut rx).await;
    assert!(app.waiting_approval());
    app.update(key(KeyCode::Esc), &tx);
    assert!(app.panel.is_none());
    assert!(!app.turn_active());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.rs")).unwrap(),
        "hi\n"
    );
    // the call is closed in the history, and nothing else was sent
    let last = app.messages().next_back().unwrap();
    assert_eq!(last.role, Role::Tool);
    assert!(last.content.contains("cancelled"));
    assert_eq!(requests.lock().unwrap().len(), 2);
    assert!(app
        .notice
        .as_ref()
        .is_some_and(|(n, _)| n.contains("cancelled")));
    // and the next question goes out as a fresh turn
    type_text(&mut app, &tx, "again");
    app.update(key(KeyCode::Enter), &tx);
    settle(&mut app, &tx, &mut rx).await;
    assert_eq!(requests.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn a_refused_path_is_reported_and_the_turn_goes_on() {
    let dir = project();
    let (mut app, tx, mut rx, _requests) = app_with_fake(
        dir.path(),
        vec![
            vec![call("read_file", json!({"path": "../outside.txt"})), done()],
            vec![ChatEvent::Delta("sorry".into()), done()],
        ],
    );
    app.enable_tools().unwrap();
    type_text(&mut app, &tx, "go");
    app.update(key(KeyCode::Enter), &tx);
    settle(&mut app, &tx, &mut rx).await;
    assert!(!app.turn_active());
    let shown: Vec<String> = app
        .visible_lines(100, 40)
        .iter()
        .map(|l| l.to_string())
        .collect();
    assert!(
        shown
            .iter()
            .any(|l| l.contains("✗ read  ../outside.txt") && l.contains("outside the project")),
        "{shown:?}"
    );
    assert_eq!(app.messages().next_back().unwrap().content, "sorry");
}

#[tokio::test]
async fn without_a_provider_the_turn_does_not_hang() {
    let dir = project();
    let (mut app, tx, _rx) = app();
    app.root = dir.path().to_path_buf();
    app.enable_tools().unwrap();
    app.current = Some(Current {
        provider: "p".into(),
        model: "m".into(),
    });
    app.loading = false;
    app.gen_id = 3;
    app.push_item(Item::Message(Message::user("go")));
    app.gen = Generation::Streaming {
        cancel: CancellationToken::new(),
        started: Instant::now(),
        first_at: None,
        deltas: 0,
        sent: 0,
    };
    app.update(
        Action::Stream(
            3,
            StreamEvent::ToolCall(ToolCall {
                id: None,
                name: "read_file".into(),
                arguments: json!({"path": "a.rs"}),
            }),
        ),
        &tx,
    );
    app.update(Action::Stream(3, StreamEvent::Done(Usage::default())), &tx);
    // the read ran, the next request could not be sent: the turn is closed
    assert!(app.items.iter().any(|i| matches!(i, Item::Step(_))));
    assert!(matches!(app.items.last(), Some(Item::Error(e)) if e.contains("provider unavailable")));
    assert!(!app.turn_active());
}

#[test]
fn the_boxes_stay_consistent() {
    let mut d = ToolsDialog {
        on: false,
        edit: false,
        create: false,
        rounds: 8,
        row: ToolsDialog::CREATE,
    };
    // creating needs reading: ticking it ticks the first box
    d.toggle();
    assert_eq!((d.on, d.edit, d.create), (true, false, true));
    d.row = ToolsDialog::EDIT;
    d.toggle();
    assert_eq!((d.on, d.edit, d.create), (true, true, true));
    // unticking a writing box leaves reading on
    d.toggle();
    assert_eq!((d.on, d.edit, d.create), (true, false, true));
    // unticking reading takes the others with it: all off is tools off
    d.row = ToolsDialog::ON;
    d.toggle();
    assert_eq!((d.on, d.edit, d.create), (false, false, false));
}
