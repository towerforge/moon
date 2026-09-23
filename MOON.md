# moon

Terminal chat client for language models, written in Rust. Cargo workspace:

- `crates/core` (`moon-core`): the `Provider` trait, `Registry`, configuration, XDG paths, JSONL sessions, file context (`context.rs`).
- `crates/providers/ollama` and `crates/providers/openai`: the two provider implementations (native Ollama API; OpenAI-compatible chat completions). Both send the request's `tools` and turn the model's tool calls into `ChatEvent::ToolCall`.
- `crates/agent` (`moon-agent`): the model editing files, behind `/tools on`. Four folders, four ideas: `harness/` (the loop, a pure state machine fed with events and answering with commands), `agents/` (the `editor`: prompt plus tool set), `tools/` (a closed enum: `read_file`, `list_dir`, `edit_file`, `write_file`; no shell), `sandbox/` (the boundary: every path stays under the start-up directory, `.git/` and secrets are never touched, writes are atomic). Depends on `moon-core` only; `docs/harness.md` is the design.
- `crates/tui` (`moon-tui`): the ratatui interface. `app/mod.rs` holds the state (Elm-style `Action`/`update`) and each other module under `app/` is one `impl App` about one concern (keys, chat, slash commands, files, models, render, status; `agent.rs` runs the harness's commands and the approval panel); `view/` is the rendering (`panel.rs` for the bottom panel: lists, help, dialogs and the approval panel, all docked under the conversation instead of floating over it; `diff.rs` paints a diff); `sysmon.rs` samples CPU and RAM for the bottom-right row.
- `crates/updater` (`moon-updater`): the GitHub releases API, version order, checksum, archive and the in-place swap of the binary. No terminal and no provider: `moon-tui` puts the panel on top (`update/`) and the start-up check reads its daily cache.
- `crates/cli` (`moon-cli`): the `moon` binary.

Conventions: identifiers, comments, log messages and the Markdown docs (`README.md`, `CONTRIBUTING.md`) in English; every user-facing string in English. No source file over 1000 lines. Visual identity: the Moon design system (single brand color `#8fb8ff`, pixel crescent, square corners).
