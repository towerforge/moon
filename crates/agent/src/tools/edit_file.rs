//! `edit_file`: replace one exact passage with another. The shape most agents
//! use, because the small local models have seen it: `old_string` must occur
//! exactly once, or `replace_all` says every occurrence.

use serde::Deserialize;
use serde_json::{json, Value};

use super::{args, diff, PendingEdit, SeenFiles, Tool, ToolError};
use crate::sandbox::{Denied, Sandbox};

#[derive(Debug, Deserialize)]
pub struct Args {
    pub path: String,
    pub old_string: String,
    pub new_string: String,
    #[serde(default)]
    pub replace_all: bool,
}

pub fn spec() -> (&'static str, Value) {
    (
        "Replace an exact passage of a file with another. Read the file first. old_string must \
         match the file exactly, including indentation and line breaks, and occur once; set \
         replace_all to change every occurrence. The user sees the diff and applies or skips it.",
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path relative to the project root"},
                "old_string": {"type": "string", "description": "The exact text to replace"},
                "new_string": {"type": "string", "description": "What to put in its place"},
                "replace_all": {"type": "boolean", "description": "Replace every occurrence (default false)"}
            },
            "required": ["path", "old_string", "new_string"]
        }),
    )
}

/// Checks the call and builds the edit; nothing is written here.
pub fn prepare(
    sandbox: &Sandbox,
    seen: &SeenFiles,
    arguments: &Value,
) -> Result<PendingEdit, ToolError> {
    let a: Args = args(Tool::EditFile, arguments)?;
    let rel = sandbox.relative(&a.path)?;
    let Some(s) = seen.get(&rel) else {
        return Err(Denied::NotRead(rel).into());
    };
    let cur = sandbox.read(&rel)?;
    if cur.hash != s.hash {
        return Err(Denied::Stale(rel).into());
    }
    let old = a.old_string.replace("\r\n", "\n");
    let new = a.new_string.replace("\r\n", "\n");
    // an empty old_string on an empty file is what a model means by "put
    // this in it": the whole content, as write_file would
    if old.is_empty() && cur.text.trim().is_empty() {
        if new.trim().is_empty() {
            return Err(ToolError::Usage(format!(
                "edit_file: `{rel}` is empty and so is new_string; nothing to do"
            )));
        }
        return Ok(PendingEdit {
            tool: Tool::EditFile,
            path: rel,
            diff: diff(&cur.text, &new),
            before: cur.text,
            after: new,
            eol: cur.eol,
            bom: cur.bom,
            expect: Some(cur.hash),
        });
    }
    if old.is_empty() {
        return Err(ToolError::Usage(format!(
            "edit_file: old_string is empty but `{rel}` has content; quote the passage to \
             replace, or use write_file with the whole new content"
        )));
    }
    let n = cur.text.matches(&old).count();
    if n == 0 {
        return Err(ToolError::Usage(format!(
            "edit_file: old_string not found in `{rel}`; it must match the file exactly, \
             including whitespace and indentation. Read the file again if in doubt"
        )));
    }
    if n > 1 && !a.replace_all {
        return Err(ToolError::Usage(format!(
            "edit_file: old_string occurs {n} times in `{rel}`; include more context to \
             make it unique, or set replace_all to true"
        )));
    }
    let after = if a.replace_all {
        cur.text.replace(&old, &new)
    } else {
        cur.text.replacen(&old, &new, 1)
    };
    if after == cur.text {
        return Err(ToolError::Usage(format!(
            "edit_file: old_string and new_string leave `{rel}` unchanged"
        )));
    }
    Ok(PendingEdit {
        tool: Tool::EditFile,
        path: rel,
        diff: diff(&cur.text, &after),
        before: cur.text,
        after,
        eol: cur.eol,
        bom: cur.bom,
        expect: Some(cur.hash),
    })
}
