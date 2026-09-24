//! Who talks to the model: a prompt, and a policy that says what it may do
//! and with what supervision. The tools offered follow from the policy. An
//! agent is a value; the two prompts, the editor's and the reader's, are in
//! `editor.rs`.

use moon_core::{Permission, ToolSpec};

use crate::tools::catalog::Policy;
use crate::tools::{run_command, Tool};
use moon_core::config::ids::{CREATE_FILES, EDIT_FILES};

mod editor;

pub use editor::{editor, editor_with, reader};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    pub name: &'static str,
    /// Appended to the system prompt while the agent is on, before the
    /// part about commands.
    pub prompt: &'static str,
    /// What it may do: the tools and the commands follow from it.
    pub policy: Policy,
}

impl Agent {
    /// The agent a policy calls for: the editor when it may change files,
    /// the reader otherwise, either one with the commands the policy has.
    pub fn for_policy(policy: Policy) -> Self {
        let base = if policy.edits() || policy.creates() {
            editor()
        } else {
            reader()
        };
        Agent { policy, ..base }
    }

    /// The tools the policy gives, in the order the model sees them.
    pub fn tools(&self) -> Vec<Tool> {
        let mut out = Vec::new();
        if self.policy.reads() {
            out.push(Tool::ReadFile);
            out.push(Tool::ListDir);
        }
        if self.policy.edits() {
            out.push(Tool::EditFile);
        }
        if self.policy.creates() {
            out.push(Tool::WriteFile);
        }
        if !self.policy.commands().is_empty() {
            out.push(Tool::RunCommand);
        }
        out
    }

    /// What goes in the request's `tools`.
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools()
            .into_iter()
            .map(|t| match t {
                Tool::RunCommand => {
                    let (description, parameters) = run_command::spec(&self.policy);
                    ToolSpec {
                        name: t.name().to_string(),
                        description,
                        parameters,
                    }
                }
                _ => t.spec(),
            })
            .collect()
    }

    pub fn has(&self, tool: Tool) -> bool {
        self.tools().contains(&tool)
    }

    /// The same agent with one tool fewer: the editor without `write_file`
    /// edits what exists and creates nothing.
    pub fn without(mut self, tool: Tool) -> Self {
        let id = match tool {
            Tool::ReadFile | Tool::ListDir => moon_core::config::ids::READ_FILES,
            Tool::EditFile => moon_core::config::ids::EDIT_FILES,
            Tool::WriteFile => moon_core::config::ids::CREATE_FILES,
            Tool::RunCommand => {
                for e in self.policy.commands() {
                    self.policy.set(e.id, Permission::Off);
                }
                return self;
            }
        };
        self.policy.set(id, Permission::Off);
        Agent::for_policy(self.policy)
    }

    /// The same agent with these commands on, each at what the catalogue
    /// turns it to: `allow` for what only looks, `ask` for what changes
    /// things.
    pub fn with_commands<S: AsRef<str>>(mut self, ids: &[S]) -> Self {
        for id in ids {
            if let Some(e) = crate::tools::catalog::find(id.as_ref()) {
                self.policy.set(e.id, e.on);
            }
        }
        self
    }

    /// The ids of the commands that are on.
    pub fn command_ids(&self) -> Vec<&'static str> {
        self.policy.commands().iter().map(|e| e.id).collect()
    }

    /// What the model reads: the prompt and, after it, the commands it may
    /// run, or that it has no shell at all.
    pub fn system_prompt(&self) -> String {
        let mut out = self.prompt.to_string();
        out.push_str("\n\n");
        // what happens to a write: seen first, or written as it comes
        let writes = [
            (self.policy.get(EDIT_FILES), "Edits to existing files"),
            (self.policy.get(CREATE_FILES), "New files"),
        ];
        let notes: Vec<String> = writes
            .iter()
            .filter(|(p, _)| *p != Permission::Off)
            .map(|(p, what)| match p {
                Permission::Allow => {
                    format!("{what} are written as you make them, without waiting: take care.")
                }
                _ => format!(
                    "{what} are shown to the user, who applies or skips each; a skipped one is \
                     not on disk."
                ),
            })
            .collect();
        if !notes.is_empty() {
            out.push_str(&notes.join(" "));
            out.push_str("\n\n");
        }
        let commands = self.policy.commands();
        if commands.is_empty() {
            out.push_str(
                "You have no shell: you cannot run commands, tests or builds. When a check would \
                 need one, say what to run and let the user run it.",
            );
            return out;
        }
        let names: Vec<&str> = commands.iter().map(|e| e.id).collect();
        out.push_str(&format!(
            "# Running commands\n\nYou can run these commands, and only these, with run_command: \
             {}. Pass the command as written here in `command`, with any arguments after it or in \
             `args`, one per item. No shell: no pipes, no redirections, no `&&`, no `cd`. \
             Arguments are relative to the project root and stay inside it.",
            names.join(", ")
        ));
        let asks: Vec<&str> = commands
            .iter()
            .filter(|e| self.policy.get(e.id) == Permission::Ask)
            .map(|e| e.id)
            .collect();
        if !asks.is_empty() {
            out.push_str(&format!(
                " {} wait for the user's ok before running; a skipped one did not run.",
                asks.join(", ")
            ));
        }
        if self.policy.subfolders() {
            out.push_str(" To run inside a folder of the project, pass it as `dir`.");
        } else {
            out.push_str(" Commands run from the project root.");
        }
        out.push_str(
            " Use them to check your work: after an edit, run the build or the tests when \
             they are among them, and read what they say.",
        );
        out
    }
}
