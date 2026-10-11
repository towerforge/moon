//! What an agent can do. A closed enum, and no shell. `run_command` runs one
//! of the commands of a fixed catalogue, the ones that the agent's file
//! turns on, as a program with its arguments, never through a shell. Every
//! tool goes through the sandbox for its paths; the four that change files
//! (edit, write, move, delete) only *prepare* the change, and the harness
//! applies it once the user says so; a command set to `ask` waits the same
//! way.

use std::collections::HashMap;

use moon_core::ToolSpec;
use serde_json::Value;

use crate::sandbox::{Denied, Eol, Sandbox};

pub mod catalog;
pub mod delete_file;
mod diff;
pub mod edit_file;
pub mod list_dir;
pub mod move_file;
pub mod read_file;
pub mod run_command;
#[cfg(test)]
mod tests;
pub mod write_file;

pub use catalog::{Category, Entry, Kind, Policy, Section, CATALOG};
pub use diff::{diff, Diff, DiffKind, DiffLine};
pub use run_command::{Exec, Output};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
    ReadFile,
    ListDir,
    EditFile,
    WriteFile,
    MoveFile,
    DeleteFile,
    RunCommand,
}

impl Tool {
    pub const ALL: [Tool; 7] = [
        Tool::ReadFile,
        Tool::ListDir,
        Tool::EditFile,
        Tool::WriteFile,
        Tool::MoveFile,
        Tool::DeleteFile,
        Tool::RunCommand,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Tool::ReadFile => "read_file",
            Tool::ListDir => "list_dir",
            Tool::EditFile => "edit_file",
            Tool::WriteFile => "write_file",
            Tool::MoveFile => "move_file",
            Tool::DeleteFile => "delete_file",
            Tool::RunCommand => "run_command",
        }
    }

    pub fn from_name(name: &str) -> Option<Tool> {
        Tool::ALL.into_iter().find(|t| t.name() == name)
    }

    /// Whether it changes the disk, and so waits for the user.
    pub fn writes(self) -> bool {
        matches!(
            self,
            Tool::EditFile | Tool::WriteFile | Tool::MoveFile | Tool::DeleteFile
        )
    }

    /// The verb for the step line: `read`, `list`, `edit`, `write`, `move`,
    /// `delete`, `run`.
    pub fn verb(self) -> &'static str {
        match self {
            Tool::ReadFile => "read",
            Tool::ListDir => "list",
            Tool::EditFile => "edit",
            Tool::WriteFile => "write",
            Tool::MoveFile => "move",
            Tool::DeleteFile => "delete",
            Tool::RunCommand => "run",
        }
    }

    /// What the model is told about the tool: its JSON schema. For
    /// `run_command` the agent fills in the commands it allows.
    pub fn spec(self) -> ToolSpec {
        let (description, parameters) = match self {
            Tool::ReadFile => read_file::spec(),
            Tool::ListDir => list_dir::spec(),
            Tool::EditFile => edit_file::spec(),
            Tool::WriteFile => write_file::spec(),
            Tool::MoveFile => move_file::spec(),
            Tool::DeleteFile => delete_file::spec(),
            Tool::RunCommand => {
                let (d, p) = run_command::spec(&Policy::default());
                return ToolSpec {
                    name: self.name().to_string(),
                    description: d,
                    parameters: p,
                };
            }
        };
        ToolSpec {
            name: self.name().to_string(),
            description: description.to_string(),
            parameters,
        }
    }
}

/// What waits for the user: an edit with its diff, or a command that
/// changes things.
#[derive(Debug, Clone, PartialEq)]
pub enum Pending {
    Edit(PendingEdit),
    Run(Exec),
}

/// Why a call did not run. The model reads the text either way; only the
/// sandbox's refusals count against the turn's limit.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ToolError {
    #[error("{0}")]
    Denied(#[from] Denied),
    /// Bad arguments, an `old_string` that is not there: the model's to fix.
    #[error("{0}")]
    Usage(String),
}

/// What a read left behind, keyed by the relative path: an edit must match
/// the hash, and writes back in the same convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seen {
    pub hash: u64,
    pub eol: Eol,
    pub bom: bool,
}

pub type SeenFiles = HashMap<String, Seen>;

/// A change the model asked for, checked and diffed, waiting for the user:
/// an edit, a new file, a move (`to` set, no diff) or a delete (the file's
/// text all removed).
#[derive(Debug, Clone, PartialEq)]
pub struct PendingEdit {
    pub tool: Tool,
    /// Relative, `/`-separated; for a move, where it comes from.
    pub path: String,
    /// Where a move goes.
    pub to: Option<String>,
    pub before: String,
    pub after: String,
    pub eol: Eol,
    pub bom: bool,
    /// Hash the file must still carry when applied; `None` for a new file,
    /// a move and an empty folder.
    pub expect: Option<u64>,
    pub diff: Diff,
}

impl PendingEdit {
    /// Writes it. Returns the file's new hash for the harness to remember.
    /// Only for an edit or a new file; a move and a delete go by `carry_out`.
    pub fn apply(&self, sandbox: &Sandbox) -> Result<u64, Denied> {
        sandbox.write(&self.path, &self.after, self.eol, self.bom, self.expect)
    }

    /// Does it, whatever it is: the new hash of a file written, `None` for a
    /// move or a delete, which leave no file to remember at this path. The
    /// checks run again first: the disk may have changed while the user
    /// looked.
    pub fn carry_out(&self, sandbox: &Sandbox) -> Result<Option<u64>, Denied> {
        match self.tool {
            Tool::MoveFile => {
                let to = self.to.as_deref().unwrap_or_default();
                sandbox.move_path(&self.path, to, true).map(|()| None)
            }
            Tool::DeleteFile => sandbox.remove(&self.path, self.expect).map(|()| None),
            _ => self.apply(sandbox).map(Some),
        }
    }

    /// What it acts on, as the step line and the panel show it: the path, or
    /// `from → to` for a move.
    pub fn subject(&self) -> String {
        match &self.to {
            Some(to) => format!("{} → {to}", self.path),
            None => self.path.clone(),
        }
    }

    /// `+3 −1`, for the step line and the panel title.
    pub fn counts(&self) -> String {
        format!("+{} −{}", self.diff.added, self.diff.removed)
    }
}

/// Arguments parsed into their struct, with a message that names the tool
/// when they are not what the schema said.
pub(crate) fn args<T: serde::de::DeserializeOwned>(tool: Tool, v: &Value) -> Result<T, ToolError> {
    serde_json::from_value(v.clone())
        .map_err(|e| ToolError::Usage(format!("{}: bad arguments: {e}", tool.name())))
}

/// The `path` argument of a call, for the step line even when the call
/// failed; `from → to` for a move.
pub fn path_of(sandbox: &Sandbox, arguments: &Value) -> String {
    let rel = |key: &str| {
        arguments
            .get(key)
            .and_then(Value::as_str)
            .map(|p| sandbox.relative(p).unwrap_or_else(|_| p.to_string()))
    };
    match (rel("path"), rel("from"), rel("to")) {
        (Some(p), _, _) => p,
        (None, Some(from), Some(to)) => format!("{from} → {to}"),
        _ => "?".to_string(),
    }
}
