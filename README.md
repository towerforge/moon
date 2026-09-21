<p align="center">
  <img src="assets/readme/hero.svg" alt="moon: chat with local language models, from your terminal" width="100%">
</p>

<p align="center">
  <a href="#installation"><img alt="Rust 1.88 or newer" src="https://img.shields.io/badge/rust-1.88%2B-8fb8ff?style=flat-square&logo=rust&logoColor=1c1c29"></a>
  <a href="LICENSE"><img alt="MIT license" src="https://img.shields.io/badge/license-MIT-8fb8ff?style=flat-square"></a>
  <img alt="macOS, Linux and Windows" src="https://img.shields.io/badge/platforms-macOS%20%C2%B7%20Linux%20%C2%B7%20Windows-8fb8ff?style=flat-square">
  <img alt="Ollama and OpenAI-compatible providers" src="https://img.shields.io/badge/providers-Ollama%20%C2%B7%20OpenAI--compatible-8fb8ff?style=flat-square">
</p>

<p align="center">
  A fast, keyboard-first chat client for language models that run on your own machine.<br>
  Ollama out of the box, any OpenAI-compatible server next to it, and the state of your hardware always in view.
</p>

<p align="center">
  <img src="assets/readme/screenshot.svg" alt="moon answering a question in the terminal" width="900">
</p>

## Features

- **Local first.** Talks to Ollama over its native API, so it knows what only Ollama can tell: context windows, quantization, which model is in memory and how much it takes. Any OpenAI-compatible server (LM Studio, llama.cpp, vLLM, OpenRouter…) plugs in with three lines of configuration.
- **A real chat interface.** Streaming Markdown with syntax-highlighted code, a status line that shows what the model is doing, tokens sent and received, speed, and how much of the context window the conversation fills.
- **Your files, your rules.** Attach a file or a line range with `@path`, keep files attached for the whole conversation from the files panel (`Ctrl+F`), and put a `MOON.md` in a project so the model knows what it is looking at. moon never runs tools or writes to disk on the model's behalf.
- **Sessions that survive.** Every conversation is saved as JSONL. Resume the last one, pick any from a list sorted by title — with the five you last opened on top once there are enough of them to be worth it — rename, delete, export to Markdown.
- **Switch models mid-conversation.** A fuzzy list that unfolds at the bottom, grouped by provider, with your recent models on top once there are enough of them to be worth it. The history stays; the next question goes to the new model.
- **The machine, always in view.** CPU and RAM in the corner, with the peak of the last three minutes, and the loaded model's footprint next to it. When a model spills to the CPU or to swap, you see it before you feel it.
- **Scriptable.** `moon ask` streams a reply to stdout, reads the prompt from a pipe, and prints token stats to stderr.
- **Fast, small, private.** One Rust binary. No telemetry, no network traffic except to the providers you configure: `moon update` goes to GitHub when you run it, and the start-up check stays off until you turn it on.

## Installation

moon is one binary with nothing else to install. It runs on macOS, Linux and Windows. The Windows build is recent and has had less testing: use Windows Terminal or another terminal with ANSI support, and open an issue if something misbehaves.

#### Linux / macOS

```sh
curl -fsSL https://raw.githubusercontent.com/towerforge/moon/main/install.sh | sh
```

The installer detects your OS and architecture, downloads the latest release, verifies its SHA-256 and installs `moon` to `/usr/local/bin` when run as root, otherwise to `~/.local/bin`. It is for the first install: run it again with moon already there and it stops and points you at `moon update`, which is what upgrades from then on.

| Variable | Effect |
|---|---|
| `MOON_INSTALL_DIR=/your/path` | install somewhere else |
| `MOON_VERSION=0.2.0` | install a specific version instead of the latest |
| `MOON_VARIANT=musl` · `gnu` | force the static (musl) or the glibc binary on Linux |
| `MOON_FORCE=1` | install over a moon that is already there instead of pointing at `moon update` |

#### Windows

```powershell
irm https://raw.githubusercontent.com/towerforge/moon/main/install.ps1 | iex
```

Installs `moon.exe` to `%LOCALAPPDATA%\Programs\moon`, verifies its SHA-256 and adds that folder to your user `PATH`. Set `$env:MOON_INSTALL_DIR` to install somewhere else and `$env:MOON_VERSION` to pin a version, and `$env:MOON_FORCE = "1"` to install over a moon that is already there. As on Linux and macOS, upgrading is `moon update`'s job — and the PowerShell installer cannot replace a `moon.exe` that is running, while `moon update` can.

#### Manual download

Pre-built binaries are on the [releases page](https://github.com/towerforge/moon/releases/latest). Extract the archive and put `moon` (or `moon.exe`) somewhere in your `PATH`. `checksums.txt` next to the assets has the SHA-256 of every archive.

**Linux**

| Platform | Asset |
|---|---|
| x86_64 (glibc) | `moon-linux-x86_64.tar.gz` |
| x86_64 (static) | `moon-linux-x86_64-musl.tar.gz` |
| ARM64 (glibc) | `moon-linux-aarch64.tar.gz` |
| ARM64 (static) | `moon-linux-aarch64-musl.tar.gz` |

**macOS**

| Platform | Asset |
|---|---|
| Intel | `moon-macos-x86_64.tar.gz` |
| Apple Silicon | `moon-macos-aarch64.tar.gz` |

**Windows**

| Platform | Asset |
|---|---|
| x86_64 | `moon-windows-x86_64.zip` |

#### Updating

```sh
moon update          # what the new release brings, and it installs it once you say yes
moon update --check  # says what there is and installs nothing
```

This is the only way moon upgrades: the installers above do the first install and then step aside. `moon update` downloads the release for your platform, checks its SHA-256 against `checksums.txt` and swaps the binary in place: the one that is running stays until the new one is written. It only replaces a binary that came from a release: one from `cargo install` or a build under `target/` is left alone, and it says so.

| Flag | Effect |
|---|---|
| `--check` | check and report, install nothing |
| `-y` · `--yes` | install without asking; required when the output is not a terminal |
| `--to 0.2.0` | that version instead of the latest, downgrades included |
| `--force` | reinstall the same version, or overwrite a `cargo install` or a local build |

If the binary lives where you cannot write, `sudo moon update` does it; and `install.sh` with `MOON_FORCE=1` is always there as a way back in.

moon does not look for updates by itself. With `update_check = true` under `[general]` it asks GitHub once a day and, when there is something newer, says so at startup next to the version.

#### From source

Needs Rust 1.88 or newer.

```sh
cargo install --git https://github.com/towerforge/moon moon-cli
```

Or from a checkout:

```sh
git clone https://github.com/towerforge/moon
cd moon
make install     # cargo install --path crates/cli --locked
```

## Getting started

1. Have [Ollama](https://ollama.com) running and a model pulled:

   ```sh
   ollama pull qwen2.5-coder:14b
   ```

2. Start moon. With no configuration it finds Ollama at `http://localhost:11434` (or wherever `OLLAMA_HOST` points) and picks the first model it sees:

   ```sh
   moon
   ```

3. Type a question and press `Enter`. `Esc` cancels a reply. `/model` or `Ctrl+P` switches models. `/help` lists everything else.

4. When you want to change something, write a commented configuration file and edit it:

   ```sh
   moon config init     # writes ~/.config/moon/config.toml
   ```

## Usage

### The interface

From top to bottom: a header with the crescent, the version, the active model and the directory you started in; the conversation; a status line; the input box; and a row of hints.

The **bottom panel** is where every list lives: models, sessions, files and the help. It unfolds under the conversation, in the input box's place, and the box comes back when it closes. Nothing floats over what you are reading and nothing paints a surface of its own:

```
 Select model                                         12 models · 2 providers
 the conversation keeps its history · type to filter

 Recent ────────────────────────────────────────────────────────── 2 models  █
 ❯  1. llama3.1:8b ✓                ollama · 8B · Q4_0 · 4.7 GB · ctx 32.8k  █
    2. gemma3:12b                  ollama · 12B · Q4_0 · 8.1 GB · ctx 32.8k  █
                                                                             █
 ollama ───────────────────────────────────────────────── ● localhost:11434  █
    3. llama3.1:8b ✓                         8B · Q4_0 · 4.7 GB · ctx 32.8k  █
    4. gemma3:12b                           12B · Q4_0 · 8.1 GB · ctx 32.8k  │
    5. qwen2.5-coder:14b                  14B · Q4_K_M · 9.0 GB · ctx 32.8k  │
    6. deepseek-r1:7b                      7B · Q4_K_M · 4.4 GB · ctx 32.8k  │
    7. mistral-nemo:12b                     12B · Q4_0 · 7.1 GB · ctx 32.8k  │
    8. phi4:14b                             14B · Q4_0 · 9.1 GB · ctx 32.8k  │

 ↑↓ move · number jump · enter select · esc close                     1-11/19
```

- The title names it and counts what is in it. Under it, what the list is for, or the filter as you type it.
- `❯` marks the row under the cursor; `✓`, right after the text, the one in use.
- Every row carries its number: press it and moon goes there and opens it. Past the ninth it takes two digits — `1` then `2` for the twelfth — and `Enter` settles for the row you are on. While you are typing a filter the digits belong to it; `Alt`+digit jumps anyway.
- The bar on the right says where you are when the list does not fit, and the footer, which keys work.
- `Esc` closes the panel. `Ctrl+D`, `Ctrl+R` and the rest keep working on the highlighted row.

The **status line** has two halves. On the left, what is happening now: `generating (12s · ↑ ~3.2k · ↓ 640 tokens · 38 tok/s) · esc to cancel`, then the wrap-up `✓ done (…)` or `✗ cancelled (…)` with the real token counts. On the right, `context 8%`, how much of the context window the last exchange filled (it turns `moon-soft` at 80 %, when models start to forget the beginning), and the session total: generation time and tokens sent and received. With Ollama the window counted is the one the model is loaded with — your `num_ctx`, not the one the model declares — because that is the one that truncates.

The **bottom row** shows the hints for the current situation on the left and, on the right, the active model and the machine:

```
qwen2.5-coder:14b · 12.1G · cpu 34% ▲61 · ram 57% ▲75
```

- `12.1G` is what the model takes in memory: weights plus the context cache, so it depends on `num_ctx` as much as on the model. If it is there, the model is loaded. If it is missing, the next request pays the loading time. Only Ollama can report this.
- If the model does not fit in the GPU, `30% cpu` appears in bold: that is when generation crawls.
- `cpu` and `ram` are sampled every 5 seconds, every second while the model is thinking or answering. After `▲`, the peak of the last 3 minutes. `swap 1.2G` appears only when swap is in use.
- Below 120 columns the peaks and the model size go; below 90, the whole block. `system_stats = false` turns the machine readings off.

`/context` prints the same in gigabytes, with the split between weights and context cache and how long until Ollama unloads the model.

### Slash commands

Type `/` and the commands that match appear over the box, drawn like the panel: a title with how many are left, `❯` on the highlighted one and a bar on the right when they do not all fit. `↑↓` move, `Tab` completes, `Enter` runs the highlighted one.

| Command | What it does |
|---|---|
| `/model` | switch model: opens the list to pick one |
| `/provider [id]` | provider status, or set the default provider |
| `/new` | new conversation |
| `/clear` | clear the view without closing the conversation |
| `/system [text]` | show or set the system prompt |
| `/params key=value …` | generation parameters: `temperature`, `num_ctx`, `top_p`, `max_tokens`, `think`, `stop` |
| `/sessions` | resume a saved conversation (also `Ctrl+S`); `Ctrl+R` renames, `Ctrl+D` deletes |
| `/save [name]` | save the conversation and, optionally, rename it |
| `/export [path.md]` | export the conversation as Markdown |
| `/copy` | copy the last reply to the clipboard |
| `/retry` | regenerate the last reply |
| `/undo` | remove the last question/reply pair |
| `/files` | attached files: see what they cost, detach them and add more (also `Ctrl+F`) |
| `/context` | what the model sees: context file, attached files, token budget, machine |
| `/help` | commands and keys, in a scrollable panel |
| `/quit` | quit |

### Keys

| Key | Action |
|---|---|
| `Enter` | send |
| `Ctrl+J` · `Alt+Enter` · `Shift+Enter` | newline (`Shift+Enter` only with the kitty keyboard protocol) |
| `Esc` | cancel the generation · close the panel · clear the selection |
| `Ctrl+C` | cancel; twice with an empty input, quit |
| `Ctrl+D` · `Del` | quit if the input is empty · in the sessions panel, delete the highlighted session |
| `Ctrl+R` | in the sessions panel, rename the highlighted session |
| `1`…`9` · `12` · `Alt+1`…`Alt+9` | in a list, go to the row with that number and open it; past the ninth it takes two digits, or `Enter` to settle for the row you are on; while you are typing a filter the digits belong to it, `Alt` always jumps |
| `Ctrl+P` | model panel |
| `Ctrl+S` | sessions panel |
| `Ctrl+F` | files panel: what is attached, what it costs, and the tree to attach more |
| `↑` · `↓` | prompt history (on the first / last line of the input) |
| `PgUp` · `PgDn` · `Ctrl+↑` · `Ctrl+↓` | scroll the conversation |
| `Ctrl+End` · `Ctrl+Home` | jump to bottom (and follow the reply) · to top |
| `Tab` | complete a command, or a path after `@` |
| `Ctrl+W` · `Ctrl+U` · `Ctrl+K` | delete word · to line start · to line end |
| `Ctrl+A` · `Ctrl+E` | start · end of line |

The mouse works too: the wheel scrolls, dragging over the conversation selects text and copies it when you let go, and the «↓ Jump to bottom» pill is clickable. In the input box a click moves the cursor and a drag selects what you are writing — it is copied when you let go, and the next key drops the highlight without touching the text. To select text with your terminal instead, hold `Shift` while dragging, or set `mouse = false`.

### Files in the context

The model only sees what you give it.

- **`@path`** in a message attaches that file as it is right now. `@path:40-120` attaches a line range. `Tab` completes paths after `@`.
- **`Ctrl+F`** (or `/files`) opens the files panel: what is attached and what each file costs, `Enter` to detach one, and a button that walks the project tree to attach more. Attachments stay for the whole conversation and are re-read on every send; how many there are is shown on the right of the status row.
- **`MOON.md`** in the directory you start from is loaded into the system prompt: what the model should know about the project without being told every time. The file name is configurable (`context_file`).
- **Budget.** If the attachments would exceed 80 % of the model's context window, moon warns and does not send. `/context` shows every file with its token estimate and the size of the next request.
- **Secrets.** Binaries and files that look like credentials (`.env`, private keys) are refused. `@!path` forces one through.

### Sessions

Conversations are saved automatically as JSONL, one file each, and titled after the first message.

```sh
moon --resume            # continue the last conversation
moon --resume <id>       # or a specific one
moon sessions list
```

Inside the TUI, `Ctrl+S` opens the panel: `All` holds every session sorted by title, ignoring case, and from ten sessions on a `Recent` section on top holds the five you last opened or wrote to, in that order. Below ten the list is in view whole and `Recent` would only repeat it, so it is not drawn. No dates on screen, only the title and the model it ran on; `moon sessions list` still prints them with their date. `Enter` resumes, `Ctrl+R` renames, `Ctrl+D` deletes, each in the same panel. Deleting the conversation you are in is allowed: the file goes and what is on screen simply stops being saved, so the next message starts a new session. `/save name` renames the current conversation; `/export notes.md` writes it as Markdown. Set `save_sessions = false` to keep nothing.

### The CLI

Everything that does not need a screen:

```sh
moon ask "explain the borrow checker"          # reply streams to stdout
git diff | moon ask --system "review this"     # prompt from stdin
moon ask --stats "…"                           # token counts and speed, on stderr
moon ask "summarize @README.md"                # the same @path mentions as the TUI
```

The `@path` mentions are expanded in the prompt you type, not in what comes
down a pipe: piped text is content, and a diff or a log is full of `@@` and
`@Annotation` tokens that are not paths.

```sh
moon -m ollama/qwen2.5-coder:14b               # start with this model
moon --config ./moon.toml                      # another configuration file
moon models                                    # models of every provider
moon providers                                 # provider status
moon config init | path | show
moon update                                    # update to the latest release
moon update --check                            # is there a new version?
```

A model is `provider/model`, or just `model` when the name is unique across providers.

## Configuration

Everything is optional. Without a file, moon talks to Ollama at `http://localhost:11434` and uses the first model it finds. Precedence: CLI flags, then environment variables, then `config.toml`, then defaults.

### Where things live

| What | Where |
|---|---|
| Configuration | `~/.config/moon/config.toml` |
| Sessions | `~/.local/share/moon/sessions/` |
| Log, recent models, recent sessions and the last update check | `~/.local/state/moon/` |

The same XDG layout on macOS and Linux; `XDG_CONFIG_HOME`, `XDG_DATA_HOME` and `XDG_STATE_HOME` are honored. On Windows the same folders hang from `%USERPROFILE%`, so the configuration is `%USERPROFILE%\.config\moon\config.toml`.

### Reference

```toml
[general]
default_model    = "ollama/qwen2.5-coder:14b"  # model at startup, provider first
system_prompt    = "Answer briefly."
mouse            = true       # wheel scrolls and drag selects; false leaves selection to the terminal
save_sessions    = true
context_file     = "MOON.md"  # project context file; "" for none
max_attachment_bytes = 200000 # bigger @files are truncated
system_stats     = true       # cpu and ram at the bottom right
update_check     = false      # ask github once a day for a newer moon and say so at startup

[params]                      # defaults for every model; /params overrides them per session
num_ctx     = 16384           # the window Ollama loads the model with
temperature = 0.7
# top_p, max_tokens, stop, think; anything else is passed to the provider as is

[providers.ollama]
type       = "ollama"
base_url   = "http://localhost:11434"   # or the OLLAMA_HOST variable
think      = false                      # ask for reasoning from models that support it
keep_alive = "5m"                       # how long Ollama keeps the model loaded

[theme.overrides]             # any of the ten Moon tokens, as hex
moon      = "#8fb8ff"
ink-muted = "#a9a7b8"
```

Every `[providers.<id>]` block takes `type` (`ollama` or `openai`), `base_url`, `enabled` (default `true`), `timeout_secs` (default `15`) and, for keyed services, `api_key_env`: the **name** of the environment variable that holds the key, never the key itself.

### Providers

Ollama is built in. Anything that speaks the OpenAI chat completions API is an `openai` provider:

```toml
[providers.lmstudio]
type     = "openai"
base_url = "http://localhost:1234/v1"

[providers.llamacpp]
type     = "openai"
base_url = "http://localhost:8080/v1"

[providers.vllm]
type     = "openai"
base_url = "http://localhost:8000/v1"

[providers.openrouter]
type        = "openai"
base_url    = "https://openrouter.ai/api/v1"
api_key_env = "OPENROUTER_API_KEY"

[providers.old]
type     = "openai"
base_url = "http://10.0.0.5:1234/v1"
enabled  = false                      # kept in the file, ignored at runtime
```

`moon providers` shows which ones answered and their version. A provider that cannot be built (a bad URL, a missing key) is disabled with the reason, and moon still starts. `/provider <id>` sets the default provider for bare model names.

### Theme

moon draws with ten tokens from the Moon design system and one brand color. It uses truecolor when the terminal announces it (`COLORTERM=truecolor` or `24bit`) and the nearest 256-color palette otherwise.

| Token | Default | Used for |
|---|---|---|
| `night` | `#1c1c29` | the darkest surface of the palette; moon leaves the terminal's own background showing |
| `night-raised` | `#2a2a3c` | code blocks, the command suggestions |
| `night-line` | `#4a4a60` | separators, borders at rest |
| `moon` | `#8fb8ff` | the crescent, the prompt, the cursor, headings, command names |
| `moon-soft` | `#c7dbff` | focus, small accents, the machine percentages |
| `ink` | `#f3ece3` | the model's text and your input |
| `ink-muted` | `#a9a7b8` | your messages, hints, everything secondary |
| `on-moon` | `#1c1c29` | text on a `moon` fill |
| `ok` | `#8fd9a0` | `✓ done` |
| `alert` | `#ff8f8f` | `✗ cancelled` |

moon never paints the background: your terminal's own shows through, and the tokens are picked to sit on top of it.

### Logging

moon writes to `~/.local/state/moon/moon.log`, never to the screen. `RUST_LOG=debug moon` for more detail, including every request to the providers.

## Project context: MOON.md

Drop a `MOON.md` in a project and start moon there. Its content goes into the system prompt of every request, so the model knows the layout, the conventions and the vocabulary of the project without being told. This repository's own [`MOON.md`](MOON.md) is a small example. The files panel (`Ctrl+F`) lists it under `Project`, and `/context` shows its size in tokens.

## FAQ

**`Shift+Enter` sends instead of inserting a newline.** Terminals only distinguish `Shift+Enter` from `Enter` with the kitty keyboard protocol (kitty, WezTerm, Ghostty, foot). Everywhere else use `Ctrl+J` or `Alt+Enter`.

**The colors look flat.** Your terminal did not announce truecolor, so moon falls back to the 256-color palette. Set `COLORTERM=truecolor` if the terminal really supports it. Terminal.app on macOS does not.

**The wheel changes my prompt instead of scrolling.** That happens with `mouse = false`: in the alternate screen the terminal turns wheel events into arrow keys, and arrows walk the prompt history. Leave mouse capture on and hold `Shift` when you want the terminal's own text selection.

**Does moon edit files or run commands?** No. It reads what you attach and nothing else. Tool use is deliberately out of scope for now.

**Where does the model size come from?** From Ollama's `/api/ps`: the loaded footprint, how much of it sits in the GPU, the context length it was loaded with and when it will be unloaded. Other providers do not expose this, so the row shows only the machine readings with them.

**Which Ollama version do I need?** Any recent one. moon is developed against 0.34.

## Development

```sh
make check              # fmt --check + clippy -D warnings + tests, what CI runs
make run ARGS="ask hi"  # run from source
make build              # release binary in target/release/moon
make package            # dist/moon-<os>-<arch>.tar.gz for this machine
make help               # everything else: cross-compiling, versioning, releasing
```

A Cargo workspace of five crates:

| Crate | Role |
|---|---|
| `moon-core` | the `Provider` trait, domain types, configuration, XDG paths, JSONL sessions, files in the context |
| `moon-provider-ollama` | Ollama over its native API |
| `moon-provider-openai` | any OpenAI-compatible chat completions API |
| `moon-tui` | the interface: `app/` is the state, Elm style, one module per concern; `view/` paints it |
| `moon-cli` | the `moon` binary |

Adding a provider is a new crate that implements `Provider` and `ProviderFactory`, plus one line in `crates/cli/src/main.rs`. Rendering is tested on ratatui's `TestBackend`, providers on `wiremock`. See [CONTRIBUTING.md](CONTRIBUTING.md) for conventions and the release flow.

## Contributing

Bug reports, provider integrations and rough edges made smooth are all welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md), open a pull request against `dev`, and make sure `make check` is green.

## License

[MIT](LICENSE)

<p align="center">
  <br>
  <img src="assets/moon-mark.svg" width="48" alt="" style="image-rendering: pixelated">
  <br>
  <sub>Built with the Moon design system: one brand color, a pixel crescent, square corners.</sub>
</p>
