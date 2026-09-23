//! `list_dir`: what a folder holds, folders first, without `target/`,
//! `node_modules/` and `.git/`.

use serde::Deserialize;
use serde_json::{json, Value};

use super::{args, Tool, ToolError};
use crate::sandbox::Sandbox;

#[derive(Debug, Deserialize)]
pub struct Args {
    /// The project root if omitted.
    #[serde(default)]
    pub path: Option<String>,
}

pub fn spec() -> (&'static str, Value) {
    (
        "List the files and folders of a directory of the project. The project root if no path is given.",
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Folder relative to the project root; the root if omitted"}
            }
        }),
    )
}

pub fn run(sandbox: &Sandbox, arguments: &Value) -> Result<String, ToolError> {
    let a: Args = args(Tool::ListDir, arguments)?;
    let path = a
        .path
        .as_deref()
        .filter(|p| !p.trim().is_empty())
        .unwrap_or(".");
    let entries = sandbox.list(path)?;
    if entries.is_empty() {
        return Ok(format!(
            "`{}` is empty",
            sandbox.relative(path).unwrap_or_else(|_| ".".into())
        ));
    }
    Ok(entries
        .into_iter()
        .map(|e| {
            if e.dir {
                format!("{}/", e.name)
            } else {
                format!("{} ({} bytes)", e.name, e.size)
            }
        })
        .collect::<Vec<_>>()
        .join("\n"))
}
