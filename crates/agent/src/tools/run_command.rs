//! `run_command`: one of the commands that are on in `/tools`, and only
//! those. `prepare` turns the call into an `Exec`, checked: the command is
//! on, the arguments stay inside the project and off the deny list, the
//! folder is one of the project's. `execute` runs it, with no shell, a time
//! limit and a cap on the output. The harness decides between the two: an
//! `Exec` that `asks` is shown to the user first, the interface runs it off
//! the main thread, and the `Output` comes back as an event.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use moon_core::Permission;
use serde::Deserialize;
use serde_json::{json, Value};

use super::catalog::{self, Policy};
use super::{args, Tool, ToolError};
use crate::harness::Outcome;
use crate::sandbox::Sandbox;

/// A command that runs longer than this is killed.
pub const TIMEOUT: Duration = Duration::from_secs(60);
/// Bytes of output the model gets, from the start; the rest is cut and said.
pub const OUTPUT_MAX: usize = 20_000;

#[derive(Debug, Deserialize)]
pub struct Args {
    /// The command as listed, `git diff`, with any arguments after it.
    pub command: String,
    /// Arguments, one per item, on top of what `command` carries.
    #[serde(default)]
    pub args: Vec<String>,
    /// Folder of the project to run it in; the root if omitted.
    #[serde(default)]
    pub dir: Option<String>,
}

/// The description the model sees, with the commands it may call.
pub fn spec(policy: &Policy) -> (String, Value) {
    let commands = policy.commands();
    let names: Vec<&str> = commands.iter().map(|e| e.id).collect();
    let asks: Vec<&str> = commands
        .iter()
        .filter(|e| policy.get(e.id) == Permission::Ask)
        .map(|e| e.id)
        .collect();
    let mut description = if names.is_empty() {
        "Run a command of the project. No command is allowed right now.".to_string()
    } else {
        format!(
            "Run one of these commands, and only these: {}. No shell: no pipes, no \
             redirections, no `&&`. Arguments are relative to the project root and stay \
             inside it.",
            names.join(", ")
        )
    };
    if !asks.is_empty() {
        description.push_str(&format!(
            " {} wait for the user's ok before running.",
            asks.join(", ")
        ));
    }
    let dir_help = if policy.subfolders() {
        "Folder of the project to run it in; the root if omitted"
    } else {
        "Not available: commands run from the project root"
    };
    (
        description,
        json!({
            "type": "object",
            "properties": {
                "command": {"type": "string", "description": "The command as listed, e.g. \"git diff\" or \"ls\", with arguments after it if you like"},
                "args": {"type": "array", "items": {"type": "string"}, "description": "Arguments, one per item, e.g. [\"--stat\", \"src\"]"},
                "dir": {"type": "string", "description": dir_help}
            },
            "required": ["command"]
        }),
    )
}

/// A command checked and ready to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exec {
    /// The catalogue id: `git diff`.
    pub id: &'static str,
    /// The executable, resolved.
    pub program: PathBuf,
    /// Everything after the program, the id's own words first.
    pub args: Vec<String>,
    /// Where it runs, absolute.
    pub dir: PathBuf,
    /// The same, as shown: `.` or `src/app`.
    pub rel_dir: String,
    /// Waits for the user before running: its permission is `ask`.
    pub asks: bool,
    pub help: &'static str,
    /// `git diff --stat src`, for the step line and the panel.
    pub line: String,
}

pub fn prepare(sandbox: &Sandbox, policy: &Policy, arguments: &Value) -> Result<Exec, ToolError> {
    let a: Args = args(Tool::RunCommand, arguments)?;
    let usage = |m: String| ToolError::Usage(format!("run_command: {m}"));
    let mut tokens: Vec<String> = a.command.split_whitespace().map(String::from).collect();
    tokens.extend(a.args.iter().map(|s| s.trim().to_string()));
    tokens.retain(|t| !t.is_empty());
    if tokens.is_empty() {
        return Err(usage("empty command".into()));
    }
    let list = || {
        let names: Vec<&str> = policy.commands().iter().map(|e| e.id).collect();
        if names.is_empty() {
            "none".to_string()
        } else {
            names.join(", ")
        }
    };
    let Some((entry, n)) = catalog::match_call(policy, &tokens) else {
        let head = tokens.iter().take(2).cloned().collect::<Vec<_>>().join(" ");
        return Err(usage(format!(
            "`{head}` is not allowed; the commands you may run are: {}",
            list()
        )));
    };
    let rest = &tokens[n..];
    for arg in rest {
        catalog::check_arg(entry, arg).map_err(&usage)?;
        // the paths the file tools refuse are refused here too, by name
        sandbox.deny_check(arg)?;
    }
    let (dir, rel_dir) = match a.dir.as_deref().map(str::trim) {
        Some(d) if !d.is_empty() && d != "." => {
            if !policy.subfolders() {
                return Err(usage(
                    "commands run from the project root here; `commands in subfolders` is off \
                     in /tools"
                        .into(),
                ));
            }
            let loc = sandbox.directory(d)?;
            (loc.full, loc.rel)
        }
        _ => (sandbox.root().to_path_buf(), ".".to_string()),
    };
    let Some(program) = entry.resolve() else {
        return Err(usage(format!(
            "`{}` is not installed on this machine",
            entry.program()
        )));
    };
    let mut argv = entry.prefix();
    argv.extend(rest.iter().cloned());
    let mut line = entry.id.to_string();
    for r in rest {
        line.push(' ');
        line.push_str(&quote(r));
    }
    Ok(Exec {
        id: entry.id,
        program,
        args: argv,
        dir,
        rel_dir,
        asks: policy.get(entry.id) == Permission::Ask,
        help: entry.help,
        line,
    })
}

/// An argument as it would be typed: quoted when it has a space.
fn quote(s: &str) -> String {
    if s.is_empty() || s.chars().any(char::is_whitespace) {
        format!("\"{}\"", s.replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

/// What a command left behind.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
    /// Exit code; `None` when killed, or when it never started.
    pub code: Option<i32>,
    pub timed_out: bool,
    pub cancelled: bool,
    /// It did not start: the error.
    pub failed: Option<String>,
    pub elapsed: Duration,
}

impl Output {
    /// For the step line.
    pub fn outcome(&self) -> Outcome {
        if let Some(e) = &self.failed {
            return Outcome::Failed(format!("could not start: {e}"));
        }
        if self.cancelled {
            return Outcome::Failed("cancelled".into());
        }
        if self.timed_out {
            return Outcome::Failed(format!("timed out after {}s", TIMEOUT.as_secs()));
        }
        match self.code {
            Some(0) => Outcome::Done,
            Some(n) => Outcome::Failed(format!("exit {n}")),
            None => Outcome::Failed("killed".into()),
        }
    }

    /// What the model reads back: the exit, both streams, cut at
    /// `OUTPUT_MAX` with a note.
    pub fn report(&self) -> String {
        if let Some(e) = &self.failed {
            return format!("could not start: {e}");
        }
        let mut out = String::new();
        if self.cancelled {
            out.push_str("cancelled by the user\n");
        } else if self.timed_out {
            out.push_str(&format!(
                "timed out after {}s and was killed; the output so far:\n",
                TIMEOUT.as_secs()
            ));
        } else {
            match self.code {
                Some(0) => {}
                Some(n) => out.push_str(&format!("exit code {n}\n")),
                None => out.push_str("killed by a signal\n"),
            }
        }
        let stdout = cut(&self.stdout);
        let stderr = cut(&self.stderr);
        match (stdout.trim().is_empty(), stderr.trim().is_empty()) {
            (true, true) => out.push_str("(no output)"),
            (false, true) => out.push_str(&stdout),
            (true, false) => out.push_str(&format!("stderr:\n{stderr}")),
            (false, false) => out.push_str(&format!("{stdout}\n--- stderr ---\n{stderr}")),
        }
        out
    }
}

fn cut(s: &str) -> String {
    if s.len() <= OUTPUT_MAX {
        return s.to_string();
    }
    let mut end = OUTPUT_MAX;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n… cut here: {} more bytes", &s[..end], s.len() - end)
}

/// Runs it and waits: no shell, stdin closed, both streams captured, killed
/// at `TIMEOUT` or when `cancel` is raised. Blocking: the interface calls
/// it off its own thread.
pub fn execute(exec: &Exec, cancel: &AtomicBool) -> Output {
    let started = Instant::now();
    let mut cmd = std::process::Command::new(&exec.program);
    cmd.args(&exec.args)
        .current_dir(&exec.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // nothing interactive, nothing coloured, no pager
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EDITOR", "true")
        .env("GIT_PAGER", "cat")
        .env("PAGER", "cat")
        .env("NO_COLOR", "1")
        .env("CLICOLOR", "0")
        .env("TERM", "dumb");
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return Output {
                failed: Some(e.to_string()),
                elapsed: started.elapsed(),
                ..Output::default()
            }
        }
    };
    let t_out = drain(child.stdout.take());
    let t_err = drain(child.stderr.take());
    let mut timed_out = false;
    let mut cancelled = false;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {}
            Err(_) => break,
        }
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            cancelled = true;
            break;
        }
        if started.elapsed() > TIMEOUT {
            let _ = child.kill();
            timed_out = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    let code = child.wait().ok().and_then(|s| s.code());
    let stdout = t_out.join().unwrap_or_default();
    let stderr = t_err.join().unwrap_or_default();
    Output {
        stdout,
        stderr,
        code,
        timed_out,
        cancelled,
        failed: None,
        elapsed: started.elapsed(),
    }
}

/// Reads a stream to its end on a thread of its own, so a command that
/// fills one pipe while the other is read does not block.
fn drain<R: std::io::Read + Send + 'static>(pipe: Option<R>) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut p) = pipe {
            let _ = p.read_to_end(&mut buf);
        }
        String::from_utf8_lossy(&buf).into_owned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use moon_core::config::ids::SUBFOLDERS;
    use std::fs;

    fn tree() -> (tempfile::TempDir, Sandbox) {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/a.rs"), "hi\n").unwrap();
        fs::write(dir.path().join(".env"), "SECRET=1\n").unwrap();
        let sb = Sandbox::new(dir.path(), 10_000, &[]).unwrap();
        (dir, sb)
    }

    fn policy(pairs: &[(&str, Permission)]) -> Policy {
        Policy::from_pairs(pairs.iter().copied())
    }

    #[test]
    fn the_spec_lists_what_is_on() {
        let (d, _) = spec(&Policy::default());
        assert!(d.contains("No command is allowed"));
        let p = policy(&[
            ("git diff", Permission::Allow),
            ("git commit", Permission::Ask),
            ("ls", Permission::Allow),
            (SUBFOLDERS, Permission::Allow),
        ]);
        let (d, params) = spec(&p);
        assert!(d.contains("ls, git diff, git commit"), "{d}");
        assert!(!d.contains("subfolders"), "{d}");
        assert!(d.contains("git commit wait for the user's ok"), "{d}");
        assert_eq!(params["required"][0], "command");
        assert!(params["properties"]["dir"]["description"]
            .as_str()
            .unwrap()
            .starts_with("Folder"));
        let (_, params) = spec(&policy(&[("ls", Permission::Allow)]));
        assert!(params["properties"]["dir"]["description"]
            .as_str()
            .unwrap()
            .starts_with("Not available"));
    }

    #[test]
    fn prepare_checks_the_command_the_arguments_and_the_folder() {
        let (_d, sb) = tree();
        let p = policy(&[
            ("git diff", Permission::Allow),
            ("ls", Permission::Allow),
            ("cat", Permission::Ask),
        ]);
        let prep = |v: Value| prepare(&sb, &p, &v);
        // the id may come whole in `command` or split into `args`
        #[cfg(unix)]
        {
            let e = prep(json!({"command": "ls -la src"})).unwrap();
            assert_eq!((e.id, e.rel_dir.as_str()), ("ls", "."));
            assert_eq!(e.args, vec!["-la", "src"]);
            assert_eq!(e.line, "ls -la src");
            assert_eq!(e.dir, sb.root());
            assert!(!e.asks);
            let e2 = prep(json!({"command": "ls", "args": ["-la", "src"]})).unwrap();
            assert_eq!(e2.args, e.args);
            // a space in an argument is quoted on the line
            let e = prep(json!({"command": "ls", "args": ["a b"]})).unwrap();
            assert_eq!(e.line, "ls \"a b\"");
            // `asks` is the permission, not the catalogue's default
            let e = prep(json!({"command": "cat src/a.rs"})).unwrap();
            assert!(e.asks);
        }
        // not on, or not in the catalogue
        let err = prep(json!({"command": "git status"})).unwrap_err();
        assert!(err.to_string().contains("not allowed"), "{err}");
        assert!(err.to_string().contains("ls, cat, git diff"), "{err}");
        let err = prep(json!({"command": "rm -rf ."})).unwrap_err();
        assert!(err.to_string().contains("`rm -rf` is not allowed"), "{err}");
        assert!(prep(json!({"command": "  "})).is_err());
        assert!(prep(json!({"nope": 1})).is_err());
        // arguments that leave the project, or name a secret
        let err = prep(json!({"command": "cat", "args": ["/etc/passwd"]})).unwrap_err();
        assert!(err.to_string().contains("outside the project"), "{err}");
        let err = prep(json!({"command": "cat ../x"})).unwrap_err();
        assert!(err.to_string().contains("outside the project"), "{err}");
        let err = prep(json!({"command": "cat .env"})).unwrap_err();
        assert!(err.to_string().contains("looks like a secret"), "{err}");
        assert!(matches!(err, ToolError::Denied(_)));
        // a folder needs `commands in subfolders` on, and must exist inside
        let err = prep(json!({"command": "ls", "dir": "src"})).unwrap_err();
        assert!(err.to_string().contains("subfolders` is off"), "{err}");
        let with_sub = policy(&[("ls", Permission::Allow), (SUBFOLDERS, Permission::Allow)]);
        #[cfg(unix)]
        {
            let e = prepare(&sb, &with_sub, &json!({"command": "ls", "dir": "src"})).unwrap();
            assert_eq!(e.rel_dir, "src");
            assert_eq!(e.dir, sb.root().join("src"));
            let e = prepare(&sb, &with_sub, &json!({"command": "ls", "dir": "."})).unwrap();
            assert_eq!(e.rel_dir, ".");
        }
        let err = prepare(&sb, &with_sub, &json!({"command": "ls", "dir": "nope"})).unwrap_err();
        assert!(err.to_string().contains("not found"), "{err}");
        let err = prepare(&sb, &with_sub, &json!({"command": "ls", "dir": "../"})).unwrap_err();
        assert!(err.to_string().contains("outside"), "{err}");
        let err =
            prepare(&sb, &with_sub, &json!({"command": "ls", "dir": "src/a.rs"})).unwrap_err();
        assert!(err.to_string().contains("not a directory"), "{err}");
    }

    #[test]
    fn the_report_and_the_outcome() {
        let ok = Output {
            stdout: "a\nb\n".into(),
            code: Some(0),
            ..Output::default()
        };
        assert_eq!(ok.outcome(), Outcome::Done);
        assert_eq!(ok.report(), "a\nb\n");
        let quiet = Output {
            code: Some(0),
            ..Output::default()
        };
        assert_eq!(quiet.report(), "(no output)");
        let failed = Output {
            stderr: "boom".into(),
            code: Some(2),
            ..Output::default()
        };
        assert_eq!(failed.outcome(), Outcome::Failed("exit 2".into()));
        assert_eq!(failed.report(), "exit code 2\nstderr:\nboom");
        let both = Output {
            stdout: "out".into(),
            stderr: "err".into(),
            code: Some(1),
            ..Output::default()
        };
        assert_eq!(both.report(), "exit code 1\nout\n--- stderr ---\nerr");
        let slow = Output {
            stdout: "x".into(),
            timed_out: true,
            ..Output::default()
        };
        assert!(matches!(slow.outcome(), Outcome::Failed(m) if m.starts_with("timed out")));
        assert!(slow.report().starts_with("timed out after 60s"));
        let stopped = Output {
            cancelled: true,
            ..Output::default()
        };
        assert_eq!(stopped.outcome(), Outcome::Failed("cancelled".into()));
        let never = Output {
            failed: Some("no such file".into()),
            ..Output::default()
        };
        assert!(never.report().starts_with("could not start"));
        // long output is cut, on a character boundary, and says so
        let long = Output {
            stdout: "é".repeat(OUTPUT_MAX),
            code: Some(0),
            ..Output::default()
        };
        let r = long.report();
        assert!(r.contains("… cut here:"), "{}", &r[r.len() - 40..]);
        assert!(r.len() < OUTPUT_MAX + 60);
    }

    #[test]
    fn execute_runs_without_a_shell_and_captures_both_streams() {
        let (_d, sb) = tree();
        let (program, args) = if cfg!(windows) {
            (
                std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into()),
                vec!["/d", "/c", "echo hi"],
            )
        } else {
            (
                "/bin/sh".to_string(),
                vec!["-c", "echo hi; echo oops >&2; exit 3"],
            )
        };
        let exec = Exec {
            id: "test",
            program: PathBuf::from(program),
            args: args.into_iter().map(String::from).collect(),
            dir: sb.root().to_path_buf(),
            rel_dir: ".".into(),
            asks: false,
            help: "",
            line: "test".into(),
        };
        let out = execute(&exec, &AtomicBool::new(false));
        assert!(out.failed.is_none(), "{out:?}");
        assert!(out.stdout.trim_end().ends_with("hi"), "{out:?}");
        if cfg!(unix) {
            assert_eq!(out.code, Some(3));
            assert_eq!(out.stderr.trim(), "oops");
        }
        // a program that is not there does not start
        let missing = Exec {
            program: PathBuf::from("/no/such/program"),
            ..exec.clone()
        };
        let out = execute(&missing, &AtomicBool::new(false));
        assert!(out.failed.is_some());
        assert!(out.report().starts_with("could not start"));
        // raised before it starts, the cancel kills it right away
        #[cfg(unix)]
        {
            let sleepy = Exec {
                args: vec!["-c".into(), "sleep 30".into()],
                ..exec
            };
            let cancel = AtomicBool::new(true);
            let out = execute(&sleepy, &cancel);
            assert!(out.cancelled, "{out:?}");
            assert!(out.elapsed < Duration::from_secs(5));
        }
    }
}
