//! `write_file`: a new file, or a whole file replaced. Replacing follows the
//! same rule as editing: read first, and the file must not have changed.

use serde::Deserialize;
use serde_json::{json, Value};

use super::{args, diff, PendingEdit, SeenFiles, Tool, ToolError};
use crate::sandbox::{Denied, Eol, Sandbox};

#[derive(Debug, Deserialize)]
pub struct Args {
    pub path: String,
    pub content: String,
}

pub fn spec() -> (&'static str, Value) {
    (
        "Create a file with this content, or replace a whole file that was read first. Prefer \
         edit_file for changes to an existing file. Folders on the way are created. The user \
         sees the diff and applies or skips it.",
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path relative to the project root"},
                "content": {"type": "string", "description": "The whole content of the file"}
            },
            "required": ["path", "content"]
        }),
    )
}

/// Checks the call and builds the edit; nothing is written here.
pub fn prepare(
    sandbox: &Sandbox,
    seen: &SeenFiles,
    arguments: &Value,
) -> Result<PendingEdit, ToolError> {
    let a: Args = args(Tool::WriteFile, arguments)?;
    let rel = sandbox.relative(&a.path)?;
    let content = a.content.replace("\r\n", "\n");
    let existing = match sandbox.read(&rel) {
        Ok(f) => Some(f),
        Err(Denied::NotFound(_)) => None,
        Err(e) => return Err(e.into()),
    };
    match existing {
        Some(cur) => {
            let Some(s) = seen.get(&rel) else {
                return Err(Denied::NotRead(rel).into());
            };
            if cur.hash != s.hash {
                return Err(Denied::Stale(rel).into());
            }
            if content == cur.text {
                return Err(ToolError::Usage(format!(
                    "write_file: `{rel}` already has exactly that content"
                )));
            }
            Ok(PendingEdit {
                tool: Tool::WriteFile,
                path: rel,
                diff: diff(&cur.text, &content),
                before: cur.text,
                after: content,
                eol: cur.eol,
                bom: cur.bom,
                expect: Some(cur.hash),
            })
        }
        None => Ok(PendingEdit {
            tool: Tool::WriteFile,
            path: rel,
            diff: diff("", &content),
            before: String::new(),
            after: content,
            eol: Eol::platform(),
            bom: false,
            expect: None,
        }),
    }
}
