# The interface

## On screen

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
- `cpu` and `ram` are sampled every 5 seconds, and twice a second while the model is thinking or answering or the `/machine` panel is open. After `▲`, the peak of the last 3 minutes. `swap 1.2G` appears only when swap is in use.
- On a machine with a graphics card of its own, `gpu 78% ▲90` follows: its memory, read through NVML on NVIDIA (the library behind `nvidia-smi`, if it is installed) and through the `amdgpu` driver on AMD. On a Mac there is no such line: the GPU shares the RAM, which is already there.
- On Linux, `ram` counts the loaded model in. The kernel files a model mapped from disk under cache and leaves it out of what it calls used, so `free` and `htop` can show a machine half empty with a 20 GB model on it; moon adds what the model keeps in RAM (from Ollama's `/api/ps`) so the number means what you expect. macOS already counts it.
- Below 120 columns the peaks and the model size go; below 90, the whole block. `system_stats = false` turns the machine readings off.

`/context` prints the same in gigabytes, with the split between weights and context cache and how long until Ollama unloads the model.

`/machine` opens the same readings as a drawing: the panel splits in columns, cpu then ram, and gpu as a third one on a machine with a card, each filled in braille — 2×4 dots per cell — from the curve down. Under the ram curve, in grey, the share of it that is the loaded model, so you see how much room is the model's and how much is everything else. The window is the one the sampler keeps, so the plot fills from the right as samples pile up; under it go the totals, swap and what the loaded model takes. `Esc` closes it.

## Files in the context

The model only sees what you give it.

- **`@path`** in a message attaches that file as it is right now. `@path:40-120` attaches a line range. `Tab` completes paths after `@`.
- **`Ctrl+F`** (or `/files`) opens the files panel: what is attached and what each file costs, `Enter` to detach one, and a button that walks the project tree to attach more. Attachments stay for the whole conversation and are re-read on every send; how many there are is shown on the right of the status row.
- **`MOON.md`** in the directory you start from is loaded into the system prompt: what the model should know about the project without being told every time. The file name is configurable (`context_file`).
- **Budget.** If the attachments would exceed 80 % of the model's context window, moon warns and does not send. `/context` shows every file with its token estimate and the size of the next request.
- **Secrets.** Binaries and files that look like credentials (`.env`, private keys) are refused. `@!path` forces one through.

## Sessions

Conversations are saved automatically as JSONL, one file each, and titled after the first message.

```sh
moon --resume            # continue the last conversation
moon --resume <id>       # or a specific one
moon sessions list
```

Inside the TUI, `Ctrl+S` opens the panel: `All` holds every session sorted by title, ignoring case, and from ten sessions on a `Recent` section on top holds the five you last opened or wrote to, in that order. Below ten the list is in view whole and `Recent` would only repeat it, so it is not drawn. No dates on screen, only the title and the model it ran on; `moon sessions list` still prints them with their date. `Enter` resumes, `Ctrl+R` renames, `Ctrl+D` deletes, each in the same panel. Deleting the conversation you are in is allowed: the file goes and what is on screen simply stops being saved, so the next message starts a new session. `/save name` renames the current conversation; `/export notes.md` writes it as Markdown. Set `save_sessions = false` to keep nothing.

## The CLI

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

While it waits, `ask` turns the same star the TUI uses, with `thinking…` or
`loading model…` next to it and the seconds gone by — a local model that is not
in memory yet can take ten of them before the first token. It goes to stderr,
and only when stderr is a terminal, so `moon ask … > file` and `… | grep` get
the reply and nothing else; it is erased before the first token is printed, and
`NO_COLOR` drops the colour.

```sh
moon -m ollama/qwen2.5-coder:14b               # start with this model
moon --config ./moon.toml                      # another configuration file
moon models                                    # models of every provider
moon providers                                 # provider status
moon config init | path | show
moon update                                    # update to the latest release
moon update --check                            # is there a new version?
```
