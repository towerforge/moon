//! Slash commands. They are recognized at the start of the input.

pub struct Spec {
    pub name: &'static str,
    pub args: &'static str,
    pub help: &'static str,
}

pub const SPECS: &[Spec] = &[
    Spec {
        name: "model",
        args: "",
        help: "switch model: opens the list to pick one",
    },
    Spec {
        name: "provider",
        args: "[id]",
        help: "provider status, or set the default provider",
    },
    Spec {
        name: "new",
        args: "",
        help: "new conversation",
    },
    Spec {
        name: "clear",
        args: "",
        help: "clear the view without closing the conversation",
    },
    Spec {
        name: "system",
        args: "[text]",
        help: "show or set the system prompt",
    },
    Spec {
        name: "params",
        args: "key=value …",
        help: "generation parameters (temperature, num_ctx, top_p, max_tokens, think, stop)",
    },
    Spec {
        name: "sessions",
        args: "",
        help: "resume a saved conversation (also ctrl+s); ctrl+r renames, ctrl+d deletes",
    },
    Spec {
        name: "save",
        args: "[name]",
        help: "save the conversation and, optionally, rename it",
    },
    Spec {
        name: "export",
        args: "[path.md]",
        help: "export the conversation as markdown",
    },
    Spec {
        name: "copy",
        args: "",
        help: "copy the last reply to the clipboard",
    },
    Spec {
        name: "retry",
        args: "",
        help: "regenerate the last reply",
    },
    Spec {
        name: "undo",
        args: "",
        help: "remove the last question/reply pair",
    },
    Spec {
        name: "files",
        args: "",
        help: "attached files: see what they cost, detach them and add more (also ctrl+f)",
    },
    Spec {
        name: "context",
        args: "",
        help: "what the model sees: context file, attached files, token budget",
    },
    Spec {
        name: "machine",
        args: "",
        help: "cpu, ram and swap drawn over the last 3 minutes",
    },
    Spec {
        name: "help",
        args: "",
        help: "this help",
    },
    Spec {
        name: "quit",
        args: "",
        help: "quit",
    },
];

pub const KEYS: &[(&str, &str)] = &[
    ("enter", "send"),
    (
        "ctrl+j · alt+enter · shift+enter",
        "newline (shift+enter only with the kitty keyboard protocol)",
    ),
    ("esc", "cancel the generation · close the panel"),
    ("ctrl+c", "cancel; twice with an empty input, quit"),
    (
        "ctrl+d · del",
        "quit if the input is empty · in the sessions panel, delete the highlighted session",
    ),
    (
        "ctrl+r",
        "in the sessions panel, rename the highlighted session",
    ),
    (
        "1-9 · alt+1-9 · 12",
        "in a list, go to the row with that number and open it; past the ninth it takes two digits, or enter to settle for the row you are on; while you are typing a filter the digits belong to it, alt always jumps",
    ),
    ("ctrl+p", "model panel"),
    (
        "ctrl+f",
        "files panel: what is attached, what it costs, and the tree to attach more",
    ),
    ("ctrl+s", "sessions panel: resume a saved conversation"),
    ("↑ · ↓", "prompt history (on the first / last line)"),
    ("pgup · pgdn · ctrl+↑ · ctrl+↓", "scroll the conversation"),
    (
        "ctrl+end · ctrl+home",
        "jump to bottom (and follow the reply) · to top",
    ),
    ("tab", "complete a command, or a path after @"),
    (
        "@path · @path:40-120 · @!path",
        "attach a file (or a line range) to this message; ! skips the secrets filter",
    ),
    (
        "ctrl+w · ctrl+u · ctrl+k",
        "delete word · to line start · to line end",
    ),
    ("ctrl+a · ctrl+e", "start · end of line"),
];

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Model,
    Provider(Option<String>),
    New,
    Clear,
    System(Option<String>),
    Params(String),
    Sessions,
    Save(Option<String>),
    Export(Option<String>),
    Copy,
    Retry,
    Undo,
    Files,
    Context,
    /// The machine drawn: cpu and ram over the window `sysmon` keeps.
    Machine,
    Help,
    Quit,
}

/// `input` starts with `/`.
pub fn parse(input: &str) -> Result<Command, String> {
    let body = input.trim().strip_prefix('/').unwrap_or(input.trim());
    let (name, rest) = match body.split_once(char::is_whitespace) {
        Some((n, r)) => (n, r.trim()),
        None => (body, ""),
    };
    let arg = || {
        if rest.is_empty() {
            None
        } else {
            Some(rest.to_string())
        }
    };
    Ok(match name {
        "model" | "m" => Command::Model,
        "provider" | "providers" => Command::Provider(arg()),
        "new" => Command::New,
        "clear" => Command::Clear,
        "system" => Command::System(arg()),
        "params" | "param" => Command::Params(rest.to_string()),
        "sessions" | "resume" => Command::Sessions,
        "save" => Command::Save(arg()),
        "export" => Command::Export(arg()),
        "copy" => Command::Copy,
        "retry" => Command::Retry,
        "undo" => Command::Undo,
        // `/add` and `/drop` were two commands with arguments; now they are
        // one panel, and the old names open it
        "files" | "attach" | "add" | "drop" => Command::Files,
        "context" | "ctx" => Command::Context,
        "machine" => Command::Machine,
        "help" | "?" => Command::Help,
        "quit" | "exit" | "q" => Command::Quit,
        "" => return Err("type a command after the slash; /help lists them".into()),
        other => return Err(format!("unknown command: /{other} (try /help)")),
    })
}

/// Commands whose name starts with `prefix` (without the slash), in `SPECS` order.
pub fn matching(prefix: &str) -> Vec<&'static Spec> {
    SPECS
        .iter()
        .filter(|s| s.name.starts_with(prefix))
        .collect()
}

/// Command names that start with `prefix` (without the slash).
pub fn complete(prefix: &str) -> Vec<&'static str> {
    matching(prefix).into_iter().map(|s| s.name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses() {
        // the model is not typed in: `/model` opens the list, and what may
        // come after it is not a model to pick
        assert_eq!(parse("/model"), Ok(Command::Model));
        assert_eq!(parse("/machine"), Ok(Command::Machine));
        assert!(parse("/cpu").is_err());
        assert_eq!(parse("/model qwen"), Ok(Command::Model));
        assert_eq!(parse("/m"), Ok(Command::Model));
        assert_eq!(
            parse("/params temperature=0.2 num_ctx=8"),
            Ok(Command::Params("temperature=0.2 num_ctx=8".into()))
        );
        assert_eq!(parse("/q"), Ok(Command::Quit));
        assert_eq!(
            parse("/system  be brief "),
            Ok(Command::System(Some("be brief".into())))
        );
        assert!(parse("/nope").is_err());
        assert!(parse("/").is_err());
    }

    #[test]
    fn completes() {
        assert_eq!(complete("mod"), vec!["model"]);
        assert_eq!(complete("se"), vec!["sessions"]);
        assert_eq!(parse("/resume"), Ok(Command::Sessions));
        assert!(parse("/models").is_err());
        assert_eq!(complete("q"), vec!["quit"]);
        // `/add` and `/drop` are gone as commands, but they still open the panel
        assert_eq!(parse("/add"), Ok(Command::Files));
        assert_eq!(parse("/drop"), Ok(Command::Files));
        assert!(complete("add").is_empty());
        assert!(complete("zz").is_empty());
        assert_eq!(matching("").len(), SPECS.len());
        assert_eq!(matching("he")[0].name, "help");
    }
}
