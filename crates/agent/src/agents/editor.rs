//! The editor: reads, lists, edits and writes inside the project. The reader
//! is the same agent with the two writing capabilities off, and a prompt
//! that says so; `Agent::for_policy` picks between them, and `editor_with`
//! is the shorthand for the two boxes.

use moon_core::config::ids::{CREATE_FILES, EDIT_FILES, READ_FILES};
use moon_core::Permission;

use super::Agent;
use crate::tools::catalog::Policy;

const READER_PROMPT: &str = "\
# Reading files

You have tools to read files in this project, and you use them: a reply that says you \
will look at a file, without the call, is wrong. Make the call in that same reply. \
Paths are relative to the project root; an absolute path the user gives inside the \
project also works, so use it as it is. Never use `..` or `~`, and if a file is not \
found, list_dir before guessing another path.

- To see a file, call read_file; to see what a folder holds, call list_dir.
- You cannot change files in this conversation: no editing, no creating. When a change \
is asked for, show it as a snippet in the reply and say which file and where it goes.";

const PROMPT: &str = "\
# Editing files

You have tools to read and change files in this project, and you use them: a reply that \
says you will read, create or edit a file, without the call, is wrong. Make the call in \
that same reply. Paths are relative to the project root; an absolute path the user gives \
inside the project also works, so use it as it is. Never use `..` or `~`, and if a file \
is not found, list_dir before guessing another path.

- To see a file, call read_file. Read a file before editing it.
- To change part of a file, call edit_file with the exact passage in old_string, \
indentation included; it must occur once.
- To create a file, or to fill an empty one, call write_file with the whole content.
- If a request leaves out something a call needs, such as a file name, choose a \
sensible one and say which.
- You cannot delete or rename files.

When a request is done, say in one sentence what you changed.";

/// Reads, edits and creates, every write with the user's ok.
pub fn editor() -> Agent {
    Agent {
        name: "editor",
        prompt: PROMPT,
        policy: Policy::from_pairs([
            (READ_FILES, Permission::Allow),
            (EDIT_FILES, Permission::Ask),
            (CREATE_FILES, Permission::Ask),
        ]),
    }
}

/// Reads and lists, nothing else.
pub fn reader() -> Agent {
    Agent {
        name: "reader",
        prompt: READER_PROMPT,
        policy: Policy::from_pairs([(READ_FILES, Permission::Allow)]),
    }
}

/// The agent for the two writing boxes: neither on is the reader; the
/// editor otherwise, without the capability of the box that is off.
pub fn editor_with(edit: bool, create: bool) -> Agent {
    let mut policy = Policy::from_pairs([(READ_FILES, Permission::Allow)]);
    if edit {
        policy.set(EDIT_FILES, Permission::Ask);
    }
    if create {
        policy.set(CREATE_FILES, Permission::Ask);
    }
    Agent::for_policy(policy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;

    #[test]
    fn offers_the_four_tools_and_no_shell() {
        let a = editor();
        let names: Vec<String> = a.specs().into_iter().map(|s| s.name).collect();
        assert_eq!(
            names,
            vec!["read_file", "list_dir", "edit_file", "write_file"]
        );
        assert!(a.has(Tool::EditFile));
        assert!(a.system_prompt().contains("no shell"));
        // the rule the small models need most: call, do not announce
        assert!(a.prompt.contains("without the call, is wrong"));
        // what happens to a write follows the permission, not the prompt
        assert!(a.system_prompt().contains("applies or skips each"));
        let mut loose = editor().policy;
        loose.set(CREATE_FILES, Permission::Allow);
        let p = Agent::for_policy(loose).system_prompt();
        assert!(p.contains("New files are written as you make them"), "{p}");
        assert!(p.contains("Edits to existing files are shown"), "{p}");
        let no_create = editor().without(Tool::WriteFile);
        assert!(!no_create.has(Tool::WriteFile));
        assert_eq!(no_create.specs().len(), 3);
        assert_eq!(no_create.name, "editor");
        // without both writing tools it is the reader
        let none = no_create.without(Tool::EditFile);
        assert_eq!(none.name, "reader");
        assert_eq!(none.specs().len(), 2);
    }

    #[test]
    fn commands_bring_run_command_and_the_prompt_says_which() {
        let a = editor().with_commands(&["git diff", "git commit", "ls"]);
        assert!(a.has(Tool::RunCommand));
        let specs = a.specs();
        assert_eq!(specs.len(), 5);
        let run = specs.iter().find(|s| s.name == "run_command").unwrap();
        assert!(
            run.description.contains("ls, git diff, git commit"),
            "{}",
            run.description
        );
        assert_eq!(a.command_ids(), vec!["ls", "git diff", "git commit"]);
        let p = a.system_prompt();
        assert!(!p.contains("no shell"), "{p}");
        assert!(p.contains("only these, with run_command: ls, git diff, git commit"));
        assert!(p.contains("git commit wait for the user's ok"));
        assert!(p.contains("run from the project root"));
        // subfolders is not listed as a command, it turns the folder on
        let with_sub = editor().with_commands(&["ls", "commands in subfolders"]);
        let p = with_sub.system_prompt();
        assert!(p.contains("with run_command: ls."), "{p}");
        assert!(p.contains("pass it as `dir`"), "{p}");
        // none again: the tool goes with them
        let none = a.without(Tool::RunCommand);
        assert!(!none.has(Tool::RunCommand));
        assert_eq!(none.specs().len(), 4);
        assert!(none.system_prompt().contains("no shell"));
        // the reader can run commands too, and still cannot change files
        let r = reader().with_commands(&["cat"]);
        assert!(r.has(Tool::RunCommand) && !r.has(Tool::EditFile));
        assert!(r.system_prompt().contains("cannot change files"));
        // a command set to ask by hand is said so, whatever the catalogue's default
        let mut p = editor().policy;
        p.set("ls", Permission::Ask);
        let a = Agent::for_policy(p);
        assert!(a.system_prompt().contains("ls wait for the user's ok"));
    }

    #[test]
    fn the_reader_only_reads_and_its_prompt_says_so() {
        let r = reader();
        assert_eq!(r.name, "reader");
        assert!(r.has(Tool::ReadFile) && r.has(Tool::ListDir));
        assert!(!r.has(Tool::EditFile) && !r.has(Tool::WriteFile));
        assert!(r.prompt.contains("cannot change files"));
        assert!(r.system_prompt().contains("no shell"));
        assert!(!r.prompt.contains("edit_file"));
        // the panel's four combinations
        assert_eq!(editor_with(false, false), reader());
        assert_eq!(editor_with(true, true), editor());
        let e = editor_with(true, false);
        assert!(e.has(Tool::EditFile) && !e.has(Tool::WriteFile));
        let c = editor_with(false, true);
        assert!(!c.has(Tool::EditFile) && c.has(Tool::WriteFile));
        assert_eq!(c.prompt, editor().prompt);
        // a policy with nothing gives the reader with no tools at all
        let empty = Agent::for_policy(Policy::default());
        assert!(empty.tools().is_empty() && empty.name == "reader");
    }
}
