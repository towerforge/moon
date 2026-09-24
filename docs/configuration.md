# Configuration

Everything is optional. Without a file, moon talks to Ollama at `http://localhost:11434` and uses the first model it finds. Precedence: CLI flags, then environment variables, then `config.toml`, then defaults.

```sh
moon config init     # writes config.toml and tools.toml, commented; --force rewrites both
moon config path     # where they are, and the sessions and the log
moon config show     # the configuration in effect
```

## Where things live

| What | Where |
|---|---|
| Configuration | `~/.config/moon/config.toml` |
| What the model may do (`/tools`) | `~/.config/moon/tools.toml` — see [tools.md](tools.md) |
| Sessions | `~/.local/share/moon/sessions/` |
| Log, recent models, recent sessions and the last update check | `~/.local/state/moon/` |

The same XDG layout on macOS and Linux; `XDG_CONFIG_HOME`, `XDG_DATA_HOME` and `XDG_STATE_HOME` are honored. On Windows the same folders hang from `%USERPROFILE%`, so the configuration is `%USERPROFILE%\.config\moon\config.toml`. `moon --config ./other.toml` uses another file, and looks for `tools.toml` next to it.

## Reference

```toml
[general]
default_model    = "ollama/qwen2.5-coder:14b"  # model at startup, provider first
system_prompt    = "Answer briefly."
mouse            = true       # wheel scrolls and drag selects; false leaves selection to the terminal
save_sessions    = true
context_file     = "MOON.md"  # project context file; "" for none
max_attachment_bytes = 200000 # bigger @files are truncated
system_stats     = true       # cpu, ram and gpu at the bottom right
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

[tools]                       # what the model may do is in tools.toml; these two are limits
max_file_bytes = 200000       # bigger files are neither read nor edited
deny           = [".github/workflows/**"]   # never touched, on top of .git/ and the secrets filter

[theme.overrides]             # any of the ten Moon tokens, as hex
moon      = "#8fb8ff"
ink-muted = "#a9a7b8"
```

Every `[providers.<id>]` block takes `type` (`ollama` or `openai`), `base_url`, `enabled` (default `true`), `timeout_secs` (default `15`) and, for keyed services, `api_key_env`: the **name** of the environment variable that holds the key, never the key itself.

Older files may have `enabled`, `edit` and `create` under `[tools]`, or a `[tools.permissions]` table. They still work while there is no `tools.toml`; once there is one, it wins.

## Providers

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

`moon providers` shows which ones answered and their version. A provider that cannot be built (a bad URL, a missing key) is disabled with the reason, and moon still starts. `/provider <id>` sets the default provider for bare model names. A model is `provider/model`, or just `model` when the name is unique across providers.

## Theme

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

## Logging

moon writes to `~/.local/state/moon/moon.log`, never to the screen. `RUST_LOG=debug moon` for more detail, including every request to the providers.
