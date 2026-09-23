//! What an agent can do. A closed enum: there is no `bash` variant, no
//! delete, no network, so no configuration can offer the model any of them.
//! Every tool goes through the sandbox for its paths; the two that write
//! only *prepare* an edit, and the harness applies it once the user says so.

use std::collections::HashMap;

use moon_core::ToolSpec;
use serde_json::Value;

use crate::sandbox::{Denied, Eol, Sandbox};

mod diff;
pub mod edit_file;
pub mod list_dir;
pub mod read_file;
#[cfg(test)]
mod tests;
pub mod write_file;

pub use diff::{diff, Diff, DiffKind, DiffLine};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
    ReadFile,
    ListDir,
    EditFile,
    WriteFile,
}

impl Tool {
    pub const ALL: [Tool; 4] = [
        Tool::ReadFile,
        Tool::ListDir,
        Tool::EditFile,
        Tool::WriteFile,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Tool::ReadFile => "read_file",
            Tool::ListDir => "list_dir",
            Tool::EditFile => "edit_file",
            Tool::WriteFile => "write_file",
        }
    }

    pub fn from_name(name: &str) -> Option<Tool> {
        Tool::ALL.into_iter().find(|t| t.name() == name)
    }

    /// Whether it changes the disk, and so waits for the user.
    pub fn writes(self) -> bool {
        matches!(self, Tool::EditFile | Tool::WriteFile)
    }

    /// The verb for the step line: `read`, `list`, `edit`, `write`.
    pub fn verb(self) -> &'static str {
        match self {
            Tool::ReadFile => "read",
            Tool::ListDir => "list",
            Tool::EditFile => "edit",
            Tool::WriteFile => "write",
        }
    }

    /// What the model is told about the tool: its JSON schema.
    pub fn spec(self) -> ToolSpec {
        let (description, parameters) = match self {
            Tool::ReadFile => read_file::spec(),
            Tool::ListDir => list_dir::spec(),
            Tool::EditFile => edit_file::spec(),
            Tool::WriteFile => write_file::spec(),
        };
        ToolSpec {
            name: self.name().to_string(),
            description: description.to_string(),
            parameters,
        }
    }
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

/// A write the model asked for, checked and diffed, waiting for the user.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingEdit {
    pub tool: Tool,
    /// Relative, `/`-separated.
    pub path: String,
    pub before: String,
    pub after: String,
    pub eol: Eol,
    pub bom: bool,
    /// Hash the file must still carry when applied; `None` for a new file.
    pub expect: Option<u64>,
    pub diff: Diff,
}

impl PendingEdit {
    /// Writes it. Returns the file's new hash for the harness to remember.
    pub fn apply(&self, sandbox: &Sandbox) -> Result<u64, Denied> {
        sandbox.write(&self.path, &self.after, self.eol, self.bom, self.expect)
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

/// The `path` argument of a call, for the step line even when the call failed.
pub fn path_of(sandbox: &Sandbox, arguments: &Value) -> String {
    arguments
        .get("path")
        .and_then(Value::as_str)
        .map(|p| sandbox.relative(p).unwrap_or_else(|_| p.to_string()))
        .unwrap_or_else(|| "?".to_string())
}
