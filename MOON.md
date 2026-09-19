# moon

Terminal chat client for language models, written in Rust. Cargo workspace:

- `crates/core` (`moon-core`): the `Provider` trait, `Registry`, configuration, XDG paths, JSONL sessions, file context (`context.rs`).
- `crates/providers/ollama` and `crates/providers/openai`: the two provider implementations (native Ollama API; OpenAI-compatible chat completions).
- `crates/tui` (`moon-tui`): the ratatui interface. `app/mod.rs` holds the state (Elm-style `Action`/`update`) and each other module under `app/` is one `impl App` about one concern (keys, chat, slash commands, files, models, render, status); `view/` is the rendering (`panel.rs` for the bottom panel: lists, help and dialogs, all docked under the conversation instead of floating over it); `sysmon.rs` samples CPU and RAM for the bottom-right row.
- `crates/cli` (`moon-cli`): the `moon` binary.

Conventions: identifiers, comments, log messages and the Markdown docs (`README.md`, `CONTRIBUTING.md`) in English; every user-facing string in English. No source file over 1000 lines. Visual identity: the Moon design system (single brand color `#8fb8ff`, pixel crescent, square corners).
