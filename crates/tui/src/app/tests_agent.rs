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

use super::tests::{app, key, last_answer, type_text};
use super::*;
use moon_agent::Policy;

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

pub(super) fn app_with_fake(root: &Path, script: Vec<Vec<ChatEvent>>) -> (App, Tx, Rx, Requests) {
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
        agents_dir: None,
    });
    app.loading = false;
    assert!(app.current.is_some(), "{:?}", app.items);
    (app, tx, rx, requests)
}

/// Feeds the app what the fake provider streams, and what a command of the
/// model prints, until nothing is in flight: the turn is over, or an edit
/// or a command is waiting on screen.
pub(super) async fn settle(app: &mut App, tx: &Tx, rx: &mut Rx) {
    while app.is_streaming() || app.running.is_some() {
        let a = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("the provider answers")
            .expect("the channel is open");
        app.update(a, tx);
    }
}

pub(super) fn call(name: &str, args: serde_json::Value) -> ChatEvent {
    ChatEvent::ToolCall(ToolCall {
        id: None,
        name: name.into(),
        arguments: args,
    })
}

pub(super) fn done() -> ChatEvent {
    ChatEvent::Done(Usage {
        prompt_tokens: Some(10),
        completion_tokens: Some(2),
        ..Default::default()
    })
}

pub(super) fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.rs"), "hi\n").unwrap();
    dir
}

/// An agents folder as `moon config init` leaves it: default.toml and
/// reviewer.toml.
pub(super) fn agents_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (name, file) in moon_agent::AgentFile::factory() {
        std::fs::write(dir.path().join(moon_agent::file_name(name)), file.to_toml()).unwrap();
    }
    dir
}

/// `default`'s permissions as its definition holds them, on or off.
pub(super) fn yours(app: &App) -> Policy {
    app.def_of(DEFAULT_AGENT)
        .expect("default is defined")
        .policy
}

pub(super) fn text(spans: &[Span<'static>]) -> String {
    spans.iter().map(|s| s.content.to_string()).collect()
}

/// The cell cursor onto one capability, inside its group's table.
pub(super) fn cell_on(app: &mut App, id: &str) {
    let i = moon_agent::CATALOG
        .iter()
        .position(|e| e.id == id)
        .unwrap_or_else(|| panic!("{id} is not in the catalogue"));
    let cat = moon_agent::CATALOG[i].category;
    let at = group_rows(cat)
        .iter()
        .position(|r| *r == PermRow::Entry(i))
        .unwrap();
    let Some(Panel::Perms(p)) = app.panel.as_mut() else {
        panic!("expected the permissions table")
    };
    p.level = PermsLevel::Group(cat);
    p.perm = at;
}

/// The first-level cursor onto a group's row.
pub(super) fn groups_on(app: &mut App, cat: moon_agent::Category) {
    let at = moon_agent::Category::ALL
        .iter()
        .position(|x| *x == cat)
        .unwrap();
    let Some(Panel::Perms(p)) = app.panel.as_mut() else {
        panic!("expected the permissions table")
    };
    p.level = PermsLevel::Groups;
    p.row = at;
}

/// The cell cursor onto the step limit, inside `Editor`.
pub(super) fn steps_on(app: &mut App) {
    let cat = moon_agent::Category::Editor;
    let at = group_rows(cat)
        .iter()
        .position(|r| *r == PermRow::Steps)
        .unwrap();
    let Some(Panel::Perms(p)) = app.panel.as_mut() else {
        panic!("expected the permissions table")
    };
    p.level = PermsLevel::Group(cat);
    p.perm = at;
}

#[tokio::test]
async fn the_switch_and_what_it_shows() {
    use moon_core::config::ids::{CREATE_FILES, EDIT_FILES, READ_FILES};
    let dir = project();
    let (mut app, tx, _rx) = app();
    app.root = dir.path().to_path_buf();
    assert!(!app.tools_on);
    // off, the marker still names the agent everything would run through
    assert_eq!(app.edit_mode_span().unwrap().content, "⏵ default · all off");
    assert!(!text(&app.model_spans()).contains("edits"));

    // `/tools` is an alias now: the table over the chosen agent
    type_text(&mut app, &tx, "/tools");
    app.update(key(KeyCode::Enter), &tx);
    {
        let Some(Panel::Perms(p)) = &app.panel else {
            panic!("expected the permissions table")
        };
        assert_eq!(p.name, "default");
        assert!(!p.from_picker);
    }
    assert!(!app.tools_on);
    // enter turns a row on to what the catalogue says — allow for what
    // only looks, ask for what writes — saved at once, and the first one
    // already turns the tools on; ←→ walk off · ask · allow
    cell_on(&mut app, READ_FILES);
    app.update(key(KeyCode::Enter), &tx);
    assert!(app.tools_on && app.harness.is_some());
    assert_eq!(yours(&app).get(READ_FILES), moon_core::Permission::Allow);
    cell_on(&mut app, EDIT_FILES);
    app.update(key(KeyCode::Enter), &tx);
    cell_on(&mut app, CREATE_FILES);
    app.update(key(KeyCode::Enter), &tx);
    assert!(yours(&app).reads() && yours(&app).edits() && yours(&app).creates());
    assert_eq!(yours(&app).get(EDIT_FILES), moon_core::Permission::Ask);
    // the permanent sign, in `moon-soft` and bold, at the left of the
    // hints row: all three are on, so it lists all three
    let mode = app.edit_mode_span().expect("edit mode indicator");
    assert_eq!(mode.content, "⏵⏵ default · Read · Edit · Create");
    assert_eq!(mode.style.fg, Some(app.theme.moon_soft));
    assert!(!text(&app.model_spans()).contains("edits"));
    assert!(app.hints().contains("/agent"));
    // the agent's rules reach the system prompt
    assert!(app.system_prompt_for(&[]).unwrap().contains("no shell"));
    // esc steps back to the groups, and out
    app.update(key(KeyCode::Esc), &tx);
    assert!(matches!(&app.panel, Some(Panel::Perms(p)) if p.level == PermsLevel::Groups));
    app.update(key(KeyCode::Esc), &tx);
    assert!(app.panel.is_none());
    // not a git repository: said once, under the command's echo
    let answer = last_answer(&app);
    assert!(answer.contains("not a git repository"), "{answer}");

    // `←` on the editor group turns your side of the whole of it off:
    // nothing granted is tools off
    type_text(&mut app, &tx, "/tools");
    app.update(key(KeyCode::Enter), &tx);
    groups_on(&mut app, moon_agent::Category::Editor);
    app.update(key(KeyCode::Left), &tx);
    assert!(!app.tools_on && app.harness.is_none());
    assert!(yours(&app).is_empty());
    assert!(app.system_prompt_for(&[]).is_none());
    let notice = app.notice.clone().map(|(x, _)| x).unwrap_or_default();
    assert!(notice.contains("tools off"), "{notice}");
    app.update(key(KeyCode::Esc), &tx);
    assert!(app.panel.is_none());

    // whatever follows the command is ignored, as with `/model`
    type_text(&mut app, &tx, "/tools on");
    app.update(key(KeyCode::Enter), &tx);
    assert!(matches!(app.panel, Some(Panel::Perms(_))));
}

#[tokio::test]
async fn the_table_turns_it_on_and_tunes_it() {
    use moon_core::config::ids::{EDIT_FILES, READ_FILES};
    let dir = project();
    let (mut app, tx, _rx) = app();
    app.root = dir.path().to_path_buf();
    type_text(&mut app, &tx, "/tools");
    app.update(key(KeyCode::Enter), &tx);
    {
        let Some(Panel::Perms(p)) = &app.panel else {
            panic!("expected the permissions table")
        };
        assert_eq!(p.found.len(), moon_agent::CATALOG.len());
    }
    assert!(yours(&app).is_empty());
    assert_eq!(app.steps_of(DEFAULT_AGENT), 8);
    // reading to allow, editing to ask, and the step limit: a digit fixes
    // it, enter adds one; every step saved on the spot
    cell_on(&mut app, READ_FILES);
    app.update(key(KeyCode::Enter), &tx);
    cell_on(&mut app, EDIT_FILES);
    app.update(key(KeyCode::Enter), &tx);
    steps_on(&mut app);
    app.update(key(KeyCode::Char('5')), &tx);
    assert_eq!(app.steps_of(DEFAULT_AGENT), 5);
    app.update(key(KeyCode::Enter), &tx);
    assert_eq!(app.steps_of(DEFAULT_AGENT), 6);
    assert!(app.tools_on);
    let h = app.harness.as_ref().unwrap();
    assert!(!h.agent().has(moon_agent::Tool::WriteFile));
    assert_eq!(h.limits().rounds, 6);
    app.update(key(KeyCode::Esc), &tx);
    app.update(key(KeyCode::Esc), &tx);
    assert!(app.panel.is_none());
    // reopened it shows what is set; the editor rule off is tools off
    type_text(&mut app, &tx, "/tools");
    app.update(key(KeyCode::Enter), &tx);
    assert!(yours(&app).reads() && yours(&app).edits() && !yours(&app).creates());
    assert_eq!(app.steps_of(DEFAULT_AGENT), 6);
    groups_on(&mut app, moon_agent::Category::Editor);
    app.update(key(KeyCode::Left), &tx);
    assert!(!app.tools_on);
    assert!(app.harness.is_none());
}

#[tokio::test]
async fn read_only_is_the_reader_and_says_so() {
    let dir = project();
    let (mut app, tx, _rx) = app();
    app.root = dir.path().to_path_buf();
    // `read files` alone: the writing rows stay off
    type_text(&mut app, &tx, "/tools");
    app.update(key(KeyCode::Enter), &tx);
    cell_on(&mut app, moon_core::config::ids::READ_FILES);
    app.update(key(KeyCode::Enter), &tx);
    assert!(yours(&app).reads() && !yours(&app).edits() && !yours(&app).creates());
    app.update(key(KeyCode::Esc), &tx);
    app.update(key(KeyCode::Esc), &tx);
    assert!(app.panel.is_none());
    assert!(app.tools_on);
    assert_eq!(app.tools_scope(), (false, false));
    assert!(!app.tools_write());
    let h = app.harness.as_ref().unwrap();
    assert_eq!(h.agent().name, "reader");
    assert_eq!(h.specs().len(), 2);
    // the marker, the notice and the prompt all say it only reads
    assert_eq!(app.edit_mode_span().unwrap().content, "⏵⏵ default · Read");
    let prompt = app.system_prompt_for(&[]).unwrap();
    assert!(prompt.contains("cannot change files") && !prompt.contains("edit_file"));
    // and the table, reopened, shows the writing rows off; `edit` back on
    // gives the editor without write_file
    type_text(&mut app, &tx, "/tools");
    app.update(key(KeyCode::Enter), &tx);
    assert!(yours(&app).reads() && !yours(&app).edits() && !yours(&app).creates());
    cell_on(&mut app, moon_core::config::ids::EDIT_FILES);
    app.update(key(KeyCode::Enter), &tx);
    app.update(key(KeyCode::Esc), &tx);
    app.update(key(KeyCode::Esc), &tx);
    assert_eq!(app.tools_scope(), (true, false));
    assert_eq!(
        app.edit_mode_span().unwrap().content,
        "⏵⏵ default · Read · Edit"
    );
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
            agents_dir: None,
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
    assert_eq!(app.edit_mode_span().unwrap().content, "⏵⏵ default · Read");

    // and one box alone
    tools.edit = true;
    let app = at_startup(tools, dir.path());
    assert_eq!(app.tools_scope(), (true, false));
    assert_eq!(
        app.edit_mode_span().unwrap().content,
        "⏵⏵ default · Read · Edit"
    );
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
    let at = shown
        .iter()
        .position(|l| l.contains("  ⎿  read  a.rs"))
        .expect("step line");
    // it hangs from the request, with no blank row between
    assert!(shown[at - 1].starts_with("▌ "), "{shown:?}");
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
        shown.iter().any(|l| l.contains("  ⎿  read  a.rs")),
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
    assert_eq!(a.title(), "Edit file");
    assert_eq!(a.subject(), "a.rs");
    assert_eq!(a.info(), "+1 −1");
    assert_eq!(
        a.question(),
        ("Do you want to make this edit to ", "a.rs", "?")
    );
    assert_eq!(a.choice, EditChoice::Yes);
    // the cursor walks the two choices; enter takes the one it is on
    app.update(key(KeyCode::Down), &tx);
    assert!(matches!(&app.panel, Some(Panel::Approval(a)) if a.choice == EditChoice::No));
    app.update(key(KeyCode::Up), &tx);
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
    app.update(key(KeyCode::Char('n')), &tx);
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
            .any(|l| l.contains("✗ read  ../outside.txt · ") && l.contains("outside the project")),
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

#[tokio::test]
async fn a_failed_step_says_why_on_the_same_line() {
    let (mut app, _tx, _rx) = app();
    app.items.clear();
    app.push_item(Item::Step(moon_agent::Step {
        tool: moon_agent::Tool::EditFile,
        path: "a.rs".into(),
        added: 0,
        removed: 0,
        outcome: moon_agent::Outcome::Failed("`a.rs` was not read".into()),
    }));
    let shown: Vec<String> = app
        .visible_lines(80, 40)
        .iter()
        .map(|l| l.to_string())
        .collect();
    assert!(
        shown
            .iter()
            .any(|l| l == "  ⎿  ✗ edit  a.rs · `a.rs` was not read"),
        "{shown:?}"
    );
}

#[tokio::test]
async fn an_agent_runs_on_its_own_permissions_and_the_session_remembers_it() {
    use moon_core::config::ids::{EDIT_FILES, READ_FILES};
    use moon_core::Permission;
    let dir = project();
    let agents = agents_dir();
    std::fs::write(
        agents.path().join("committer.toml"),
        "description = \"stages and commits what you approve\"\n\
         inherit     = false\n\
         max_steps   = 6\n\
         prompt      = \"You prepare commits.\"\n\n\
         [permissions]\n\
         \"read files\" = \"allow\"\n\
         \"git diff\"   = \"allow\"\n\
         \"git add\"    = \"ask\"\n\
         \"git commit\" = \"ask\"\n",
    )
    .unwrap();
    let (mut app, tx, _rx) = app();
    app.root = dir.path().to_path_buf();
    app.agents_dir = Some(agents.path().to_path_buf());
    // default's permissions: reading, editing, and two git commands
    app.set_policy(Policy::from_pairs([
        (READ_FILES, Permission::Allow),
        (EDIT_FILES, Permission::Allow),
        ("git diff", Permission::Allow),
        ("git commit", Permission::Ask),
    ]));
    assert!(app.tools_on);
    assert_eq!(app.harness.as_ref().unwrap().agent().name, "editor");
    assert_eq!(
        text(&[app.edit_mode_span().unwrap()]),
        "⏵⏵ default · Read · Edit · 2 commands"
    );

    // `/agent` is a picker like `/model`: one row per file of the
    // folder, default first and the rest by name
    type_text(&mut app, &tx, "/agent");
    app.update(key(KeyCode::Enter), &tx);
    let Some(Panel::Agents(p)) = &app.panel else {
        panic!("expected the agent picker")
    };
    let names: Vec<String> = p
        .rows()
        .iter()
        .filter_map(|r| match r {
            crate::picker::Row::Item(it, _, _) => Some(it.label.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(names, vec!["default", "committer", "reviewer"]);
    assert!(p.current().unwrap().active, "default is the one on");
    // choose the reviewer: the loop runs on ITS permissions, whole —
    // choosing an agent is choosing its policy
    app.update(key(KeyCode::Down), &tx);
    app.update(key(KeyCode::Down), &tx);
    app.update(key(KeyCode::Enter), &tx);
    assert!(app.panel.is_none());
    assert_eq!(app.agent, "reviewer");
    assert!(
        last_answer(&app).contains("set agent to reviewer"),
        "{}",
        last_answer(&app)
    );
    let a = app.harness.as_ref().unwrap().agent().clone();
    assert_eq!(a.name, "reviewer");
    assert!(a.has(moon_agent::Tool::ReadFile) && !a.has(moon_agent::Tool::EditFile));
    assert_eq!(
        a.command_ids(),
        vec!["git status", "git diff", "git log", "git show", "git blame"]
    );
    assert!(app.system_prompt_for(&[]).unwrap().contains("# Reviewing"));
    // the marker says the agent's own; default's file stays as it was
    assert_eq!(
        text(&[app.edit_mode_span().unwrap()]),
        "⏵⏵ reviewer · Read · 5 commands"
    );
    assert!(yours(&app).edits());
    let saved = moon_agent::AgentFile::load(&agents.path().join("default.toml")).unwrap();
    assert_eq!(saved.policy(), yours(&app));

    // the agent from the file carries its own step limit: default keeps
    // 8, 6 is what the harness gets, and its commands are its own — git
    // add runs (asking) although default never named it
    app.select_agent("committer");
    let h = app.harness.as_ref().unwrap();
    assert_eq!(h.agent().prompt, "You prepare commits.");
    assert_eq!(h.limits().rounds, 6);
    assert_eq!(app.steps_of(DEFAULT_AGENT), 8);
    assert_eq!(
        h.agent().command_ids(),
        vec!["git diff", "git add", "git commit"]
    );

    // a resumed session carries the name; one that is gone falls back
    let meta: SessionMeta = serde_json::from_str(
        r#"{"id":"x","title":"t","created_at":"2026-01-01T00:00:00Z","tools":true,"agent":"ghost"}"#,
    )
    .unwrap();
    app.apply_session(Session {
        meta,
        messages: Vec::new(),
    });
    assert_eq!(app.agent, DEFAULT_AGENT);
    let notice = app
        .notice
        .as_ref()
        .map(|(n, _)| n.clone())
        .unwrap_or_default();
    assert!(notice.contains("back to default"), "{notice}");
    assert_eq!(app.harness.as_ref().unwrap().agent().name, "editor");
}

#[tokio::test]
async fn agents_are_made_edited_and_deleted_from_the_picker() {
    let ctrl = |c| Action::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
    let dir = project();
    let agents = agents_dir();
    std::fs::write(
        agents.path().join("trusty.toml"),
        "description = \"x\"\ninherit = true\nprompt = \"y\"\n",
    )
    .unwrap();
    let (mut app, tx, _rx) = app();
    app.root = dir.path().to_path_buf();
    app.agents_dir = Some(agents.path().to_path_buf());

    // ctrl+a asks for a name; the template is written and its table
    // opens, ready to edit
    type_text(&mut app, &tx, "/agent");
    app.update(key(KeyCode::Enter), &tx);
    assert!(matches!(app.panel, Some(Panel::Agents(_))));
    app.update(ctrl('a'), &tx);
    assert!(matches!(
        app.panel,
        Some(Panel::AgentAction {
            action: AgentAction::New { .. },
            ..
        })
    ));
    // a bad name is said and the input stays
    type_text(&mut app, &tx, "Bad Name");
    app.update(key(KeyCode::Enter), &tx);
    let notice = |app: &App| app.notice.clone().map(|(n, _)| n).unwrap_or_default();
    assert!(notice(&app).contains("lowercase"), "{}", notice(&app));
    for _ in 0.."Bad Name".len() {
        app.update(key(KeyCode::Backspace), &tx);
    }
    type_text(&mut app, &tx, "tester");
    app.update(key(KeyCode::Enter), &tx);
    let path = agents.path().join("tester.toml");
    assert!(path.exists());
    {
        let Some(Panel::Perms(p)) = &app.panel else {
            panic!("expected the permissions table")
        };
        assert_eq!((p.name.as_str(), p.from_picker), ("tester", true));
    }

    // enter on `git status` lists it in the agent's file at what the
    // catalogue says, written at once
    cell_on(&mut app, "git status");
    app.update(key(KeyCode::Enter), &tx);
    let saved = moon_agent::AgentFile::load(&path).unwrap();
    assert_eq!(
        saved.permissions.get("git status"),
        Some(&moon_core::Permission::Allow),
        "{saved:?}"
    );
    // what the table does not edit is kept from the template
    assert_eq!(
        saved.description,
        moon_agent::AgentFile::template().description
    );
    assert_eq!(saved.max_steps, Some(8), "the template's own limit");
    // enter again turns it off, in the file: it lists everything there
    // is, so the line stays, set to off
    app.update(key(KeyCode::Enter), &tx);
    let saved = moon_agent::AgentFile::load(&path).unwrap();
    assert_eq!(
        saved.permissions.get("git status"),
        Some(&moon_core::Permission::Off),
        "{saved:?}"
    );
    // esc steps out to the groups, and back to the picker it came from
    app.update(key(KeyCode::Esc), &tx);
    app.update(key(KeyCode::Esc), &tx);
    assert!(matches!(app.panel, Some(Panel::Agents(_))));

    // default's table edits default.toml, a file like any other — while
    // renaming default is refused, since every conversation starts there
    app.update(ctrl('t'), &tx);
    {
        let Some(Panel::Perms(p)) = &app.panel else {
            panic!("expected the permissions table")
        };
        assert_eq!(p.name, "default");
    }
    let default_file = agents.path().join("default.toml");
    cell_on(&mut app, "git status");
    app.update(key(KeyCode::Enter), &tx);
    assert_eq!(yours(&app).get("git status"), moon_core::Permission::Allow);
    assert_eq!(
        moon_agent::AgentFile::load(&default_file)
            .unwrap()
            .policy()
            .get("git status"),
        moon_core::Permission::Allow
    );
    app.update(key(KeyCode::Enter), &tx);
    assert_eq!(yours(&app).get("git status"), moon_core::Permission::Off);
    app.update(key(KeyCode::Esc), &tx);
    app.update(key(KeyCode::Esc), &tx);
    app.update(ctrl('r'), &tx);
    assert!(
        notice(&app).contains("cannot be renamed"),
        "{}",
        notice(&app)
    );

    // an old file with `inherit` still loads — the line is ignored, its
    // `[permissions]` are the whole truth — and edits like any other
    app.update(key(KeyCode::Down), &tx);
    app.update(key(KeyCode::Down), &tx);
    app.update(key(KeyCode::Down), &tx);
    app.update(ctrl('t'), &tx);
    {
        let Some(Panel::Perms(p)) = &app.panel else {
            panic!("expected the permissions table")
        };
        assert_eq!(p.name, "trusty");
    }
    cell_on(&mut app, "git status");
    app.update(key(KeyCode::Enter), &tx);
    let trusty = agents.path().join("trusty.toml");
    let f = moon_agent::AgentFile::load(&trusty).unwrap();
    assert_eq!(
        f.permissions.get("git status"),
        Some(&moon_core::Permission::Allow),
        "{f:?}"
    );
    app.update(key(KeyCode::Enter), &tx);
    let f = moon_agent::AgentFile::load(&trusty).unwrap();
    assert!(f.policy().is_empty(), "{f:?}");
    app.update(key(KeyCode::Esc), &tx);
    app.update(key(KeyCode::Esc), &tx);

    // ctrl+r renames the file — the name is the file — and a selection
    // that named it follows
    app.select_agent("tester");
    assert_eq!(app.agent, "tester");
    app.update(key(KeyCode::Down), &tx);
    app.update(key(KeyCode::Down), &tx);
    app.update(ctrl('r'), &tx);
    match &app.panel {
        Some(Panel::AgentAction {
            action: AgentAction::Rename { name, input },
            ..
        }) => assert_eq!((name.as_str(), input.as_str()), ("tester", "tester")),
        other => panic!("expected the rename dialog, got {:?}", other.is_some()),
    }
    for _ in 0.."tester".len() {
        app.update(key(KeyCode::Backspace), &tx);
    }
    type_text(&mut app, &tx, "probe");
    app.update(key(KeyCode::Enter), &tx);
    assert!(!path.exists());
    let renamed = agents.path().join("probe.toml");
    assert!(renamed.exists());
    assert_eq!(app.agent, "probe");
    assert!(matches!(app.panel, Some(Panel::Agents(_))));

    // deleting the selected agent falls back to default; the fresh picker
    // opens with the cursor on the active one, probe
    app.update(ctrl('d'), &tx);
    assert!(matches!(
        app.panel,
        Some(Panel::AgentAction {
            action: AgentAction::Delete { .. },
            ..
        })
    ));
    app.update(key(KeyCode::Enter), &tx);
    assert!(!renamed.exists());
    assert_eq!(app.agent, DEFAULT_AGENT);
    // said while the panel was down: it lands in the command's echo
    let echoed = app
        .echo
        .as_ref()
        .map(|(_, o)| o.join(" · "))
        .unwrap_or_default();
    assert!(echoed.contains("back to default"), "{echoed}");
    assert!(matches!(app.panel, Some(Panel::Agents(_))));
}

#[tokio::test]
async fn slash_tools_is_an_alias_of_the_table() {
    let dir = project();
    let agents = tempfile::tempdir().unwrap();
    std::fs::write(
        agents.path().join("committer.toml"),
        "description = \"x\"\ninherit = false\nprompt = \"y\"\n\n[permissions]\n\"read files\" = \"allow\"\n",
    )
    .unwrap();
    let (mut app, tx, _rx) = app();
    app.root = dir.path().to_path_buf();
    app.agents_dir = Some(agents.path().to_path_buf());
    let _ = app.reload_agents();
    app.select_agent("committer");
    // `/tools` opens the table over the chosen agent
    type_text(&mut app, &tx, "/tools");
    app.update(key(KeyCode::Enter), &tx);
    let Some(Panel::Perms(p)) = &app.panel else {
        panic!("expected the permissions table")
    };
    assert_eq!((p.name.as_str(), p.from_picker), ("committer", false));
}
