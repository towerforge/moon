//! The loop, as a state machine with no I/O of its own beyond the file
//! tools it runs. The interface feeds it what the model said, what the user
//! decided and what a command printed; it hands back what to do next: send
//! the tool results and ask the model again, show an edit or a command and
//! wait, run a command, put a line in the conversation, or stop. Sending,
//! running, drawing and persisting stay in the interface, so the whole loop
//! is tested here with a scripted model and a scripted user.
//!
//! A *turn* runs from the user's message to the model's last reply. Inside
//! it, a *round* is one reply with calls, the calls run, and the request
//! that follows. The limits stop a small model that loops.

use std::collections::VecDeque;
use std::fmt;

use moon_core::config::ids::{CREATE_FILES, EDIT_FILES};
use moon_core::{Message, Permission, ToolCall, ToolSpec};

use crate::agents::Agent;
use crate::sandbox::Sandbox;
use crate::tools::{
    self, path_of, Exec, Output, Pending, PendingEdit, Seen, SeenFiles, Tool, ToolError,
};

mod fallback;
#[cfg(test)]
mod tests;

pub use fallback::calls_in_text;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Replies with calls in one turn.
    pub rounds: usize,
    /// Calls honoured from one reply; the rest are answered "ask again".
    pub calls_per_round: usize,
    /// Sandbox refusals in one turn before the turn stops.
    pub rejections: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            rounds: 8,
            calls_per_round: 10,
            rejections: 3,
        }
    }
}

/// What the interface tells the harness.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The model's reply finished; these are the calls it made, if any.
    ModelDone(Vec<ToolCall>),
    /// The user decided about the edit or the command the harness asked about.
    Verdict(Verdict),
    /// The command the harness asked to run has finished.
    Ran(Output),
    /// `Esc`: the turn is over, nothing pending is written.
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Apply,
    Skip,
}

/// What the harness tells the interface to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Add these tool results to the conversation and ask the model again.
    Continue(Vec<Message>),
    /// Show this edit or command and wait for a verdict.
    Ask(Pending),
    /// Run this command, off the main thread, and feed back what it printed.
    Run(Exec),
    /// A line for the conversation: what a tool did.
    Step(Step),
    /// The last reply had no calls: the turn is over.
    Finished,
    /// The turn stopped early. The results close the calls that were left
    /// so the history stays well formed; they are not to be answered.
    Stopped { reason: Stop, results: Vec<Message> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    Cancelled,
    TooManyRounds,
    TooManyRejections,
}

impl fmt::Display for Stop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Stop::Cancelled => "cancelled",
            Stop::TooManyRounds => "too many steps for one message · /tools to raise the limit",
            Stop::TooManyRejections => "too many paths refused by the sandbox",
        })
    }
}

/// One tool call, resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub tool: Tool,
    /// The file, or the command line for a `run`.
    pub path: String,
    pub added: usize,
    pub removed: usize,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// A read, a listing or a command that went through.
    Done,
    Applied,
    Skipped,
    Failed(String),
}

/// What a call turned into, once its tool ran or prepared it.
enum Next {
    /// Answered already: the result is in.
    Done,
    /// Shown to the user first.
    Ask(Pending),
    /// Written right away: the user allowed it without asking.
    Apply(PendingEdit),
    /// Run right away.
    Run(Exec),
}

pub struct Harness {
    agent: Agent,
    sandbox: Sandbox,
    limits: Limits,
    /// Files read in this conversation, with the hash they had.
    seen: SeenFiles,
    round: usize,
    rejections: usize,
    /// Calls of the current reply not yet run.
    queue: VecDeque<ToolCall>,
    /// The edit or command shown to the user, with the call it answers.
    pending: Option<(ToolCall, Pending)>,
    /// The command the interface is running, with the call it answers.
    running: Option<(ToolCall, Exec)>,
    /// Tool results of the current round, sent together.
    results: Vec<Message>,
}

impl Harness {
    pub fn new(agent: Agent, sandbox: Sandbox, limits: Limits) -> Self {
        Self {
            agent,
            sandbox,
            limits,
            seen: SeenFiles::new(),
            round: 0,
            rejections: 0,
            queue: VecDeque::new(),
            pending: None,
            running: None,
            results: Vec::new(),
        }
    }

    pub fn agent(&self) -> &Agent {
        &self.agent
    }

    pub fn sandbox(&self) -> &Sandbox {
        &self.sandbox
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// Another tool set or prompt, mid-conversation. What was read stays known.
    pub fn set_agent(&mut self, agent: Agent) {
        self.agent = agent;
    }

    pub fn set_limits(&mut self, limits: Limits) {
        self.limits = limits;
    }

    /// What goes in every request while the agent is on.
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.agent.specs()
    }

    /// What is appended to the system prompt while the agent is on.
    pub fn prompt(&self) -> String {
        self.agent.system_prompt()
    }

    /// A reply with no `tool_calls` that is a call written as text, as the
    /// small models do. Empty if it is a reply.
    pub fn calls_from_text(&self, text: &str) -> Vec<ToolCall> {
        calls_in_text(text, &self.agent)
    }

    /// An edit or a command is on screen, waiting.
    pub fn waiting(&self) -> bool {
        self.pending.is_some()
    }

    /// What is on screen, if anything.
    pub fn pending(&self) -> Option<&Pending> {
        self.pending.as_ref().map(|(_, p)| p)
    }

    /// The command being run, if any.
    pub fn running(&self) -> Option<&Exec> {
        self.running.as_ref().map(|(_, e)| e)
    }

    /// Between the user's message and the model's last reply.
    pub fn in_turn(&self) -> bool {
        self.round > 0 || self.pending.is_some() || self.running.is_some() || !self.queue.is_empty()
    }

    /// A new user message: the counters start over. What was read stays
    /// known, the hash check guards it.
    pub fn begin_turn(&mut self) {
        self.reset();
    }

    fn reset(&mut self) {
        self.round = 0;
        self.rejections = 0;
        self.queue.clear();
        self.pending = None;
        self.running = None;
        self.results.clear();
    }

    pub fn feed(&mut self, ev: Event) -> Vec<Command> {
        let mut out = Vec::new();
        match ev {
            Event::ModelDone(calls) => {
                if calls.is_empty() {
                    self.reset();
                    out.push(Command::Finished);
                    return out;
                }
                self.round += 1;
                if self.round > self.limits.rounds {
                    let results = calls
                        .iter()
                        .map(|c| result(c, "not run: too many rounds of tool calls in this turn"))
                        .collect();
                    out.push(Command::Stopped {
                        reason: Stop::TooManyRounds,
                        results,
                    });
                    self.reset();
                    return out;
                }
                let keep = calls.len().min(self.limits.calls_per_round);
                for c in &calls[keep..] {
                    self.results.push(result(
                        c,
                        "not run: too many calls in one reply; ask for the rest in the next one",
                    ));
                }
                self.queue.extend(calls.into_iter().take(keep));
                self.drain(&mut out);
            }
            Event::Verdict(v) => {
                let Some((call, pending)) = self.pending.take() else {
                    return out;
                };
                match pending {
                    Pending::Edit(edit) => self.settle_edit(call, edit, v, &mut out),
                    Pending::Run(exec) => match v {
                        // approved: it runs, and the loop pauses until it is done
                        Verdict::Apply => {
                            out.push(Command::Run(exec.clone()));
                            self.running = Some((call, exec));
                            return out;
                        }
                        Verdict::Skip => {
                            self.results.push(result(
                                &call,
                                "skipped by the user: it did not run; do not retry it unless asked",
                            ));
                            out.push(Command::Step(run_step(&exec, Outcome::Skipped)));
                        }
                    },
                }
                self.drain(&mut out);
            }
            Event::Ran(output) => {
                // a late result of a run that was cancelled: nothing to answer
                let Some((call, exec)) = self.running.take() else {
                    return out;
                };
                self.results.push(result(&call, output.report()));
                out.push(Command::Step(run_step(&exec, output.outcome())));
                self.drain(&mut out);
            }
            Event::Cancel => {
                let mut results = std::mem::take(&mut self.results);
                let left = self
                    .pending
                    .take()
                    .map(|(c, _)| c)
                    .into_iter()
                    .chain(self.running.take().map(|(c, _)| c))
                    .chain(self.queue.drain(..));
                for c in left {
                    results.push(result(&c, "not run: the user cancelled the turn"));
                }
                out.push(Command::Stopped {
                    reason: Stop::Cancelled,
                    results,
                });
                self.reset();
            }
        }
        out
    }

    /// The user's word on an edit: written, or left alone, and the model
    /// told either way.
    fn settle_edit(
        &mut self,
        call: ToolCall,
        edit: PendingEdit,
        v: Verdict,
        out: &mut Vec<Command>,
    ) {
        let (text, outcome) = match v {
            Verdict::Apply => match edit.apply(&self.sandbox) {
                Ok(hash) => {
                    self.seen.insert(
                        edit.path.clone(),
                        Seen {
                            hash,
                            eol: edit.eol,
                            bom: edit.bom,
                        },
                    );
                    (
                        format!("applied: `{}` ({})", edit.path, edit.counts()),
                        Outcome::Applied,
                    )
                }
                Err(e) => (format!("not applied: {e}"), Outcome::Failed(e.to_string())),
            },
            Verdict::Skip => (
                "skipped by the user; do not retry it unless asked".to_string(),
                Outcome::Skipped,
            ),
        };
        self.results.push(result(&call, text));
        out.push(Command::Step(Step {
            tool: edit.tool,
            path: edit.path.clone(),
            added: edit.diff.added,
            removed: edit.diff.removed,
            outcome,
        }));
    }

    /// Runs the queued calls until one needs the user or a command has to
    /// run, or the queue is empty.
    fn drain(&mut self, out: &mut Vec<Command>) {
        while let Some(call) = self.queue.pop_front() {
            let path = path_of(&self.sandbox, &call.arguments);
            let tool = match Tool::from_name(&call.name) {
                Some(t) if self.agent.has(t) => t,
                _ => {
                    self.results.push(result(
                        &call,
                        format!(
                            "unknown tool `{}`; the tools are: {}",
                            call.name,
                            self.tool_names()
                        ),
                    ));
                    continue;
                }
            };
            let ran: Result<Next, ToolError> = match tool {
                Tool::ReadFile => {
                    tools::read_file::run(&self.sandbox, &mut self.seen, &call.arguments).map(
                        |text| {
                            self.results.push(result(&call, text));
                            Next::Done
                        },
                    )
                }
                Tool::ListDir => tools::list_dir::run(&self.sandbox, &call.arguments).map(|text| {
                    self.results.push(result(&call, text));
                    Next::Done
                }),
                Tool::EditFile => {
                    tools::edit_file::prepare(&self.sandbox, &self.seen, &call.arguments)
                        .map(|e| self.settle_or_ask(e))
                }
                Tool::WriteFile => {
                    tools::write_file::prepare(&self.sandbox, &self.seen, &call.arguments).and_then(
                        |edit| {
                            // with the edit box off, `create` means create: no
                            // replacing a file whole through the back door
                            if edit.expect.is_some() && !self.agent.has(Tool::EditFile) {
                                return Err(ToolError::Usage(format!(
                                    "write_file: `{}` exists and editing existing files is off in \
                             this conversation; write_file may only create new files here",
                                    edit.path
                                )));
                            }
                            Ok(self.settle_or_ask(edit))
                        },
                    )
                }
                Tool::RunCommand => {
                    tools::run_command::prepare(&self.sandbox, &self.agent.policy, &call.arguments)
                        .map(|exec| {
                            if exec.asks {
                                Next::Ask(Pending::Run(exec))
                            } else {
                                Next::Run(exec)
                            }
                        })
                }
            };
            match ran {
                Ok(Next::Apply(edit)) => self.settle_edit(call, edit, Verdict::Apply, out),
                Ok(Next::Ask(pending)) => {
                    out.push(Command::Ask(pending.clone()));
                    self.pending = Some((call, pending));
                    return;
                }
                Ok(Next::Run(exec)) => {
                    out.push(Command::Run(exec.clone()));
                    self.running = Some((call, exec));
                    return;
                }
                Ok(Next::Done) => out.push(Command::Step(Step {
                    tool,
                    path,
                    added: 0,
                    removed: 0,
                    outcome: Outcome::Done,
                })),
                Err(e) => {
                    if matches!(e, ToolError::Denied(_)) {
                        self.rejections += 1;
                    }
                    self.results.push(result(&call, format!("error: {e}")));
                    // a command that did not run shows what was asked for
                    let path = if tool == Tool::RunCommand {
                        command_of(&call.arguments)
                    } else {
                        path
                    };
                    out.push(Command::Step(Step {
                        tool,
                        path,
                        added: 0,
                        removed: 0,
                        outcome: Outcome::Failed(e.to_string()),
                    }));
                    if self.rejections >= self.limits.rejections {
                        let mut results = std::mem::take(&mut self.results);
                        for c in self.queue.drain(..) {
                            results.push(result(&c, "not run: the turn stopped"));
                        }
                        out.push(Command::Stopped {
                            reason: Stop::TooManyRejections,
                            results,
                        });
                        self.reset();
                        return;
                    }
                }
            }
        }
        out.push(Command::Continue(std::mem::take(&mut self.results)));
    }

    /// A write the user chose to see first, or to let through: replacing a
    /// file that exists goes by the permission on editing, a new one by the
    /// permission on creating.
    fn settle_or_ask(&self, edit: PendingEdit) -> Next {
        let id = if edit.expect.is_some() {
            EDIT_FILES
        } else {
            CREATE_FILES
        };
        if self.agent.policy.get(id) == Permission::Allow {
            Next::Apply(edit)
        } else {
            Next::Ask(Pending::Edit(edit))
        }
    }

    fn tool_names(&self) -> String {
        self.agent
            .tools()
            .iter()
            .map(|t| t.name())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// The step line of a command: its line, and how it went.
fn run_step(exec: &Exec, outcome: Outcome) -> Step {
    Step {
        tool: Tool::RunCommand,
        path: exec.line.clone(),
        added: 0,
        removed: 0,
        outcome,
    }
}

/// The `command` argument of a call, with its `args`, for the step line of
/// one that was refused.
fn command_of(arguments: &serde_json::Value) -> String {
    let mut line = arguments
        .get("command")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("?")
        .trim()
        .to_string();
    if let Some(args) = arguments.get("args").and_then(serde_json::Value::as_array) {
        for a in args.iter().filter_map(serde_json::Value::as_str) {
            line.push(' ');
            line.push_str(a);
        }
    }
    line
}

/// The tool message that answers a call.
fn result(call: &ToolCall, content: impl Into<String>) -> Message {
    Message::tool(call.name.clone(), call.id.clone(), content)
}
