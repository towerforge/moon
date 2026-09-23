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
    let Command::Ask(e) = &cmds[0] else {
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
    assert!(matches!(cmds.last(), Some(Command::Ask(e)) if e.path == "src/new.rs"));
}
