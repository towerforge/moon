//! The editor: reads, lists, edits and writes inside the project. The reader
//! is the same agent with the two writing tools taken away, and a prompt that
//! says so; `editor_with` picks between them from the `/tools` panel.

use super::Agent;
use crate::tools::Tool;

const READER_PROMPT: &str = "\
# Reading files

You have tools to read files in this project, and you use them: a reply that says you \
will look at a file, without the call, is wrong. Make the call in that same reply. \
Paths are relative to the project root; an absolute path the user gives inside the \
project also works, so use it as it is. Never use `..` or `~`, and if a file is not \
found, list_dir before guessing another path.

- To see a file, call read_file; to see what a folder holds, call list_dir.
- You cannot change files in this conversation: no editing, no creating. When a change \
is asked for, show it as a snippet in the reply and say which file and where it goes.
- You have no shell: you cannot run commands, tests or builds.";

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
- Every edit is shown to the user, who applies or skips it; a skipped edit is not on disk.
- You have no shell: you cannot run commands, tests or builds, and you cannot delete or \
rename files.

When a request is done, say in one sentence what you changed.";

pub fn editor() -> Agent {
    Agent {
        name: "editor",
        prompt: PROMPT,
        tools: vec![
            Tool::ReadFile,
            Tool::ListDir,
            Tool::EditFile,
            Tool::WriteFile,
        ],
    }
}

/// Reads and lists, nothing else: what the panel gives with both writing
/// boxes off.
pub fn reader() -> Agent {
    Agent {
        name: "reader",
        prompt: READER_PROMPT,
        tools: vec![Tool::ReadFile, Tool::ListDir],
    }
}

/// The agent for the panel's two boxes: neither on is the reader; the editor
/// otherwise, without the tool of the box that is off.
pub fn editor_with(edit: bool, create: bool) -> Agent {
    match (edit, create) {
        (false, false) => reader(),
        (true, true) => editor(),
        (true, false) => editor().without(Tool::WriteFile),
        (false, true) => editor().without(Tool::EditFile),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offers_the_four_tools_and_no_shell() {
        let a = editor();
        let names: Vec<String> = a.specs().into_iter().map(|s| s.name).collect();
        assert_eq!(
            names,
            vec!["read_file", "list_dir", "edit_file", "write_file"]
        );
        assert!(a.has(Tool::EditFile));
        assert!(a.prompt.contains("no shell"));
        // the rule the small models need most: call, do not announce
        assert!(a.prompt.contains("without the call, is wrong"));
        let no_create = editor().without(Tool::WriteFile);
        assert!(!no_create.has(Tool::WriteFile));
        assert_eq!(no_create.specs().len(), 3);
    }

    #[test]
    fn the_reader_only_reads_and_its_prompt_says_so() {
        let r = reader();
        assert_eq!(r.name, "reader");
        assert!(r.has(Tool::ReadFile) && r.has(Tool::ListDir));
        assert!(!r.has(Tool::EditFile) && !r.has(Tool::WriteFile));
        assert!(r.prompt.contains("cannot change files"));
        assert!(r.prompt.contains("no shell"));
        assert!(!r.prompt.contains("edit_file"));
        // the panel's four combinations
        assert_eq!(editor_with(false, false), reader());
        assert_eq!(editor_with(true, true), editor());
        let e = editor_with(true, false);
        assert!(e.has(Tool::EditFile) && !e.has(Tool::WriteFile));
        let c = editor_with(false, true);
        assert!(!c.has(Tool::EditFile) && c.has(Tool::WriteFile));
        assert_eq!(c.prompt, editor().prompt);
    }
}
