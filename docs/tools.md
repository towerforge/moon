# Letting the model act

With no configuration file at all moon is a chat client: the model only reads what you attach. `/tools` changes that. It lists everything the model could do on your project — reading and editing files, and running a fixed list of commands — and for each thing you pick one of three words:

| | |
|---|---|
| **off** | not offered to the model at all |
| **ask** | shown to you first — the diff of an edit, the line of a command — and nothing happens until you confirm; `s` skips it and tells the model so |
| **allow** | runs on its own |

`moon config init` writes a `tools.toml` with reading on and everything else off. The design behind all of this is in [harness.md](harness.md).

## The panel

`/tools` has two levels. The first is the overview: one row per group with what is on in it, and the step limit under them.

```
 What may the model do?                                        ~/Towerforge/moon
 off: not offered · ask: shown and waits for you · allow: runs on its own

 Only under this directory, and never through a shell. Enter opens a group;
 ← turns the whole of it off, → turns it on with its defaults.

 ❯ Editor   ▸  allow: read files, commands in subfolders · ask: edit existing files · 8 steps
   Files    ▸  allow: ls, cat
   Git      ▸  allow: git status, git diff · ask: git commit
   Build    ▸  off
   Network  ▸  off

 ↑↓ move · enter open · ←→ group off/on · esc save
```

Inside a group, one row per thing it has, with the selector to the right:

```
 What may the model do? › Editor                               ~/Towerforge/moon
 moon's own tools, and how the model's calls run: files read and changed with a …

 ❯ read files              ◀ allow ▶  open and list files under this directory
   edit existing files     ◀  ask  ▶  a diff you apply or skip; allow writes it …
   create new files        ◀  off  ▶  a new file you apply or skip; allow writes …
   commands in subfolders  ◀ allow ▶  in a folder below this one, never above it
   max steps per message   ◀   8   ▶  tool calls one message may take

 ↑↓ move · enter on/off · ←→ off · ask · allow · esc save & back
```

| Where | Key | Does |
|---|---|---|
| groups | `Enter` | open the group |
| groups | `←` · `→` | the whole group off · on with its defaults |
| a group | `←` · `→` | walk `off · ask · allow` |
| a group | `Enter` · `Space` | between off and what the row is for: `allow` for what only looks, `ask` for what changes things |
| `Editor` | `←` · `→` on `commands in subfolders` | off or allow: there is nothing to ask |
| `Editor` | `←` · `→` on the last row | the step limit, 1 to 20 |
| both | `Esc` | save; inside a group it also steps back to the groups, from the groups it also closes |

There is no cancel: `Esc` always saves.

## What there is

Five groups. **Editor** is moon's own, not programs of the system: `read files` is not `cat` (it hands the model the file the way `@path` does, with line ranges and the hash an edit must match), `edit existing files` has no command it could be (a change you see before it lands), and `create new files` is not `mkdir` (a file with its content, shown first). **Files** is the programs that work on files. **Git**, **Build** and **Network** are what their names say.

**Editor** also holds two settings on how the model's calls run, whatever their group:

- **`commands in subfolders`**, `off` or `allow`. Off, every command runs in the directory you started moon in. Allow, the model may ask for one to run inside a folder *below* it — `frontend/`, `crates/agent/` — never above: `..`, `/tmp`, `~` or a symlink that leads out are refused whatever this says, the same way a path is. There is no `ask`: a command that asks already shows the folder in its approval.
- **`max steps per message`**, the last row of `Editor`: how many times the model may use a tool for one message before it has to answer. It is a number, so turning `Editor` off leaves it as it is.

This is the whole catalogue; nothing outside it can be run. The tables are generated from the code (`UPDATE_DOCS=1 cargo test -p moon-agent docs`), so they are what moon has.

<!-- CATALOG:START -->

**Editor** — moon's own tools, and how the model's calls run: files read and changed with a diff, commands in subfolders, the step limit

| Name | Default when turned on | What it does | Refused flags |
|---|---|---|---|
| `read files` | `allow` | open and list files under this directory | — |
| `edit existing files` | `ask` | a diff you apply or skip; allow writes it without showing you | — |
| `create new files` | `ask` | a new file you apply or skip; allow writes it without showing you | — |
| `commands in subfolders` | `allow` | in a folder below this one, never above it | — |

**Files** — programs that work on files, run as they are

| Name | Default when turned on | What it does | Refused flags |
|---|---|---|---|
| `ls` | `allow` | list a folder | — |
| `cat` | `allow` | print a file | — |
| `head` | `allow` | the first lines of a file | — |
| `tail` | `allow` | the last lines of a file | — |
| `wc` | `allow` | count lines, words and bytes | — |
| `grep` | `allow` | search text in files | — |
| `find` | `allow` | find files by name | `-exec` `-execdir` `-ok` `-okdir` `-delete` `-fprint` `-fprint0` `-fprintf` `-fls` |
| `tree` | `allow` | the folder tree | `-o` |
| `pwd` | `allow` | the folder the command runs in | — |
| `mkdir` | `ask` | create a folder | — |

**Git** — the repository: what only looks allows by default, what changes it asks

| Name | Default when turned on | What it does | Refused flags |
|---|---|---|---|
| `git status` | `allow` | what is changed, staged and untracked | `--exec-path` `--output` `--upload-pack` `--receive-pack` `--exec` `--config-env` `--git-dir` `--work-tree` |
| `git diff` | `allow` | the changes not yet committed | `--exec-path` `--output` `--upload-pack` `--receive-pack` `--exec` `--config-env` `--git-dir` `--work-tree` |
| `git log` | `allow` | the history | `--exec-path` `--output` `--upload-pack` `--receive-pack` `--exec` `--config-env` `--git-dir` `--work-tree` |
| `git show` | `allow` | one commit, or a file as it was in one | `--exec-path` `--output` `--upload-pack` `--receive-pack` `--exec` `--config-env` `--git-dir` `--work-tree` |
| `git blame` | `allow` | who changed each line of a file | `--exec-path` `--output` `--upload-pack` `--receive-pack` `--exec` `--config-env` `--git-dir` `--work-tree` |
| `git add` | `ask` | stage changes | `--exec-path` `--output` `--upload-pack` `--receive-pack` `--exec` `--config-env` `--git-dir` `--work-tree` |
| `git commit` | `ask` | commit what is staged | `--exec-path` `--output` `--upload-pack` `--receive-pack` `--exec` `--config-env` `--git-dir` `--work-tree` |

**Build** — builds and tests: they run the project's own code, so most ask by default

| Name | Default when turned on | What it does | Refused flags |
|---|---|---|---|
| `make` | `ask` | run a Makefile target | `--eval` `-E` |
| `cargo check` | `allow` | compile without building | `--config` `-Z` |
| `cargo clippy` | `allow` | the lints | `--config` `-Z` |
| `cargo build` | `allow` | build | `--config` `-Z` |
| `cargo test` | `ask` | run the tests | `--config` `-Z` |
| `cargo fmt` | `ask` | format the code in place | `--config` `-Z` |
| `npm run` | `ask` | run a package.json script | — |
| `npm test` | `ask` | run the tests | — |
| `pytest` | `ask` | run the tests | — |

**Network** — what a command fetches goes to the model; flags that would send a file are refused

| Name | Default when turned on | What it does | Refused flags |
|---|---|---|---|
| `curl` | `ask` | fetch a URL; what it gets goes to the model | `-d` `--data` `--data-ascii` `--data-binary` `--data-raw` `--data-urlencode` `--json` `-F` `--form` `--form-string` `-T` `--upload-file` `-K` `--config` |
| `wget` | `ask` | download a URL into the project | `--post-file` `--body-file` `-i` `--input-file` `--config` `-e` `--execute` |

<!-- CATALOG:END -->

A program the machine does not have shows `not installed` and stays off, and a group's defaults leave it out. On Windows the file commands are looked for next to Git for Windows, whose `ls`, `cat` and `grep` are the real ones — never the system's own `find`.

## The rules

Three, and only three.

1. **Editing and creating files ask by default**, and `allow` on them is yours to set: then the model writes without showing you the diff, you see `⎿  ✎ edit  a.rs  +3 −1  applied` after the fact, and git is the only way back. moon says so when you set it in a directory that is not a git repository.
2. **Editing and creating need reading**, so turning either on turns `read files` on, and turning `read files` off turns them off. Nothing else is coupled: a command is a choice of its own, and `make` at `allow` is yours to set.
3. **With everything off, tools are off.**

## `tools.toml`

What the panel sets lives in `tools.toml`, next to `config.toml` (`moon config path` says where). `moon config init` writes it; `moon config init --force` writes it again, as it writes `config.toml`. Every capability is in it, in its group, `off` included, so it is also the list of what there is:

```toml
max_steps = 8

[permissions]
# Editor — moon's own tools, and how the model's calls run: files read and changed with a diff, commands in subfolders, the step limit
"read files"            = "allow"  # open and list files under this directory
"edit existing files"   = "ask"    # a diff you apply or skip; allow writes it without showing you
"create new files"      = "off"    # a new file you apply or skip; allow writes it without showing you

"commands in subfolders" = "allow"  # in a folder below this one, never above it

# Files — programs that work on files, run as they are
"ls"                    = "allow"  # list a folder
"cat"                   = "allow"  # print a file
…
```

It works both ways:

- **Panel → file.** Each `Esc` in the panel rewrites the file: leaving a group, and closing.
- **File → moon.** moon reads it at startup, when the panel opens and before every message. Edit it by hand while moon runs and the next message uses it; the status row says `tools.toml changed · reloaded`.

Two things to know. There is no watching in real time: with the panel open, a change made to the file is not seen until the panel is opened again, and saving from the panel overwrites it. And the panel writes the whole file: comments of your own do not survive it. A line that names nothing moon knows is ignored; a value that is not one of the three is an error, said at the next read, and the last good state stays until it is fixed.

While `tools.toml` exists it wins over `config.toml` and over what a resumed session had. Without it, the older `enabled`, `edit` and `create` keys under `[tools]` in `config.toml` are still honoured.

## What keeps it safe

- **Only forward, never back.** Paths are relative to the directory you started moon in and must stay under it: no `..`, no `~`, no absolute path outside it (one inside it, pasted from your editor, works), no symlink that leads outside. `.git/`, the files the secrets filter refuses (`.env`, keys) and the globs in `deny` are never touched. Every write goes through the same check, before and after touching the disk.
- **No shell.** A command starts as a program with its arguments as a list, from the directory you started in: no pipes, no redirections, no `&&`. Its arguments go through the same lexical check as a path, the value of a `--flag=value` included, may not name a file the secrets filter refuses, and the flags in the last column of the tables above are refused whatever comes with them.
- **With your ok.** What is set to `ask` opens a panel under the conversation — the diff, or the line the command would run — and nothing happens until you press `Enter` on `Apply` or `Run`; `s` skips it; `Esc` cancels the whole turn and kills a command that is running.
- **Limits.** At most eight rounds of tool calls per message (the step limit), ten calls per reply and three refused paths per turn; then the turn stops and says why. An edit needs the file read first in the conversation, and fails if the file changed since. A command is killed after 60 seconds, and the model gets at most 20 kB of what it printed.

What it cannot protect you from:

- The model can write a `Makefile`, a `build.rs` or a git hook that you will run later — or that it will, if `make` or `cargo test` is on. That is why edits and those commands ask by default, and why moon warns when there is no git repository to undo with.
- What a command prints is what the model reads: `grep -r` over a folder with a `.env` in it shows the model the `.env`, since the secrets filter applies to the paths named in the arguments, not to what the program opens on its own.
- A model that has read a file can put what it read in the URL it asks `curl` to fetch. The `ask` shows you the line.
- `cargo build`, `check` and `clippy` allow by default, and a `build.rs` runs code when they do. Set them to `ask` if that matters to you.
- Killing a command does not kill what it started (`make` running `cargo`).

## What you see

- `⏵⏵ Read · Edit · Create · 4 commands` at the left of the bottom row while tools are on.
- One line per tool call in the conversation: `⎿  read  src/a.rs`, `⎿  ✎ edit  src/a.rs  +3 −1  applied`, `⎿  run   git diff --stat`.
- `waiting for your approval` in the status row while an approval is open, `running git diff --stat (2s)` while a command runs.
- `/context` lists what is on, with its permission, and the root.

## Models

The model needs tool support. With Ollama, `ollama show <model>` lists `tools` under capabilities when it has it (`qwen2.5-coder`, `llama3.1`, `qwen3`, `mistral-nemo` do); a model without it answers the request with an error. Some of them, `qwen2.5-coder` among them, write the call as JSON in the reply instead of a proper tool call; moon recognizes a reply that is nothing but a call and runs it all the same.
