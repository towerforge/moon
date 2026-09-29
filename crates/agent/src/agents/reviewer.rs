//! The reviewer: reads the project and looks at the repository, reports what
//! it finds and never writes. `moon config init` writes it as
//! `reviewer.toml`, a sample agent to start from; once a file, it is the
//! user's to change.

use moon_core::config::ids::READ_FILES;
use moon_core::Permission;

use super::AgentDef;
use crate::tools::catalog::Policy;

const PROMPT: &str = "\
# Reviewing

You review this project: you read files and look at the repository, and you \
use your tools to do it — a reply that says you will look at something, \
without the call, is wrong. Make the call in that same reply. Paths are \
relative to the project root; an absolute path the user gives inside the \
project also works, so use it as it is. Never use `..` or `~`, and if a file \
is not found, list_dir before guessing another path.

- To see a file, call read_file; to see what a folder holds, call list_dir.
- You cannot change files or the repository in this conversation. When a \
change is called for, show it as a snippet in the reply and say which file \
and where it goes.
- Report plainly: what is wrong, where, and why it matters. What is fine \
gets one line, not a paragraph.";

/// Reads the project and reviews changes; never writes.
pub fn reviewer() -> AgentDef {
    AgentDef {
        name: "reviewer".into(),
        description: "reads the project and reviews changes; never writes".into(),
        prompt: Some(PROMPT.into()),
        policy: Policy::from_pairs([
            (READ_FILES, Permission::Allow),
            ("git status", Permission::Allow),
            ("git diff", Permission::Allow),
            ("git log", Permission::Allow),
            ("git show", Permission::Allow),
            ("git blame", Permission::Allow),
        ]),
        max_steps: Some(super::DEFAULT_STEPS),
    }
}
