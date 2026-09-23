//! The loop, as a state machine with no I/O of its own beyond the tools it
//! runs. The interface feeds it what the model said and what the user
//! decided; it hands back what to do next: send the tool results and ask the
//! model again, show an edit and wait, put a line in the conversation, or
//! stop. Sending, drawing and persisting stay in the interface, so the whole
//! loop is tested here with a scripted model and a scripted user.
//!
//! A *turn* runs from the user's message to the model's last reply. Inside
//! it, a *round* is one reply with calls, the calls run, and the request
//! that follows. The limits stop a small model that loops.

use std::collections::VecDeque;
use std::fmt;

use moon_core::{Message, ToolCall, ToolSpec};

use crate::agents::Agent;
use crate::sandbox::Sandbox;
use crate::tools::{self, path_of, PendingEdit, Seen, SeenFiles, Tool, ToolError};

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
    /// The user decided about the edit the harness asked about.
    Verdict(Verdict),
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
    /// Show this edit and wait for a verdict.
    Ask(PendingEdit),
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
    pub path: String,
    pub added: usize,
    pub removed: usize,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// A read or a listing that went through.
    Done,
    Applied,
    Skipped,
    Failed(String),
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
    /// The edit shown to the user, with the call it answers.
    pending: Option<(ToolCall, PendingEdit)>,
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
    pub fn prompt(&self) -> &'static str {
        self.agent.prompt
    }

    /// A reply with no `tool_calls` that is a call written as text, as the
    /// small models do. Empty if it is a reply.
    pub fn calls_from_text(&self, text: &str) -> Vec<ToolCall> {
        calls_in_text(text, &self.agent)
    }

    /// An edit is on screen, waiting.
    pub fn waiting(&self) -> bool {
        self.pending.is_some()
    }

    /// The edit on screen, if any.
    pub fn pending(&self) -> Option<&PendingEdit> {
        self.pending.as_ref().map(|(_, e)| e)
    }

    /// Between the user's message and the model's last reply.
    pub fn in_turn(&self) -> bool {
        self.round > 0 || self.pending.is_some() || !self.queue.is_empty()
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
                let Some((call, edit)) = self.pending.take() else {
                    return out;
                };
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
                self.drain(&mut out);
            }
            Event::Cancel => {
                let mut results = std::mem::take(&mut self.results);
                let left = self
                    .pending
                    .take()
                    .map(|(c, _)| c)
                    .into_iter()
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

    /// Runs the queued calls until one needs the user or the queue is empty.
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
            let ran: Result<Option<PendingEdit>, ToolError> = match tool {
                Tool::ReadFile => {
                    tools::read_file::run(&self.sandbox, &mut self.seen, &call.arguments).map(
                        |text| {
                            self.results.push(result(&call, text));
                            None
                        },
                    )
                }
                Tool::ListDir => tools::list_dir::run(&self.sandbox, &call.arguments).map(|text| {
                    self.results.push(result(&call, text));
                    None
                }),
                Tool::EditFile => {
                    tools::edit_file::prepare(&self.sandbox, &self.seen, &call.arguments).map(Some)
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
                            Ok(Some(edit))
                        },
                    )
                }
            };
            match ran {
                Ok(Some(edit)) => {
                    out.push(Command::Ask(edit.clone()));
                    self.pending = Some((call, edit));
                    return;
                }
                Ok(None) => out.push(Command::Step(Step {
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

    fn tool_names(&self) -> String {
        self.agent
            .tools
            .iter()
            .map(|t| t.name())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// The tool message that answers a call.
fn result(call: &ToolCall, content: impl Into<String>) -> Message {
    Message::tool(call.name.clone(), call.id.clone(), content)
}
