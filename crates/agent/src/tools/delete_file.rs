//! `delete_file`: a file the model has read and that has not changed since,
//! or a folder with nothing in it. Never a folder with its contents at once:
//! each file goes on its own, and the user sees each one whole.

use serde::Deserialize;
use serde_json::{json, Value};

use super::{args, diff, Diff, PendingEdit, SeenFiles, Tool, ToolError};
use crate::sandbox::{Denied, Eol, Sandbox};

#[derive(Debug, Deserialize)]
pub struct Args {
    pub path: String,
}

pub fn spec() -> (&'static str, Value) {
    (
        "Delete a file read in this conversation, or an empty folder. Read the file first; to \
         remove a folder, delete what it holds, then the folder. The user sees what goes and \
         applies or skips it.",
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path relative to the project root"}
            },
            "required": ["path"]
        }),
    )
}

/// Checks the call and builds the delete, the file's text as the removed
/// side of the diff; nothing is deleted here.
pub fn prepare(
    sandbox: &Sandbox,
    seen: &SeenFiles,
    arguments: &Value,
) -> Result<PendingEdit, ToolError> {
    let a: Args = args(Tool::DeleteFile, arguments)?;
    let rel = sandbox.relative(&a.path)?;
    let loc = sandbox.resolve(&rel)?;
    if loc.exists && loc.full.is_dir() {
        sandbox.check_remove(&rel, None)?;
        return Ok(PendingEdit {
            tool: Tool::DeleteFile,
            path: loc.rel,
            to: None,
            before: String::new(),
            after: String::new(),
            eol: Eol::platform(),
            bom: false,
            expect: None,
            diff: Diff::default(),
        });
    }
    let cur = sandbox.read(&rel)?;
    let Some(s) = seen.get(&cur.rel) else {
        return Err(Denied::NotRead(cur.rel).into());
    };
    if cur.hash != s.hash {
        return Err(Denied::Stale(cur.rel).into());
    }
    sandbox.check_remove(&cur.rel, Some(cur.hash))?;
    Ok(PendingEdit {
        tool: Tool::DeleteFile,
        diff: diff(&cur.text, ""),
        path: cur.rel,
        to: None,
        before: cur.text,
        after: String::new(),
        eol: cur.eol,
        bom: cur.bom,
        expect: Some(cur.hash),
    })
}
