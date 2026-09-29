//! Who talks to the model: a prompt, and a policy that says what it may do
//! and with what supervision. The tools offered follow from the policy. An
//! agent is a value; the two prompts, the editor's and the reader's, are in
//! `editor.rs`.
//!
//! An `AgentDef` is an agent as it is defined: one file under the agents
//! folder, `default.toml` included. Since 2026-09-28 an agent carries its
//! permissions whole — choosing an agent is choosing its policy — and since
//! 2026-09-29 every agent is a file, `default` and `reviewer` too. Agent
//! files live only in the user's configuration folder, the same trust as
//! the configuration itself; a project-shipped agent must never load
//! without an explicit confirmation, because its file GRANTS.

use moon_core::{Permission, ToolSpec};

use crate::tools::catalog::Policy;
use crate::tools::{run_command, Tool};
use moon_core::config::ids::{CREATE_FILES, EDIT_FILES};

mod editor;
mod file;
mod reviewer;

pub use editor::{editor, editor_with, reader};
pub use file::{defs_from_dir, ensure_default, file_name, sync_dir, AgentFile};
pub use reviewer::reviewer;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    pub name: String,
    /// Appended to the system prompt while the agent is on, before the
    /// part about commands.
    pub prompt: String,
    /// What it may do: the tools and the commands follow from it.
    pub policy: Policy,
}

/// The name every conversation starts with, and the first row of the
/// picker: `default.toml`, the one file the folder always has.
pub const DEFAULT_AGENT: &str = "default";
/// `max steps per message` for an agent whose file sets none.
pub const DEFAULT_STEPS: usize = 8;

/// An agent as it is defined, from its file: a prompt, and its permissions
/// whole. It has no tools, sandbox or catalogue of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDef {
    pub name: String,
    /// One line for the picker.
    pub description: String,
    /// Its own prompt; none means moon's own — the editor's or the
    /// reader's, whichever the policy calls for.
    pub prompt: Option<String>,
    /// Its permissions, whole: what it lists is what it may do. Whatever
    /// the source, they went through `Policy::from_pairs`, so unknown ids
    /// are dropped and the coupling rules hold.
    pub policy: Policy,
    /// Its own step limit; none is `DEFAULT_STEPS`.
    pub max_steps: Option<usize>,
}

impl AgentDef {
    /// `default` as `moon config init` writes it: reading on, nothing else,
    /// moon's own prompt.
    pub fn factory_default() -> Self {
        AgentDef {
            name: DEFAULT_AGENT.into(),
            description: "the agent every conversation starts with".into(),
            prompt: None,
            policy: Policy::from_pairs([(moon_core::config::ids::READ_FILES, Permission::Allow)]),
            max_steps: Some(DEFAULT_STEPS),
        }
    }

    /// What stands in for `default.toml` where there is no folder to read
    /// it from (the tests, a folder that cannot be written): the name,
    /// and nothing on.
    pub fn default_agent() -> Self {
        AgentDef {
            policy: Policy::default(),
            max_steps: None,
            ..Self::factory_default()
        }
    }

    /// The two agents `moon config init` writes, in the order the picker
    /// shows them.
    pub fn factory() -> Vec<AgentDef> {
        vec![Self::factory_default(), reviewer()]
    }

    /// The definitions there are without a folder: `default` with nothing
    /// on, and the `reviewer`.
    pub fn builtin() -> Vec<AgentDef> {
        vec![Self::default_agent(), reviewer()]
    }

    /// The step limit the agent runs under.
    pub fn steps(&self) -> usize {
        self.max_steps.unwrap_or(DEFAULT_STEPS)
    }

    /// The runtime agent this definition gives: its permissions and, for
    /// a definition without a prompt of its own, the prompt the policy
    /// calls for.
    pub fn agent(&self) -> Agent {
        match &self.prompt {
            None => Agent::for_policy(self.policy.clone()),
            Some(p) => Agent {
                name: self.name.clone(),
                prompt: p.clone(),
                policy: self.policy.clone(),
            },
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn user() -> Policy {
        Policy::from_pairs([
            (moon_core::config::ids::READ_FILES, Permission::Allow),
            (EDIT_FILES, Permission::Allow),
            ("git diff", Permission::Allow),
            ("git commit", Permission::Ask),
            ("cargo test", Permission::Ask),
        ])
    }

    #[test]
    fn an_agent_carries_its_permissions_whole() {
        // without a prompt of its own, the policy picks moon's: the editor
        // when it may write, the reader otherwise
        let d = AgentDef {
            policy: user(),
            ..AgentDef::default_agent()
        };
        assert_eq!(d.agent().name, "editor");
        assert_eq!(d.agent().policy, user());
        assert_eq!(AgentDef::default_agent().agent().name, "reader");
        assert!(AgentDef::default_agent().policy.is_empty());
        assert_eq!(AgentDef::default_agent().steps(), DEFAULT_STEPS);
        // one with a prompt keeps its name and its policy, whole
        let own = AgentDef {
            prompt: Some("You commit.".into()),
            policy: Policy::from_pairs([
                ("git status", Permission::Allow),
                ("git commit", Permission::Allow),
                ("rm -rf", Permission::Allow), // not in the catalogue: dropped
            ]),
            max_steps: Some(3),
            name: "committer".into(),
            ..AgentDef::default_agent()
        };
        let a = own.agent();
        assert_eq!(a.name, "committer");
        assert_eq!(a.policy.get("git status"), Permission::Allow);
        assert_eq!(a.policy.get("git commit"), Permission::Allow);
        assert_eq!(a.policy.get("rm -rf"), Permission::Off);
        assert_eq!(a.policy.get(EDIT_FILES), Permission::Off);
        assert_eq!(own.steps(), 3);
        // what config init writes: default reads, and both have the
        // default limit spelled out
        let factory = AgentDef::factory();
        assert_eq!(factory[0].name, DEFAULT_AGENT);
        assert!(factory[0].policy.reads() && !factory[0].policy.edits());
        assert!(factory.iter().all(|d| d.max_steps == Some(DEFAULT_STEPS)));
    }

    #[test]
    fn the_reviewer_reads_and_looks_and_never_writes() {
        let d = reviewer();
        let a = d.agent();
        assert_eq!(a.name, "reviewer");
        assert!(a.has(Tool::ReadFile) && !a.has(Tool::EditFile) && !a.has(Tool::WriteFile));
        assert_eq!(
            a.command_ids(),
            vec!["git status", "git diff", "git log", "git show", "git blame"]
        );
        assert!(a.system_prompt().contains("# Reviewing"));
        // the built-ins, for a run without a folder: default first
        let names: Vec<String> = AgentDef::builtin().into_iter().map(|d| d.name).collect();
        assert_eq!(names, vec![DEFAULT_AGENT, "reviewer"]);
    }
}
