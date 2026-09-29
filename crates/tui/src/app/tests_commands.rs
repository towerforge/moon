//! The model running commands, from the interface: the commands in the
//! permissions table, the cells stepping within their bounds, a turn
//! that runs one and reads what it printed, one that waits for the ok, a
//! refusal on screen, `Esc` killing one that runs, and the agent files.
//! The fake provider and the helpers are `tests_agent`'s.

use std::time::Duration;

use moon_agent::CATALOG;
use moon_core::config::ids::{EDIT_FILES, READ_FILES};
use moon_core::{Permission, ToolSpec, ToolsConfig};
use serde_json::json;

use super::tests::{app, key, last_answer, type_text};
use super::tests_agent::{
    agents_dir, app_with_fake, call, cell_on, done, groups_on, project, settle, steps_on, text,
    yours,
};
use super::*;
use moon_agent::Policy;

/// The permissions a conversation starts with, for the fake provider.
fn allow(app: &mut App, pairs: &[(&str, Permission)]) {
    for (id, p) in pairs {
        app.cfg.tools.permissions.insert(id.to_string(), *p);
    }
    app.enable_tools().unwrap();
}

#[tokio::test]
async fn the_table_edits_each_agent_where_it_lives() {
    use moon_agent::Category;
    let dir = project();
    let agents = agents_dir();
    let path = agents.path().join("committer.toml");
    std::fs::write(&path, "description = \"x\"\nprompt = \"y\"\n").unwrap();
    let (mut app, tx, _rx) = app();
    app.root = dir.path().to_path_buf();
    app.agents_dir = Some(agents.path().to_path_buf());
    // opening the table reads the folder: default's file has reading on
    app.open_perms("default", false);
    assert!(app.tools_on);
    cell_on(&mut app, READ_FILES);
    app.update(key(KeyCode::Enter), &tx);
    assert!(!app.tools_on && yours(&app).is_empty());

    // default's table edits its file: ←→ walk off · ask · allow with a
    // stop at each end, enter toggles, and the switch follows
    cell_on(&mut app, "git diff");
    app.update(key(KeyCode::Right), &tx);
    assert_eq!(yours(&app).get("git diff"), Permission::Ask);
    assert!(app.tools_on);
    app.update(key(KeyCode::Right), &tx);
    assert_eq!(yours(&app).get("git diff"), Permission::Allow);
    app.update(key(KeyCode::Right), &tx);
    assert_eq!(yours(&app).get("git diff"), Permission::Allow, "clamped");
    app.update(key(KeyCode::Enter), &tx);
    assert_eq!(yours(&app).get("git diff"), Permission::Off);
    assert!(!app.tools_on);
    // where commands run knows no ask
    cell_on(&mut app, "commands in subfolders");
    app.update(key(KeyCode::Right), &tx);
    assert!(yours(&app).subfolders());
    app.update(key(KeyCode::Enter), &tx);
    assert!(!yours(&app).subfolders());
    // a group as a whole, from the first level: → its defaults, ← off
    groups_on(&mut app, Category::Git);
    app.update(key(KeyCode::Right), &tx);
    assert_eq!(yours(&app).get("git status"), Permission::Allow);
    assert_eq!(yours(&app).get("git commit"), Permission::Ask);
    app.update(key(KeyCode::Left), &tx);
    assert_eq!(yours(&app).get("git status"), Permission::Off);
    assert!(!yours(&app).allows("git commit"));

    // a named agent's table edits its own file, whatever default says:
    // enter turns the row on, enter again off
    let named = |path: &std::path::Path, id: &str| {
        moon_agent::AgentFile::load(path).unwrap().policy().get(id)
    };
    app.open_perms("committer", false);
    cell_on(&mut app, "git add");
    app.update(key(KeyCode::Enter), &tx);
    assert_eq!(named(&path, "git add"), Permission::Ask);
    app.update(key(KeyCode::Right), &tx);
    assert_eq!(named(&path, "git add"), Permission::Allow);
    app.update(key(KeyCode::Enter), &tx);
    assert_eq!(named(&path, "git add"), Permission::Off);
    assert_eq!(
        yours(&app).get("git add"),
        Permission::Off,
        "default untouched"
    );
    // its whole git group to defaults, in its file
    groups_on(&mut app, Category::Git);
    app.update(key(KeyCode::Right), &tx);
    assert_eq!(named(&path, "git status"), Permission::Allow);
    assert_eq!(named(&path, "git commit"), Permission::Ask);
    app.update(key(KeyCode::Left), &tx);
    assert_eq!(named(&path, "git status"), Permission::Off);

    // the step limit: a digit fixes it, ←→ walk it, in the agent's file
    steps_on(&mut app);
    app.update(key(KeyCode::Char('4')), &tx);
    assert_eq!(
        moon_agent::AgentFile::load(&path).unwrap().max_steps,
        Some(4)
    );
    app.update(key(KeyCode::Right), &tx);
    assert_eq!(
        moon_agent::AgentFile::load(&path).unwrap().max_steps,
        Some(5)
    );
    // what the cells did not touch is kept, and the file now lists
    // everything there is
    let f = moon_agent::AgentFile::load(&path).unwrap();
    assert_eq!(f.description.as_str(), "x");
    assert!(f.stale().is_none());

    // the reviewer is a file like any other: its table edits it
    app.open_perms("reviewer", false);
    cell_on(&mut app, "git diff");
    app.update(key(KeyCode::Enter), &tx);
    let reviewer = agents.path().join("reviewer.toml");
    assert_eq!(named(&reviewer, "git diff"), Permission::Off);
    assert_eq!(named(&reviewer, "git status"), Permission::Allow);
}

#[cfg(unix)]
#[tokio::test]
async fn commands_are_set_in_the_view_like_anything_else() {
    let dir = project();
    let (mut app, tx, _rx) = app();
    app.root = dir.path().to_path_buf();
    type_text(&mut app, &tx, "/tools");
    app.update(key(KeyCode::Enter), &tx);
    // `ls` to allow: a command does not bring reading with it, it is a
    // choice of its own
    cell_on(&mut app, "ls");
    app.update(key(KeyCode::Enter), &tx);
    assert_eq!(yours(&app).get("ls"), Permission::Allow);
    assert!(!yours(&app).reads());
    // `git add` asks by default; the user may let it run on its own
    cell_on(&mut app, "git add");
    app.update(key(KeyCode::Enter), &tx);
    assert_eq!(yours(&app).get("git add"), Permission::Ask);
    app.update(key(KeyCode::Right), &tx);
    assert_eq!(yours(&app).get("git add"), Permission::Allow);
    app.update(key(KeyCode::Esc), &tx);
    app.update(key(KeyCode::Esc), &tx);
    assert!(app.panel.is_none());
    assert!(app.tools_on);
    // the agent has the tool and nothing else, and the marker counts
    assert_eq!(app.tools_commands(), vec!["ls", "git add"]);
    let h = app.harness.as_ref().unwrap();
    assert!(h.agent().has(moon_agent::Tool::RunCommand));
    assert!(!h.agent().has(moon_agent::Tool::ReadFile));
    assert_eq!(h.agent().name, "reader");
    assert_eq!(
        app.edit_mode_span().unwrap().content,
        "⏵⏵ default · 2 commands"
    );
    let prompt = app.system_prompt_for(&[]).unwrap();
    assert!(prompt.contains("run_command: ls, git add"), "{prompt}");
    assert!(
        !prompt.contains("no shell") && !prompt.contains("wait for the user"),
        "{prompt}"
    );
    // reopened, the table shows what is set; enter turns a row off in
    // one step, and both off is tools off
    type_text(&mut app, &tx, "/tools");
    app.update(key(KeyCode::Enter), &tx);
    assert_eq!(yours(&app).get("git add"), Permission::Allow);
    cell_on(&mut app, "ls");
    app.update(key(KeyCode::Enter), &tx);
    cell_on(&mut app, "git add");
    app.update(key(KeyCode::Enter), &tx);
    assert!(!app.tools_on);
    assert!(app.tools_commands().is_empty());
    // everything off again: the marker falls back to naming the agent
    assert_eq!(app.edit_mode_span().unwrap().content, "⏵ default · all off");
}

#[cfg(unix)]
#[tokio::test]
async fn a_turn_runs_a_command_and_reads_what_it_printed() {
    let dir = project();
    let (mut app, tx, mut rx, requests) = app_with_fake(
        dir.path(),
        vec![
            vec![
                call("run_command", json!({"command": "ls", "args": ["-1"]})),
                done(),
            ],
            vec![ChatEvent::Delta("there is a.rs".into()), done()],
        ],
    );
    allow(
        &mut app,
        &[(READ_FILES, Permission::Allow), ("ls", Permission::Allow)],
    );
    assert_eq!(app.tools_commands(), vec!["ls"]);
    type_text(&mut app, &tx, "what is in the project");
    app.update(key(KeyCode::Enter), &tx);
    settle(&mut app, &tx, &mut rx).await;
    assert!(!app.turn_active());
    assert!(app.running.is_none());
    // the request offered run_command with `ls` in it
    let reqs = requests.lock().unwrap();
    assert_eq!(reqs.len(), 2);
    let run = reqs[0]
        .tools
        .iter()
        .find(|t: &&ToolSpec| t.name == "run_command")
        .expect("run_command offered");
    assert!(
        run.description.contains("only these: ls"),
        "{}",
        run.description
    );
    // the model got the listing back as the tool's result
    let m = &reqs[1].messages;
    let last = m.last().unwrap();
    assert_eq!(last.role, Role::Tool);
    assert_eq!(last.tool_name.as_deref(), Some("run_command"));
    assert!(last.content.contains("a.rs"), "{}", last.content);
    drop(reqs);
    // on screen: the step line with the command, and the answer
    assert!(app.items.iter().any(
        |i| matches!(i, Item::Step(s) if s.tool == moon_agent::Tool::RunCommand && s.path == "ls -1" && s.outcome == moon_agent::Outcome::Done)
    ));
    let shown: Vec<String> = app
        .visible_lines(80, 40)
        .iter()
        .map(|l| l.to_string())
        .collect();
    assert!(
        shown.iter().any(|l| l.contains("  ⎿  run   ls -1")),
        "{shown:?}"
    );
    assert_eq!(app.messages().next_back().unwrap().content, "there is a.rs");
    assert_eq!(
        app.edit_mode_span().unwrap().content,
        "⏵⏵ default · Read · 1 command"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_command_set_to_ask_waits_for_the_ok() {
    let dir = project();
    let (mut app, tx, mut rx, requests) = app_with_fake(
        dir.path(),
        vec![
            vec![
                call(
                    "run_command",
                    json!({"command": "git add", "args": ["a.rs"]}),
                ),
                done(),
            ],
            vec![ChatEvent::Delta("staged".into()), done()],
        ],
    );
    allow(&mut app, &[("git add", Permission::Ask)]);
    type_text(&mut app, &tx, "stage it");
    app.update(key(KeyCode::Enter), &tx);
    settle(&mut app, &tx, &mut rx).await;
    // paused on the command: on screen, not run
    assert!(app.waiting_approval());
    assert!(app.turn_active());
    let Some(Panel::Approval(a)) = &app.panel else {
        panic!("expected the approval panel")
    };
    assert_eq!(a.title(), "Run command");
    assert_eq!(a.subject(), "git add");
    assert_eq!(a.info(), "");
    assert_eq!(a.question(), ("Do you want to run ", "git add", "?"));
    assert_eq!(a.exec().unwrap().line, "git add a.rs");
    assert!(a.edit().is_none());
    let activity = text(&app.activity_spans().unwrap());
    assert!(activity.contains("waiting for your approval") && activity.contains("1 yes · 2 no"));
    assert!(app.panel_keys().contains(&("1/2", "yes/no")));
    // skipped: nothing ran, the model hears so, the turn goes on
    app.update(key(KeyCode::Char('2')), &tx);
    settle(&mut app, &tx, &mut rx).await;
    assert!(!app.turn_active());
    assert!(app
        .items
        .iter()
        .any(|i| matches!(i, Item::Step(s) if s.tool == moon_agent::Tool::RunCommand && s.outcome == moon_agent::Outcome::Skipped)));
    let reqs = requests.lock().unwrap();
    let last = reqs[1].messages.last().unwrap();
    assert!(
        last.content.starts_with("skipped by the user"),
        "{}",
        last.content
    );
    assert_eq!(app.messages().next_back().unwrap().content, "staged");
}

#[cfg(unix)]
#[tokio::test]
async fn a_command_that_is_off_is_refused_on_screen() {
    let dir = project();
    let (mut app, tx, mut rx, _requests) = app_with_fake(
        dir.path(),
        vec![
            vec![call("run_command", json!({"command": "git diff"})), done()],
            vec![ChatEvent::Delta("ok".into()), done()],
        ],
    );
    allow(&mut app, &[("ls", Permission::Allow)]);
    type_text(&mut app, &tx, "go");
    app.update(key(KeyCode::Enter), &tx);
    settle(&mut app, &tx, &mut rx).await;
    assert!(!app.turn_active());
    let shown: Vec<String> = app
        .visible_lines(120, 40)
        .iter()
        .map(|l| l.to_string())
        .collect();
    assert!(
        shown
            .iter()
            .any(|l| l.contains("✗ run   git diff · ") && l.contains("not allowed")),
        "{shown:?}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn esc_kills_a_running_command_and_closes_the_turn() {
    let dir = project();
    let (mut app, tx, mut rx, requests) = app_with_fake(
        dir.path(),
        vec![vec![
            call(
                "run_command",
                json!({"command": "tail", "args": ["-f", "a.rs"]}),
            ),
            done(),
        ]],
    );
    allow(&mut app, &[("tail", Permission::Allow)]);
    type_text(&mut app, &tx, "follow it");
    app.update(key(KeyCode::Enter), &tx);
    // the reply is in and the command is running: nothing streams, the
    // turn is open, and the status row says what runs
    while app.is_streaming() {
        let a = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        app.update(a, &tx);
    }
    assert!(app.running.is_some());
    assert!(app.turn_active() && !app.waiting_approval());
    assert_eq!(app.running_command().unwrap().line, "tail -f a.rs");
    assert!(app.needs_tick());
    assert!(text(&app.activity_spans().unwrap()).contains("running"));
    // esc: killed, closed as not run, and its late result is dropped
    app.update(key(KeyCode::Esc), &tx);
    assert!(app.running.is_none());
    assert!(!app.turn_active());
    let last = app.messages().next_back().unwrap();
    assert_eq!(last.role, Role::Tool);
    assert!(last.content.contains("cancelled"));
    // whatever else is on the channel first (the model being polled), the
    // thread reports the kill in the end
    let ran = loop {
        let a = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("the thread reports")
            .unwrap();
        if matches!(a, Action::Ran(..)) {
            break a;
        }
        app.update(a, &tx);
    };
    assert!(
        matches!(&ran, Action::Ran(_, out) if out.cancelled),
        "{ran:?}"
    );
    app.update(ran, &tx);
    assert!(!app.turn_active());
    assert_eq!(requests.lock().unwrap().len(), 1);
}

#[cfg(unix)]
#[test]
fn the_agent_files_are_the_truth_and_are_read_back() {
    let dir = project();
    let conf = dir.path().join("conf");
    let agents = conf.join("agents");
    let file = agents.join("default.toml");
    let boot = |tools: ToolsConfig| {
        let (tx, rx) = mpsc::unbounded_channel();
        let app = App::new(RunOptions {
            config: Config {
                tools,
                ..Default::default()
            },
            config_source: ConfigSource::Default(conf.join("config.toml")),
            registry: Arc::new(Registry::new()),
            store: None,
            resume: None,
            model: None,
            version: "0.0.0".into(),
            cwd: "~/p".into(),
            root: dir.path().to_path_buf(),
            state_dir: None,
            agents_dir: Some(agents.clone()),
        });
        (app, tx, rx)
    };
    let load = |path: &std::path::Path| moon_agent::AgentFile::load(path).unwrap();
    // no folder yet: default.toml is written with reading on, said once,
    // and the tools are on with it
    let (mut app, tx, _rx) = boot(ToolsConfig::default());
    assert!(file.exists());
    assert!(
        app.items
            .iter()
            .any(|i| matches!(i, Item::Info(s) if s.contains("default.toml written"))),
        "{:?}",
        app.items
    );
    assert!(app.tools_on);
    assert_eq!(app.tools_commands(), Vec::<&str>::new());
    assert!(!app.tools_write());
    // every cell step writes it, with what is set: editing and `ls`
    type_text(&mut app, &tx, "/tools");
    app.update(key(KeyCode::Enter), &tx);
    cell_on(&mut app, EDIT_FILES);
    app.update(key(KeyCode::Enter), &tx);
    cell_on(&mut app, "ls");
    app.update(key(KeyCode::Enter), &tx);
    app.update(key(KeyCode::Esc), &tx);
    app.update(key(KeyCode::Esc), &tx);
    let saved = load(&file);
    assert_eq!(saved.max_steps, Some(8));
    // the whole catalogue is in it, what is off said off
    let text = std::fs::read_to_string(&file).unwrap();
    for e in CATALOG {
        assert!(text.contains(&format!("\"{}\"", e.id)), "{}", e.id);
    }
    assert!(text.contains("# Network — "), "{text}");
    let curl = text.lines().find(|l| l.starts_with("\"curl\"")).unwrap();
    assert!(curl.contains("= \"off\""), "{curl}");
    assert_eq!(
        saved.policy(),
        Policy::from_pairs([
            (READ_FILES, Permission::Allow),
            (EDIT_FILES, Permission::Ask),
            ("ls", Permission::Allow),
        ])
    );
    // edited by hand: the next message goes out with what it says, even
    // one that cannot be sent
    std::fs::write(
        &file,
        "max_steps = 5\n[permissions]\n\"read files\" = \"allow\"\n\"create new files\" = \"ask\"\n\"git diff\" = \"allow\"\n\"cat\" = \"ask\"\n",
    )
    .unwrap();
    type_text(&mut app, &tx, "hello");
    app.update(key(KeyCode::Enter), &tx);
    app.input.clear();
    assert_eq!(app.tools_scope(), (false, true));
    assert_eq!(app.tools_commands(), vec!["cat", "git diff"]);
    assert_eq!(app.policy().get("cat"), Permission::Ask);
    assert_eq!(app.harness.as_ref().unwrap().limits().rounds, 5);
    assert_eq!(
        app.edit_mode_span().unwrap().content,
        "⏵⏵ default · Read · Create · 2 commands"
    );
    // …and the short file was brought up to the catalogue on the way,
    // with what it set kept
    let synced = load(&file);
    assert!(synced.stale().is_none());
    assert_eq!(synced.max_steps, Some(5));
    assert_eq!(synced.policy().get("cat"), Permission::Ask);
    // switched off by hand: off with the next message
    std::fs::write(&file, "[permissions]\n").unwrap();
    type_text(&mut app, &tx, "hi");
    app.update(key(KeyCode::Enter), &tx);
    app.input.clear();
    assert!(!app.tools_on && app.harness.is_none());
    // the panel, reopened, shows the file and says it changed
    std::fs::write(
        &file,
        "max_steps = 3\n[permissions]\n\"read files\" = \"allow\"\n\"edit existing files\" = \"ask\"\n\"create new files\" = \"ask\"\n\"cat\" = \"allow\"\n",
    )
    .unwrap();
    type_text(&mut app, &tx, "/tools");
    app.update(key(KeyCode::Enter), &tx);
    let p = yours(&app);
    assert!(p.reads() && p.edits() && p.creates(), "{p:?}");
    assert_eq!(app.steps_of(DEFAULT_AGENT), 3);
    assert_eq!(p.get("cat"), Permission::Allow);
    app.update(key(KeyCode::Esc), &tx);
    let answer = last_answer(&app);
    assert!(answer.contains("default.toml changed"), "{answer}");
    // nothing stepped: what the hand wrote stays
    let again = load(&file);
    assert_eq!(again.max_steps, Some(3));
    assert_eq!(again.policy().get("cat"), Permission::Allow);
    // at startup the file is what counts, not the old keys
    let (app, _tx, _rx) = boot(ToolsConfig {
        enabled: false,
        ..Default::default()
    });
    assert!(app.tools_on);
    assert_eq!(app.tools_commands(), vec!["cat"]);
    assert_eq!(app.harness.as_ref().unwrap().limits().rounds, 3);
    assert_eq!(app.tools_scope(), (true, true));
    // a file that does not parse is said at startup, left alone, and
    // default runs with nothing on until it is fixed
    std::fs::write(&file, "[permissions]\n\"cat\" = \"maybe\"\n").unwrap();
    let (app, _tx, _rx) = boot(ToolsConfig {
        enabled: true,
        ..Default::default()
    });
    assert!(
        app.items
            .iter()
            .any(|i| matches!(i, Item::Info(s) if s.contains("default.toml"))),
        "{:?}",
        app.items
    );
    assert!(!app.tools_on);
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "[permissions]\n\"cat\" = \"maybe\"\n"
    );
    // gone: written again, this time from the old keys of the configuration
    std::fs::remove_file(&file).unwrap();
    let (app, _tx, _rx) = boot(ToolsConfig {
        enabled: true,
        edit: true,
        create: false,
        ..Default::default()
    });
    assert!(app.tools_on);
    assert_eq!(app.tools_scope(), (true, false));
    assert_eq!(load(&file).policy().get(EDIT_FILES), Permission::Ask);
}
