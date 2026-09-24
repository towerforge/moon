//! The model running commands, from the interface: the commands in the
//! `/tools` panel, the permissions stepping within their bounds, a turn
//! that runs one and reads what it printed, one that waits for the ok, a
//! refusal on screen, `Esc` killing one that runs, and `tools.toml`. The
//! fake provider and the helpers are `tests_agent`'s.

use std::time::Duration;

use moon_agent::{Policy, CATALOG};
use moon_core::config::ids::{CREATE_FILES, EDIT_FILES, READ_FILES};
use moon_core::{Permission, ToolSpec, ToolsConfig, ToolsFile};
use serde_json::json;

use super::tests::{app, key, last_answer, type_text};
use super::tests_agent::{app_with_fake, call, done, project, settle, text};
use super::*;

/// The permissions a conversation starts with, for the fake provider.
fn allow(app: &mut App, pairs: &[(&str, Permission)]) {
    for (id, p) in pairs {
        app.cfg.tools.permissions.insert(id.to_string(), *p);
    }
    app.enable_tools().unwrap();
}

#[test]
fn the_dialog_walks_two_levels_and_steps_permissions_within_their_bounds() {
    use moon_agent::Category;
    let mut d = ToolsDialog::new(Policy::default(), 20, vec![true; CATALOG.len()]);
    // the groups alone on the first level, and the cursor wraps over them
    assert_eq!(d.level, Level::Groups);
    d.up();
    assert_eq!(d.category(), Some(Category::Network));
    d.down();
    assert_eq!(d.category(), Some(Category::Editor));
    // the step limit is the last row of `Editor`: the number stays in
    // range, and the cursor wraps over it too
    d.go_to_steps();
    assert!(d.on_steps() && d.entry().is_none());
    d.change(5);
    assert_eq!(d.max_steps, 20);
    d.change(-100);
    assert_eq!(d.max_steps, 1);
    d.toggle();
    assert_eq!(d.max_steps, 1);
    d.down();
    assert_eq!(d.entry().map(|e| e.id), Some(READ_FILES));
    d.up();
    assert!(d.on_steps());
    // the row before it is where commands run: off or allow, no ask
    d.up();
    assert_eq!(d.entry().map(|e| e.id), Some("commands in subfolders"));
    d.change(1);
    assert!(d.policy.subfolders());
    d.change(1);
    assert!(d.policy.subfolders());
    d.change(-1);
    assert!(!d.policy.subfolders());
    d.back();
    // enter opens a group; inside, a command walks off · ask · allow with
    // ←→ and stops at the ends
    d.enter();
    assert_eq!(d.level, Level::Group(Category::Editor));
    assert_eq!(d.entry().map(|e| e.id), Some(READ_FILES));
    d.go_to("git diff");
    assert_eq!(d.level, Level::Group(Category::Git));
    assert_eq!(d.entry().map(|e| e.id), Some("git diff"));
    d.change(1);
    assert_eq!(d.policy.get("git diff"), Permission::Ask);
    d.change(1);
    assert_eq!(d.policy.get("git diff"), Permission::Allow);
    d.change(1);
    assert_eq!(d.policy.get("git diff"), Permission::Allow);
    d.change(-1);
    assert_eq!(d.policy.get("git diff"), Permission::Ask);
    // enter is off and on, on being what the catalogue turns it to
    d.enter();
    assert_eq!(d.policy.get("git diff"), Permission::Off);
    d.enter();
    assert_eq!(d.policy.get("git diff"), Permission::Allow);
    d.go_to("git commit");
    d.toggle();
    assert_eq!(d.policy.get("git commit"), Permission::Ask);
    // editing may go all the way to allow, the user's call, and brings
    // reading with it
    d.go_to(EDIT_FILES);
    d.change(1);
    assert_eq!(d.policy.get(EDIT_FILES), Permission::Ask);
    d.change(1);
    assert_eq!(d.policy.get(EDIT_FILES), Permission::Allow);
    assert_eq!(d.policy.get(READ_FILES), Permission::Allow);
    // reading off takes editing with it, and the commands stay
    d.go_to(READ_FILES);
    d.toggle();
    assert!(!d.policy.reads() && !d.policy.edits());
    assert_eq!(d.policy.get("git diff"), Permission::Allow);
    // esc goes back to the groups, on the one left; from the groups there
    // is nowhere back to
    assert!(d.back());
    assert_eq!(
        (d.level, d.category()),
        (Level::Groups, Some(Category::Editor))
    );
    assert!(!d.back());
    // a group as a whole: → its defaults, ← all off, space between the
    // two; the summary says what is on
    d.row = 2;
    assert_eq!(d.category(), Some(Category::Git));
    d.change(1);
    assert_eq!(d.policy.get("git status"), Permission::Allow);
    assert_eq!(d.policy.get("git commit"), Permission::Ask);
    assert_eq!(
        d.summary(Category::Git),
        "allow: git status, git diff, git log, git show, git blame · ask: git add, git commit"
    );
    d.change(-1);
    assert!(!d.group_on(Category::Git));
    assert_eq!(d.summary(Category::Git), "off");
    d.toggle();
    assert!(d.group_on(Category::Git));
    d.toggle();
    assert!(!d.group_on(Category::Git));
    // `Editor` as a whole: its defaults, then off takes reading, and so
    // editing, with it
    d.row = 0;
    d.change(1);
    assert!(d.policy.reads() && d.policy.edits() && d.policy.creates());
    assert_eq!(d.policy.get(CREATE_FILES), Permission::Ask);
    d.change(-1);
    assert!(d.policy.is_empty());
    // `Editor` as a whole also lets commands run in subfolders, and off
    // takes that with it; `Files` as a whole brings mkdir at ask
    d.row = 0;
    d.change(1);
    assert!(d.policy.subfolders());
    d.change(-1);
    assert!(!d.policy.subfolders());
    d.row = 1;
    d.change(1);
    assert!(!d.policy.subfolders());
    assert_eq!(d.policy.get("mkdir"), Permission::Ask);
    assert_eq!(d.policy.get("ls"), Permission::Allow);
    d.change(-1);
    assert!(d.policy.is_empty());
    // a program that is not installed stays off whatever the key, and a
    // group's defaults leave it out
    let mut missing = ToolsDialog::new(Policy::default(), 8, vec![false; CATALOG.len()]);
    missing.go_to("ls");
    missing.toggle();
    missing.change(1);
    assert_eq!(missing.policy.get("ls"), Permission::Off);
    assert!(!missing.found_at(missing.index().unwrap()));
    missing.back();
    missing.change(1);
    assert!(missing.policy.is_empty());
    // the scroll follows the cursor and never goes past the end
    d.show(10, 30, 8);
    assert_eq!(d.scroll, 3);
    d.show(2, 30, 8);
    assert_eq!(d.scroll, 2);
    d.show(29, 30, 8);
    assert_eq!(d.scroll, 22);
    d.show(0, 5, 8);
    assert_eq!(d.scroll, 0);
}

#[cfg(unix)]
#[tokio::test]
async fn commands_are_set_in_the_panel_like_anything_else() {
    use moon_agent::Category;
    let dir = project();
    let (mut app, tx, _rx) = app();
    app.root = dir.path().to_path_buf();
    type_text(&mut app, &tx, "/tools");
    app.update(key(KeyCode::Enter), &tx);
    // into `Files`, on `ls`: the machine has it, space turns it on
    let Some(Panel::Tools(d)) = app.panel.as_mut() else {
        panic!("expected the tools panel")
    };
    d.go_to("ls");
    assert!(d.found_at(d.index().unwrap()));
    app.update(key(KeyCode::Char(' ')), &tx);
    let Some(Panel::Tools(d)) = app.panel.as_mut() else {
        panic!("expected the tools panel")
    };
    assert_eq!(d.policy.get("ls"), Permission::Allow);
    // a command does not bring reading with it: it is a choice of its own
    assert!(!d.policy.reads());
    // `git add` asks by default; the user may let it run on its own
    d.go_to("git add");
    app.update(key(KeyCode::Enter), &tx);
    app.update(key(KeyCode::Right), &tx);
    let Some(Panel::Tools(d)) = &app.panel else {
        panic!("expected the tools panel")
    };
    assert_eq!(d.policy.get("git add"), Permission::Allow);
    // esc steps out onto the group left and applies on its way: on, and
    // the panel still open; esc again closes
    app.update(key(KeyCode::Esc), &tx);
    assert!(
        matches!(&app.panel, Some(Panel::Tools(d)) if d.level == Level::Groups && d.category() == Some(Category::Git))
    );
    assert!(app.tools_on);
    assert_eq!(app.tools_commands(), vec!["ls", "git add"]);
    app.update(key(KeyCode::Esc), &tx);
    assert!(app.panel.is_none());
    assert!(app.tools_on);
    // the agent has the tool and nothing else, the marker counts, and the
    // answer lists them
    assert_eq!(app.tools_commands(), vec!["ls", "git add"]);
    let h = app.harness.as_ref().unwrap();
    assert!(h.agent().has(moon_agent::Tool::RunCommand));
    assert!(!h.agent().has(moon_agent::Tool::ReadFile));
    assert_eq!(h.agent().name, "reader");
    assert_eq!(app.edit_mode_span().unwrap().content, "⏵⏵ 2 commands");
    let answer = last_answer(&app);
    assert!(answer.contains("allow: ls, git add"), "{answer}");
    assert!(!answer.contains("ask:"), "{answer}");
    let prompt = app.system_prompt_for(&[]).unwrap();
    assert!(prompt.contains("run_command: ls, git add"), "{prompt}");
    assert!(
        !prompt.contains("no shell") && !prompt.contains("wait for the user"),
        "{prompt}"
    );
    // reopened, the panel shows what is set; both off, out and closed is
    // tools off
    type_text(&mut app, &tx, "/tools");
    app.update(key(KeyCode::Enter), &tx);
    let Some(Panel::Tools(d)) = app.panel.as_mut() else {
        panic!("expected the tools panel")
    };
    assert_eq!(d.policy.get("git add"), Permission::Allow);
    d.go_to("ls");
    app.update(key(KeyCode::Enter), &tx);
    let Some(Panel::Tools(d)) = app.panel.as_mut() else {
        panic!("expected the tools panel")
    };
    d.go_to("git add");
    app.update(key(KeyCode::Enter), &tx);
    app.update(key(KeyCode::Esc), &tx);
    app.update(key(KeyCode::Esc), &tx);
    assert!(app.panel.is_none());
    assert!(!app.tools_on);
    assert!(app.tools_commands().is_empty());
    assert!(app.edit_mode_span().is_none());
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
    assert_eq!(app.edit_mode_span().unwrap().content, "⏵⏵ Read · 1 command");
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
    assert_eq!(a.title(), "Run git add");
    assert_eq!(a.verb(), "Run");
    assert_eq!(a.info(), "");
    assert_eq!(a.exec().unwrap().line, "git add a.rs");
    assert!(a.edit().is_none());
    let activity = text(&app.activity_spans().unwrap());
    assert!(activity.contains("waiting for your approval") && activity.contains("enter run"));
    assert!(app.panel_keys().contains(&("r", "run")));
    // skipped: nothing ran, the model hears so, the turn goes on
    app.update(key(KeyCode::Char('s')), &tx);
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
fn the_panel_keeps_what_is_set_in_a_file_and_reads_it_back() {
    let dir = project();
    let file = dir.path().join("conf").join(ToolsFile::FILE);
    let boot = |tools: ToolsConfig| {
        let (tx, rx) = mpsc::unbounded_channel();
        let app = App::new(RunOptions {
            config: Config {
                tools,
                ..Default::default()
            },
            config_source: ConfigSource::Default(dir.path().join("conf").join("config.toml")),
            registry: Arc::new(Registry::new()),
            store: None,
            resume: None,
            model: None,
            version: "0.0.0".into(),
            cwd: "~/p".into(),
            root: dir.path().to_path_buf(),
            state_dir: None,
            tools_file: Some(file.clone()),
        });
        (app, tx, rx)
    };
    let perms = |pairs: &[(&str, Permission)]| -> std::collections::BTreeMap<String, Permission> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    };
    // no file yet: the configuration says, and starting writes nothing
    let (mut app, tx, _rx) = boot(ToolsConfig::default());
    assert!(!app.tools_on && !file.exists() && app.tools_seen.is_none());
    // closing the panel writes it, with what is set: reading, editing, `ls`
    type_text(&mut app, &tx, "/tools");
    app.update(key(KeyCode::Enter), &tx);
    app.update(key(KeyCode::Enter), &tx);
    app.update(key(KeyCode::Char(' ')), &tx);
    app.update(key(KeyCode::Down), &tx);
    app.update(key(KeyCode::Char(' ')), &tx);
    let Some(Panel::Tools(d)) = app.panel.as_mut() else {
        panic!("expected the tools panel")
    };
    d.go_to("ls");
    app.update(key(KeyCode::Char(' ')), &tx);
    app.update(key(KeyCode::Esc), &tx);
    app.update(key(KeyCode::Esc), &tx);
    let saved = ToolsFile::load(&file)
        .unwrap()
        .expect("the file was written");
    assert_eq!(saved.max_steps, 8);
    // the whole catalogue is in it, what is off said off
    let text = std::fs::read_to_string(&file).unwrap();
    for e in CATALOG {
        assert!(text.contains(&format!("\"{}\"", e.id)), "{}", e.id);
    }
    assert!(text.contains("# Network — "), "{text}");
    let curl = text.lines().find(|l| l.starts_with("\"curl\"")).unwrap();
    assert!(curl.contains("= \"off\""), "{curl}");
    assert_eq!(
        saved.permissions,
        perms(&[
            (READ_FILES, Permission::Allow),
            (EDIT_FILES, Permission::Ask),
            ("ls", Permission::Allow),
        ])
    );
    assert_eq!(app.tools_seen, Some(saved));
    // edited by hand: the next message goes out with what it says, even
    // one that cannot be sent
    ToolsFile {
        max_steps: 5,
        permissions: perms(&[
            (READ_FILES, Permission::Allow),
            (CREATE_FILES, Permission::Ask),
            ("git diff", Permission::Allow),
            ("cat", Permission::Ask),
        ]),
    }
    .save(&file)
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
        "⏵⏵ Read · Create · 2 commands"
    );
    // switched off by hand: off with the next message
    ToolsFile::default().save(&file).unwrap();
    type_text(&mut app, &tx, "hi");
    app.update(key(KeyCode::Enter), &tx);
    app.input.clear();
    assert!(!app.tools_on && app.harness.is_none());
    // the panel, reopened, shows the file and says it changed; closing it
    // writes the same back
    ToolsFile {
        max_steps: 3,
        permissions: perms(&[
            (READ_FILES, Permission::Allow),
            (EDIT_FILES, Permission::Ask),
            (CREATE_FILES, Permission::Ask),
            ("cat", Permission::Allow),
        ]),
    }
    .save(&file)
    .unwrap();
    type_text(&mut app, &tx, "/tools");
    app.update(key(KeyCode::Enter), &tx);
    let Some(Panel::Tools(d)) = &app.panel else {
        panic!("expected the tools panel")
    };
    assert!(
        d.policy.reads() && d.policy.edits() && d.policy.creates(),
        "{d:?}"
    );
    assert_eq!(d.max_steps, 3);
    assert_eq!(d.policy.get("cat"), Permission::Allow);
    app.update(key(KeyCode::Esc), &tx);
    let answer = last_answer(&app);
    assert!(answer.contains("tools.toml changed"), "{answer}");
    let again = ToolsFile::load(&file).unwrap().unwrap();
    assert_eq!(again.max_steps, 3);
    assert_eq!(again.permissions.get("cat"), Some(&Permission::Allow));
    // at startup the file wins over the configuration
    let (app, _tx, _rx) = boot(ToolsConfig {
        enabled: false,
        ..Default::default()
    });
    assert!(app.tools_on);
    assert_eq!(app.tools_commands(), vec!["cat"]);
    assert_eq!(app.harness.as_ref().unwrap().limits().rounds, 3);
    assert_eq!(app.tools_scope(), (true, true));
    // a file that does not parse is said at startup, and the configuration
    // applies until it is fixed
    std::fs::write(&file, "[permissions]\n\"cat\" = \"maybe\"\n").unwrap();
    let (app, _tx, _rx) = boot(ToolsConfig {
        enabled: true,
        ..Default::default()
    });
    assert!(app
        .items
        .iter()
        .any(|i| matches!(i, Item::Error(e) if e.contains("tools.toml"))));
    assert!(app.tools_on && app.tools_seen.is_none());
}
