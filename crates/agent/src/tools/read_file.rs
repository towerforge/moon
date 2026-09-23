//! `read_file`: a file, or a range of lines, in the same `<file>` block the
//! `@path` mentions use, so the model sees one shape. Leaves the hash and
//! the line-ending convention behind for the edit that may follow.

use moon_core::context::{estimate_tokens, Attachment};
use serde::Deserialize;
use serde_json::{json, Value};

use super::{args, Seen, SeenFiles, Tool, ToolError};
use crate::sandbox::{Denied, Sandbox};

#[derive(Debug, Deserialize)]
pub struct Args {
    pub path: String,
    /// `"40-120"`, `"40-"` or `"40"`.
    #[serde(default)]
    pub range: Option<String>,
}

pub fn spec() -> (&'static str, Value) {
    (
        "Read a text file of the project. Paths are relative to the project root. \
         Read a file before editing it. Optionally only a range of lines, \"40-120\".",
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path relative to the project root, e.g. src/main.rs"},
                "range": {"type": "string", "description": "Lines to read, \"40-120\" or \"40-\"; the whole file if omitted"}
            },
            "required": ["path"]
        }),
    )
}

/// `"40-120"`, `"40-"` (to the end) or `"40"` (one line), 1-based.
fn parse_range(r: &str) -> Option<(usize, usize)> {
    let (a, b) = match r.split_once('-') {
        Some((a, b)) => (a, b),
        None => (r, r),
    };
    let from: usize = a.trim().parse().ok()?;
    let to: usize = if b.trim().is_empty() {
        usize::MAX
    } else {
        b.trim().parse().ok()?
    };
    (from > 0 && to >= from).then_some((from, to))
}

/// A `not found` with the files it may have meant: small models drop the
/// first folders of a path (`src/…` for `frontend/src/…`) and then give up.
fn not_found(sandbox: &Sandbox, rel: String) -> ToolError {
    let found = sandbox.find(&rel);
    match found.as_slice() {
        [] => Denied::NotFound(rel).into(),
        [one] => ToolError::Usage(format!("`{rel}` not found; the file is `{one}`")),
        many => ToolError::Usage(format!(
            "`{rel}` not found; it may be one of: {}",
            many.iter()
                .take(5)
                .map(|p| format!("`{p}`"))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

pub fn run(
    sandbox: &Sandbox,
    seen: &mut SeenFiles,
    arguments: &Value,
) -> Result<String, ToolError> {
    let a: Args = args(Tool::ReadFile, arguments)?;
    let range = match &a.range {
        Some(r) if !r.trim().is_empty() => Some(parse_range(r.trim()).ok_or_else(|| {
            ToolError::Usage(format!("read_file: invalid range `{r}`; use \"40-120\""))
        })?),
        _ => None,
    };
    let f = match sandbox.read(&a.path) {
        Err(Denied::NotFound(rel)) => return Err(not_found(sandbox, rel)),
        r => r?,
    };
    seen.insert(
        f.rel.clone(),
        Seen {
            hash: f.hash,
            eol: f.eol,
            bom: f.bom,
        },
    );
    let content = match range {
        Some((from, to)) => {
            let lines: Vec<&str> = f.text.lines().collect();
            if from > lines.len() {
                return Err(ToolError::Usage(format!(
                    "read_file: range starts at line {from} but `{}` has {} lines",
                    f.rel,
                    lines.len()
                )));
            }
            lines[from - 1..to.min(lines.len())].join("\n")
        }
        None => f.text.clone(),
    };
    let tokens = estimate_tokens(&content);
    let block = Attachment {
        path: f.rel,
        content,
        hash: f.hash,
        tokens,
        range,
        truncated: false,
    }
    .block();
    Ok(block)
}
