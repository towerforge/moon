//! Who talks to the model: a prompt, the tools it may use, and nothing else.
//! An agent is a value; adding one is another file in this folder. Two so
//! far, the editor and the reader, both in `editor.rs`.

use moon_core::ToolSpec;

use crate::tools::Tool;

mod editor;

pub use editor::{editor, editor_with, reader};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    pub name: &'static str,
    /// Appended to the system prompt while the agent is on.
    pub prompt: &'static str,
    pub tools: Vec<Tool>,
}

impl Agent {
    /// What goes in the request's `tools`.
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.iter().map(|t| t.spec()).collect()
    }

    pub fn has(&self, tool: Tool) -> bool {
        self.tools.contains(&tool)
    }

    /// The same agent with one tool fewer: the editor without `write_file`
    /// edits what exists and creates nothing.
    pub fn without(mut self, tool: Tool) -> Self {
        self.tools.retain(|t| *t != tool);
        self
    }
}
