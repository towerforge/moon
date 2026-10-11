//! `move_file`: a file or a folder to another path inside the project, the
//! way a rename or a refactor needs it. Nothing is lost, so nothing has to
//! be read first; it never lands on something that exists.

use serde::Deserialize;
use serde_json::{json, Value};

use super::{args, Diff, PendingEdit, Tool, ToolError};
use crate::sandbox::{Eol, Sandbox};

#[derive(Debug, Deserialize)]
pub struct Args {
    pub from: String,
    pub to: String,
}

pub fn spec() -> (&'static str, Value) {
    (
        "Move or rename a file or a folder inside the project. It never replaces something \
         that exists at `to`. The user sees the move and applies or skips it. Update what \
         refers to the old path with edit_file afterwards.",
        json!({
            "type": "object",
            "properties": {
                "from": {"type": "string", "description": "Path to move, relative to the project root"},
                "to": {"type": "string", "description": "Where it goes, relative to the project root"}
            },
            "required": ["from", "to"]
        }),
    )
}

/// Checks the call and builds the move; nothing is moved here. New folders
/// on the way are made only when creating files is on too.
pub fn prepare(
    sandbox: &Sandbox,
    make_dirs: bool,
    arguments: &Value,
) -> Result<PendingEdit, ToolError> {
    let a: Args = args(Tool::MoveFile, arguments)?;
    let (from, to) = sandbox
        .check_move(&a.from, &a.to, make_dirs)
        .map_err(|e| match e {
            crate::sandbox::Denied::NotFound(ref p) if !make_dirs && *p != a.from => {
                ToolError::Usage(format!(
                    "move_file: the folder `{p}` does not exist, and creating files is off; \
                     move into a folder that exists"
                ))
            }
            e => e.into(),
        })?;
    Ok(PendingEdit {
        tool: Tool::MoveFile,
        path: from.rel,
        to: Some(to.rel),
        before: String::new(),
        after: String::new(),
        eol: Eol::platform(),
        bom: false,
        expect: None,
        diff: Diff::default(),
    })
}
