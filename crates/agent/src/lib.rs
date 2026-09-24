//! The model acting on the project, behind an explicit switch and with every
//! write approved on screen. Four folders, four ideas:
//!
//! - `harness/`: the loop. A pure state machine: feed it what the model said
//!   and what the user decided, it hands back what to do next. No I/O of its
//!   own beyond the tools it runs.
//! - `agents/`: who talks to the model. A prompt and a set of tools: the
//!   `editor`, and the `reader` that only looks.
//! - `tools/`: what an agent can do. A closed enum: `read_file`, `list_dir`,
//!   `edit_file`, `write_file` and `run_command`, which runs one of the
//!   commands of a fixed catalogue, the ones ticked in `/tools`, with no
//!   shell in between.
//! - `sandbox/`: the boundary. Every tool turns the model's string into a
//!   path through it, and the path stays under the start-up directory.
//!
//! The crate depends on `moon-core` only: no terminal, no HTTP.

pub mod agents;
pub mod harness;
pub mod sandbox;
pub mod tools;

pub use agents::{editor, editor_with, reader, Agent};
pub use harness::{calls_in_text, Command, Event, Harness, Limits, Outcome, Step, Stop, Verdict};
pub use sandbox::{Denied, Eol, Sandbox};
pub use tools::{
    catalog, run_command, Category, Diff, DiffKind, DiffLine, Entry, Exec, Kind, Output, Pending,
    PendingEdit, Policy, Tool, CATALOG,
};
