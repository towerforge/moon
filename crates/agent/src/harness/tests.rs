//! The loop with a scripted model and a scripted user: no provider, no
//! terminal.

use std::fs;

use moon_core::Role;
use serde_json::json;

use super::*;
use crate::agents::{editor, editor_with, reader};

fn harness(limits: Limits) -> (tempfile::TempDir, Harness) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/a.rs"), "one\ntwo\n").unwrap();
    let sb = Sandbox::new(dir.path(), 10_000, &[]).unwrap();
    (dir, Harness::new(editor(), sb, limits))
}

fn call(name: &str, args: serde_json::Value) -> ToolCall {
    ToolCall {
        id: Some(format!("id-{name}")),
        name: name.into(),
        arguments: args,
    }
}

fn read(path: &str) -> ToolCall {
    call("read_file", json!({"path": path}))
}

fn edit(path: &str, old: &str, new: &str) -> ToolCall {
    call(
        "edit_file",
        json!({"path": path, "old_string": old, "new_string": new}),
    )
}

/// The tool messages of a `Continue`.
fn continued(cmds: &[Command]) -> &[Message] {
    match cmds.last() {
        Some(Command::Continue(r)) => r,
        other => panic!("expected Continue, got {other:?}"),
    }
}

#[test]
fn a_clean_turn_read_edit_apply_finish() {
    let (dir, mut h) = harness(Limits::default());
    h.begin_turn();
    assert!(!h.in_turn());
    // round 1: the model reads; that runs on its own
    let cmds = h.feed(Event::ModelDone(vec![read("src/a.rs")]));
    assert!(
        matches!(&cmds[0], Command::Step(s) if s.tool == Tool::ReadFile && s.outcome == Outcome::Done)
    );
    let results = continued(&cmds);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].role, Role::Tool);
    assert_eq!(results[0].tool_call_id.as_deref(), Some("id-read_file"));
    assert!(results[0].content.contains("one\ntwo"));
    assert!(h.in_turn());
    // round 2: an edit waits for the user
    let cmds = h.feed(Event::ModelDone(vec![edit("src/a.rs", "two", "2")]));
    assert_eq!(cmds.len(), 1);
    let Command::Ask(Pending::Edit(e)) = &cmds[0] else {
        panic!("expected Ask, got {cmds:?}")
    };
    assert_eq!(e.path, "src/a.rs");
    assert_eq!(e.counts(), "+1 −1");
    assert!(h.waiting());
    assert_eq!(
        fs::read_to_string(dir.path().join("src/a.rs")).unwrap(),
        "one\ntwo\n"
    );
    // the user applies: the file changes and the model hears so
    let cmds = h.feed(Event::Verdict(Verdict::Apply));
    assert!(matches!(&cmds[0], Command::Step(s) if s.outcome == Outcome::Applied && s.added == 1));
    assert_eq!(
        fs::read_to_string(dir.path().join("src/a.rs")).unwrap(),
        "one\n2\n"
    );
    assert!(continued(&cmds)[0]
        .content
        .starts_with("applied: `src/a.rs`"));
    assert!(!h.waiting());
    // the same file edited again in the turn: no re-read needed, the harness
    // remembered the new hash
    let cmds = h.feed(Event::ModelDone(vec![edit("src/a.rs", "one", "1")]));
    assert!(matches!(&cmds[0], Command::Ask(_)));
    let cmds = h.feed(Event::Verdict(Verdict::Skip));
    assert!(matches!(&cmds[0], Command::Step(s) if s.outcome == Outcome::Skipped));
    assert!(continued(&cmds)[0].content.starts_with("skipped"));
    assert_eq!(
        fs::read_to_string(dir.path().join("src/a.rs")).unwrap(),
        "one\n2\n"
    );
    // a reply with no calls ends the turn
    let cmds = h.feed(Event::ModelDone(vec![]));
    assert_eq!(cmds, vec![Command::Finished]);
    assert!(!h.in_turn());
}

#[test]
fn several_calls_in_one_reply_pause_at_each_edit() {
    let (_d, mut h) = harness(Limits::default());
    let cmds = h.feed(Event::ModelDone(vec![
        read("src/a.rs"),
        edit("src/a.rs", "one", "1"),
        call("list_dir", json!({})),
    ]));
    // the read ran, the edit is on screen, the listing waits its turn
    assert!(matches!(&cmds[0], Command::Step(_)));
    assert!(matches!(&cmds[1], Command::Ask(_)));
    assert_eq!(cmds.len(), 2);
    let cmds = h.feed(Event::Verdict(Verdict::Apply));
    assert!(matches!(&cmds[0], Command::Step(s) if s.outcome == Outcome::Applied));
    assert!(matches!(&cmds[1], Command::Step(s) if s.tool == Tool::ListDir));
    let results = continued(&cmds);
    assert_eq!(results.len(), 3);
    assert_eq!(results[2].tool_name.as_deref(), Some("list_dir"));
    assert!(results[2].content.contains("src/"));
}

#[test]
fn errors_go_back_to_the_model_and_only_refusals_count() {
    let (_d, mut h) = harness(Limits {
        rejections: 2,
        ..Limits::default()
    });
    // a bad old_string is the model's mistake, not a refusal
    h.feed(Event::ModelDone(vec![read("src/a.rs")]));
    let cmds = h.feed(Event::ModelDone(vec![edit("src/a.rs", "nope", "x")]));
    assert!(matches!(&cmds[0], Command::Step(s) if matches!(s.outcome, Outcome::Failed(_))));
    assert!(continued(&cmds)[0].content.contains("old_string not found"));
    // an unknown tool is answered, not counted
    let cmds = h.feed(Event::ModelDone(vec![call(
        "bash",
        json!({"cmd": "rm -rf /"}),
    )]));
    assert!(continued(&cmds)[0].content.contains("unknown tool `bash`"));
    // two refusals stop the turn; what was queued behind is closed too
    let cmds = h.feed(Event::ModelDone(vec![read("../etc/passwd")]));
    assert!(continued(&cmds)[0].content.contains("outside the project"));
    let cmds = h.feed(Event::ModelDone(vec![
        read("/etc/passwd"),
        read("src/a.rs"),
    ]));
    let Some(Command::Stopped { reason, results }) = cmds.last() else {
        panic!("expected Stopped, got {cmds:?}")
    };
    assert_eq!(*reason, Stop::TooManyRejections);
    assert_eq!(results.len(), 2);
    assert!(results[1].content.contains("not run"));
    assert!(!h.in_turn());
}

#[test]
fn the_round_and_call_limits() {
    let (_d, mut h) = harness(Limits {
        rounds: 2,
        calls_per_round: 1,
        rejections: 3,
    });
    let cmds = h.feed(Event::ModelDone(vec![read("src/a.rs"), read("src/a.rs")]));
    let results = continued(&cmds);
    assert_eq!(results.len(), 2);
    assert!(results[0].content.contains("too many calls"));
    assert!(results[1].content.contains("one\ntwo"));
    h.feed(Event::ModelDone(vec![read("src/a.rs")]));
    let cmds = h.feed(Event::ModelDone(vec![read("src/a.rs")]));
    assert!(matches!(
        cmds.last(),
        Some(Command::Stopped { reason: Stop::TooManyRounds, results }) if results.len() == 1
    ));
    // a new turn starts the count over
    h.begin_turn();
    let cmds = h.feed(Event::ModelDone(vec![read("src/a.rs")]));
    assert!(matches!(cmds.last(), Some(Command::Continue(_))));
}

#[test]
fn cancel_closes_what_was_pending_and_writes_nothing() {
    let (dir, mut h) = harness(Limits::default());
    h.feed(Event::ModelDone(vec![read("src/a.rs")]));
    let cmds = h.feed(Event::ModelDone(vec![
        edit("src/a.rs", "one", "1"),
        edit("src/a.rs", "two", "2"),
    ]));
    assert!(matches!(&cmds[0], Command::Ask(_)));
    let cmds = h.feed(Event::Cancel);
    let Some(Command::Stopped { reason, results }) = cmds.first() else {
        panic!("expected Stopped, got {cmds:?}")
    };
    assert_eq!(*reason, Stop::Cancelled);
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.content.contains("cancelled")));
    assert_eq!(
        fs::read_to_string(dir.path().join("src/a.rs")).unwrap(),
        "one\ntwo\n"
    );
    assert!(!h.waiting());
    // a verdict with nothing pending is a no-op
    assert!(h.feed(Event::Verdict(Verdict::Apply)).is_empty());
}

#[test]
fn the_prompt_and_the_specs_come_from_the_agent() {
    let (_d, h) = harness(Limits::default());
    assert_eq!(h.specs().len(), 4);
    assert!(h.prompt().contains("no shell"));
    assert_eq!(h.agent().name, "editor");
    assert_eq!(h.limits(), Limits::default());
    assert!(h.pending().is_none());
    // re-tuned mid-conversation
    let (_d, mut h) = harness(Limits::default());
    h.set_agent(editor().without(Tool::WriteFile));
    assert_eq!(h.specs().len(), 3);
    h.set_limits(Limits {
        rounds: 2,
        ..Limits::default()
    });
    assert_eq!(h.limits().rounds, 2);
    let cmds = h.feed(Event::ModelDone(vec![call(
        "write_file",
        json!({"path": "x", "content": "y"}),
    )]));
    assert!(continued(&cmds)[0].content.contains("unknown tool"));
}

#[test]
fn the_reader_and_the_creator_without_the_editor() {
    let (_d, mut h) = harness(Limits::default());
    h.set_agent(reader());
    assert_eq!(h.specs().len(), 2);
    assert!(h.prompt().contains("cannot change files"));
    let cmds = h.feed(Event::ModelDone(vec![call(
        "edit_file",
        json!({"path": "src/a.rs", "old_string": "one", "new_string": "uno"}),
    )]));
    assert!(continued(&cmds)[0].content.contains("unknown tool"));
    // create without edit: a new file goes to the user, an existing one is
    // refused even after a read
    let (_d, mut h) = harness(Limits::default());
    h.set_agent(editor_with(false, true));
    let cmds = h.feed(Event::ModelDone(vec![call(
        "read_file",
        json!({"path": "src/a.rs"}),
    )]));
    assert!(matches!(cmds.last(), Some(Command::Continue(_))));
    let cmds = h.feed(Event::ModelDone(vec![call(
        "write_file",
        json!({"path": "src/a.rs", "content": "replaced\n"}),
    )]));
    let r = continued(&cmds);
    assert!(r[0].content.contains("may only create new files"), "{r:?}");
    let cmds = h.feed(Event::ModelDone(vec![call(
        "write_file",
        json!({"path": "src/new.rs", "content": "fresh\n"}),
    )]));
    assert!(matches!(cmds.last(), Some(Command::Ask(Pending::Edit(e))) if e.path == "src/new.rs"));
}

fn run(command: &str, args: &[&str]) -> ToolCall {
    call("run_command", json!({"command": command, "args": args}))
}

fn output(stdout: &str, code: i32) -> Output {
    Output {
        stdout: stdout.into(),
        code: Some(code),
        ..Output::default()
    }
}

#[test]
fn a_command_that_only_looks_runs_and_its_output_goes_back() {
    let (_d, mut h) = harness(Limits::default());
    h.set_agent(editor().with_commands(&["git status", "ls"]));
    assert!(h.specs().len() == 5 && h.prompt().contains("run_command"));
    // a read and a command in one reply: the read answers, the command
    // pauses the loop until the interface reports what it printed
    let cmds = h.feed(Event::ModelDone(vec![
        read("src/a.rs"),
        run("git status", &["--short"]),
        call("list_dir", json!({})),
    ]));
    assert!(matches!(&cmds[0], Command::Step(s) if s.tool == Tool::ReadFile));
    let Command::Run(exec) = &cmds[1] else {
        panic!("expected Run, got {cmds:?}")
    };
    assert_eq!(
        (exec.id, exec.line.as_str()),
        ("git status", "git status --short")
    );
    assert_eq!(exec.args, vec!["status", "--short"]);
    assert!(exec.program.is_absolute());
    assert_eq!(exec.dir, h.sandbox().root());
    assert_eq!(cmds.len(), 2);
    assert!(h.in_turn() && !h.waiting());
    assert_eq!(h.running(), Some(exec));
    // done: the step, the listing that waited its turn, and the results
    let cmds = h.feed(Event::Ran(output(" M src/a.rs\n", 0)));
    assert!(
        matches!(&cmds[0], Command::Step(s) if s.tool == Tool::RunCommand && s.path == "git status --short" && s.outcome == Outcome::Done)
    );
    assert!(matches!(&cmds[1], Command::Step(s) if s.tool == Tool::ListDir));
    let results = continued(&cmds);
    assert_eq!(results.len(), 3);
    assert_eq!(results[1].tool_name.as_deref(), Some("run_command"));
    assert_eq!(results[1].content, " M src/a.rs\n");
    assert!(h.running().is_none());
    // a failure is reported, not counted as a refusal
    let cmds = h.feed(Event::ModelDone(vec![run("ls", &["nope"])]));
    assert!(matches!(&cmds[0], Command::Run(_)));
    let mut failed = output("", 2);
    failed.stderr = "ls: nope: No such file".into();
    let cmds = h.feed(Event::Ran(failed));
    assert!(matches!(&cmds[0], Command::Step(s) if s.outcome == Outcome::Failed("exit 2".into())));
    assert!(continued(&cmds)[0].content.contains("exit code 2"));
    // a late result with nothing running is dropped
    assert!(h.feed(Event::Ran(output("x", 0))).is_empty());
}

#[test]
fn a_command_that_changes_things_asks_first() {
    let (_d, mut h) = harness(Limits::default());
    h.set_agent(editor().with_commands(&["git add", "git commit"]));
    let cmds = h.feed(Event::ModelDone(vec![
        run("git add", &["src/a.rs"]),
        run("git commit", &["-m", "fix: a"]),
    ]));
    let Command::Ask(Pending::Run(exec)) = &cmds[0] else {
        panic!("expected Ask, got {cmds:?}")
    };
    assert!(exec.asks);
    assert_eq!(exec.line, "git add src/a.rs");
    assert!(h.waiting());
    assert_eq!(cmds.len(), 1);
    // skipped: the model hears so and the next one comes up
    let cmds = h.feed(Event::Verdict(Verdict::Skip));
    assert!(
        matches!(&cmds[0], Command::Step(s) if s.outcome == Outcome::Skipped && s.path == "git add src/a.rs")
    );
    let Command::Ask(Pending::Run(exec)) = &cmds[1] else {
        panic!("expected Ask, got {cmds:?}")
    };
    assert_eq!(exec.line, "git commit -m \"fix: a\"");
    // approved: it runs, and only then the round closes
    let cmds = h.feed(Event::Verdict(Verdict::Apply));
    assert!(matches!(&cmds[0], Command::Run(e) if e.line == "git commit -m \"fix: a\""));
    assert_eq!(cmds.len(), 1);
    assert!(!h.waiting() && h.running().is_some());
    let cmds = h.feed(Event::Ran(output("[dev 1234] fix: a\n", 0)));
    assert!(matches!(&cmds[0], Command::Step(s) if s.outcome == Outcome::Done));
    let results = continued(&cmds);
    assert_eq!(results.len(), 2);
    assert!(results[0].content.starts_with("skipped by the user"));
    assert!(results[1].content.contains("fix: a"));
}

#[test]
fn a_refused_command_is_answered_and_cancel_closes_a_running_one() {
    let (_d, mut h) = harness(Limits::default());
    h.set_agent(editor().with_commands(&["ls"]));
    // not ticked: answered with the list, shown as it was asked
    let cmds = h.feed(Event::ModelDone(vec![run("git diff", &["--stat"])]));
    assert!(
        matches!(&cmds[0], Command::Step(s) if s.tool == Tool::RunCommand && s.path == "git diff --stat" && matches!(s.outcome, Outcome::Failed(_)))
    );
    let r = continued(&cmds);
    assert!(r[0].content.contains("not allowed"), "{r:?}");
    assert!(r[0].content.contains("ls"), "{r:?}");
    // a path outside is a refusal, like the file tools' own
    let cmds = h.feed(Event::ModelDone(vec![run("ls", &["/etc"])]));
    assert!(continued(&cmds)[0].content.contains("outside the project"));
    // without commands, the tool is unknown
    h.set_agent(editor());
    let cmds = h.feed(Event::ModelDone(vec![run("ls", &[])]));
    assert!(continued(&cmds)[0]
        .content
        .contains("unknown tool `run_command`"));
    // cancelled while one runs: closed as not run, nothing left over
    h.set_agent(editor().with_commands(&["ls"]));
    let cmds = h.feed(Event::ModelDone(vec![run("ls", &[]), read("src/a.rs")]));
    assert!(matches!(&cmds[0], Command::Run(_)));
    let cmds = h.feed(Event::Cancel);
    let Some(Command::Stopped { reason, results }) = cmds.first() else {
        panic!("expected Stopped, got {cmds:?}")
    };
    assert_eq!(*reason, Stop::Cancelled);
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.content.contains("cancelled")));
    assert!(!h.in_turn() && h.running().is_none());
    assert!(h.feed(Event::Ran(output("late", 0))).is_empty());
}

#[test]
fn an_edit_allowed_without_asking_is_written_as_it_comes() {
    use crate::agents::Agent;
    use moon_core::config::ids::EDIT_FILES;
    use moon_core::Permission;
    let (dir, mut h) = harness(Limits::default());
    let mut policy = editor().policy;
    policy.set(EDIT_FILES, Permission::Allow);
    h.set_agent(Agent::for_policy(policy));
    assert!(h.prompt().contains("written as you make them"));
    h.feed(Event::ModelDone(vec![read("src/a.rs")]));
    // no Ask: the step says applied, the file changed and the round closes
    let cmds = h.feed(Event::ModelDone(vec![edit("src/a.rs", "two", "2")]));
    assert!(matches!(&cmds[0], Command::Step(s) if s.outcome == Outcome::Applied && s.added == 1));
    assert!(continued(&cmds)[0]
        .content
        .starts_with("applied: `src/a.rs`"));
    assert!(!h.waiting());
    assert_eq!(
        fs::read_to_string(dir.path().join("src/a.rs")).unwrap(),
        "one\n2\n"
    );
    // creating still asks: its permission is its own
    let cmds = h.feed(Event::ModelDone(vec![call(
        "write_file",
        json!({"path": "src/new.rs", "content": "x\n"}),
    )]));
    assert!(matches!(cmds.last(), Some(Command::Ask(Pending::Edit(e))) if e.path == "src/new.rs"));
    h.feed(Event::Cancel);
    // and replacing a file whole through write_file goes by the permission
    // on editing
    let cmds = h.feed(Event::ModelDone(vec![call(
        "write_file",
        json!({"path": "src/a.rs", "content": "whole\n"}),
    )]));
    assert!(matches!(&cmds[0], Command::Step(s) if s.outcome == Outcome::Applied));
    assert_eq!(
        fs::read_to_string(dir.path().join("src/a.rs")).unwrap(),
        "whole\n"
    );
}
