# Letting the model act

Out of the box, with every permission off, moon is a chat client: the model
only reads what you attach. Permissions change that. They list everything the
model could do on your project — read and edit files, and run a fixed list of
commands — and each one is set to one of three words:

| | |
|---|---|
| **off** | not offered to the model at all |
| **ask** | shown to you first — the diff of an edit, the line of a command — and nothing happens until you say yes |
| **allow** | runs on its own |

Permissions belong to an **agent**, and an agent is a file: choosing the
agent is choosing them. Every conversation starts with `default`, whose file
`moon config init` writes with reading on and everything else off. The design
behind the loop and the sandbox is in [harness.md](harness.md), the one
behind agents in [agents.md](agents.md).

## The permissions panel

`/agent` opens the list of agents; `ctrl+t` on one opens its permissions.
`/tools` still works as a shortcut to the permissions of the agent the
conversation is using.

Two levels. First the groups, one row each with a summary of what the agent
may do in it; `enter` opens one, `←`/`→` turn the whole of it off, or on with
its defaults. Inside, one row per capability:

```
 Permissions · committer › Git                    agents/committer.toml
 the repository: what only looks allows by default, what changes it asks

   git status   ◀  off  ▶  what is changed, staged and untracked
 ❯ git diff     ◀ allow ▶  the changes not yet committed
   git commit   ◀  ask  ▶  commit what is staged

 ↑↓ move · enter on/off · ←→ off · ask · allow · esc back to the groups
```

- `↑↓` move, `←→` step through `off · ask · allow`, and `enter` or space
  toggle between off and the row's default — `allow` for what only looks,
  `ask` for what changes things.
- **Every step is saved at once**, to the agent's file. There is nothing to
  save or cancel.
- `max steps per message` is the last row of `Editor`: how many rounds of tool
  calls the model may make for one message (8 by default, 1 to 20). A digit
  sets it, `←`/`→` walk it.
- The first permission turns tools on; clearing the last one turns them off
  and says so.
- A program that is not on this machine shows `not installed`, is dimmed and
  stays off.

## What there is

Six groups. **Editor** is moon's own, not programs of the system: `read files`
is not `cat` (it hands the model the file the way `@path` does, with line
ranges and the hash an edit must match), `edit existing files` has no command
it could be (a change you see before it lands), and `create new files` is not
`mkdir` (a file with its content, shown first). **Files** holds the programs
that work on files. **Git** and **Network** are what their names say.
**Stack** is each language's tools, in sections: Make, Rust, Node and
Python. **Docker** is the project's Compose — never `docker run` or
`docker exec` on their own, which reach the whole machine.

**Editor** also holds two settings on how the model's calls run, whatever
their group:

- **`commands in subfolders`**, `off` or `allow`. Off, every command runs in
  the directory you started moon in. Allow, the model may ask for one to run
  inside a folder *below* it — `frontend/`, `crates/agent/` — never above:
  `..`, `/tmp`, `~` or a symlink that leads out are refused whatever this
  says, the same way a path is. There is no `ask`: a command that asks
  already shows its folder.
- **`max steps per message`**, described above. It is a number, not a
  permission, so turning `Editor` off leaves it as it is.

This is the whole catalogue; nothing outside it can be run. The tables are
generated from the code (`UPDATE_DOCS=1 cargo test -p moon-agent docs`), so
they are what moon has.

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

**Stack** — each language's tools: they run the project's own code, so most ask by default

*Make*

| Name | Default when turned on | What it does | Refused flags |
|---|---|---|---|
| `make` | `ask` | run a Makefile target | `--eval` `-E` |

*Rust*

| Name | Default when turned on | What it does | Refused flags |
|---|---|---|---|
| `cargo check` | `allow` | compile without building | `--config` `-Z` |
| `cargo clippy` | `allow` | the lints | `--config` `-Z` |
| `cargo build` | `allow` | build | `--config` `-Z` |
| `cargo test` | `ask` | run the tests | `--config` `-Z` |
| `cargo fmt` | `ask` | format the code in place | `--config` `-Z` |

*Node*

| Name | Default when turned on | What it does | Refused flags |
|---|---|---|---|
| `npm run` | `ask` | run a package.json script | — |
| `npm test` | `ask` | run the tests | — |
| `npm install` | `ask` | install dependencies; reaches the network and runs their scripts | — |
| `npm ci` | `ask` | a clean install from the lockfile | — |
| `npm ls` | `allow` | the dependency tree | — |
| `pnpm run` | `ask` | run a package.json script | — |
| `pnpm test` | `ask` | run the tests | — |
| `pnpm install` | `ask` | install dependencies; reaches the network and runs their scripts | — |
| `pnpm list` | `allow` | the dependency tree | — |

*Python*

| Name | Default when turned on | What it does | Refused flags |
|---|---|---|---|
| `pytest` | `ask` | run the tests | — |
| `python3` | `ask` | run a Python script of the project | — |
| `pip install` | `ask` | install packages; reaches the network | — |
| `pip list` | `allow` | the installed packages | — |

**Docker** — the project's Compose: what only looks allows, what starts, stops or runs inside asks

| Name | Default when turned on | What it does | Refused flags |
|---|---|---|---|
| `docker ps` | `allow` | the running containers | `-H` `--host` `--context` `--config` |
| `docker compose ps` | `allow` | the services of the compose | `-H` `--host` `--context` `--config` |
| `docker compose logs` | `allow` | a service's logs, without following them | `-H` `--host` `--context` `--config` `-f` `--follow` |
| `docker compose config` | `allow` | the compose as it resolves | `-H` `--host` `--context` `--config` |
| `docker compose build` | `ask` | build the images | `-H` `--host` `--context` `--config` |
| `docker compose up` | `ask` | start the services, detached: `-d` is required | `-H` `--host` `--context` `--config` |
| `docker compose down` | `ask` | stop the services | `-H` `--host` `--context` `--config` |
| `docker compose restart` | `ask` | restart a service | `-H` `--host` `--context` `--config` |
| `docker compose exec` | `ask` | run a command inside a service | `-H` `--host` `--context` `--config` |

**Network** — what a command fetches goes to the model; flags that would send a file are refused

| Name | Default when turned on | What it does | Refused flags |
|---|---|---|---|
| `curl` | `ask` | fetch a URL; what it gets goes to the model | `-d` `--data` `--data-ascii` `--data-binary` `--data-raw` `--data-urlencode` `--json` `-F` `--form` `--form-string` `-T` `--upload-file` `-K` `--config` |
| `wget` | `ask` | download a URL into the project | `--post-file` `--body-file` `-i` `--input-file` `--config` `-e` `--execute` |

<!-- CATALOG:END -->

On Windows the file commands are looked for next to Git for Windows, whose
`ls`, `cat` and `grep` are the real ones — never the system's own `find`.

## The rules

Three, and only three.

1. **Editing and creating files ask by default**, and `allow` on them is yours
   to set: then the model writes without showing you the diff, you see
   `⎿  ✎ edit  a.rs  +3 −1  applied` after the fact, and git is the only way
   back. moon warns when you set it in a directory that is not a git
   repository.
2. **Editing and creating need reading**, so turning either on turns
   `read files` on, and turning `read files` off turns them off. Nothing else
   is coupled: a command is a choice of its own, and `make` at `allow` is
   yours to set.
3. **With everything off, tools are off.** There is no separate switch.

## Approving

What is set to `ask` opens a panel under the conversation, in the input box's
place:

```
 Edit file                                                             +1 −1
 crates/tui/src/app/chat.rs

   280 -        self.push_item(Item::Info("(empty reply)".into()));
   280 +        self.push_item(Item::Note("(empty reply)".into()));
 ╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌
 Do you want to make this edit to chat.rs?
 ❯ 1. Yes
   2. No
```

A command shows `Run command`, the line it would run and, when it is not the
project root, the folder.

| Key | |
|---|---|
| `1` · `y` | yes: apply the edit, run the command |
| `2` · `n` | no: skip it and tell the model so |
| `↑↓` · `j k` · `enter` | choose, then confirm |
| `pgup` · `pgdn` · `space` · `home` · `end` | scroll a long diff |
| `esc` · `ctrl+c` | cancel the whole turn; nothing pending is written, and a running command is killed |

## Agents

An agent is a prompt and its permissions, whole, and every agent is one TOML
file in `agents/`, next to `config.toml`, the file name being the agent's
name. `moon config init` writes two:

- **`default.toml`** — what every conversation starts with: reading on,
  nothing else, and moon's own prompt. If the file is missing moon writes it
  again at startup; it cannot be renamed or deleted from `/agent`.
- **`reviewer.toml`** — reads the project and runs the read-only git commands
  (`status`, `diff`, `log`, `show`, `blame`), reports, and never writes. A
  sample to start from: edit it, rename it, delete it.

Any other file in the folder is an agent of yours:

```toml
# ~/.config/moon/agents/committer.toml
description = "stages and commits what you approve; edits nothing"
max_steps   = 6

prompt = """
You prepare commits: read what changed, stage it, and write a Conventional
Commit message. You never edit files; when a change is needed, say so and stop.
"""

[permissions]
"read files" = "allow"
"git diff"   = "allow"
"git add"    = "ask"
"git commit" = "ask"
```

Everything is optional. Without a `prompt` the agent gets moon's own — the
editor's when it may write files, the reader's otherwise. `[permissions]` is
the whole truth: what it lists is what the agent may do, and what it does not
list is off. A file that does not parse is reported and skipped, and never
rewritten.

You do not have to write the file by hand. In the `/agent` picker:

| Key | |
|---|---|
| `enter` | use this agent for the conversation |
| `ctrl+a` | a new agent: name it, and moon writes a template and opens its permissions |
| `ctrl+t` | its permissions |
| `ctrl+e` | its description and prompt, edited in moon: `tab` switches field, `esc` saves on the way out, `ctrl+c` discards |
| `ctrl+r` | rename it; a conversation that uses it follows |
| `ctrl+d` | delete it, after a confirmation |

The chosen agent is named on the bottom row, is saved with the session, and
brings its own permissions and step limit with it. The full picture — the
prompt it gets, the reasons, what was tried — is in [agents.md](agents.md).

**An agent file grants permissions**, so the agents folder carries the same
trust as `config.toml`: only ever your own configuration. Agents shipped
inside a project are not supported, and if they ever are, they will not load
without an explicit confirmation.

## The files

`moon config path` says where the folder is. A file moon writes lists every
capability there is, in its group, `off` included, with what it does as a
comment — so the file is also the list of what the agent could be allowed:

```toml
description = "the agent every conversation starts with"
max_steps   = 8

# no prompt: moon's own, the editor's when it may write files, the reader's otherwise

[permissions]
# Editor — moon's own tools, and how the model's calls run: files read and changed with a diff, commands in subfolders, the step limit
"read files"            = "allow"  # open and list files under this directory
"edit existing files"   = "off"    # a diff you apply or skip; allow writes it without showing you
"create new files"      = "off"    # a new file you apply or skip; allow writes it without showing you

"commands in subfolders" = "allow"  # in a folder below this one, never above it

# Files — programs that work on files, run as they are
"ls"                    = "allow"  # list a folder
"cat"                   = "allow"  # print a file
…
```

It works both ways:

- **Panel → file.** Every step in an agent's panel rewrites its file.
- **File → moon.** moon reads the folder at startup, when `/agent` opens and
  before every message. Edit a file by hand while moon runs and the next
  message uses it; the status row says `agents/default.toml changed ·
  reloaded`.

**New versions.** When a moon update adds capabilities, every file is brought
up to the catalogue on the next start: what is missing is added as `off`,
what the catalogue no longer knows is dropped, what you set is kept, and moon
says so once (`agents/committer.toml brought up to the catalogue · 3 new`).
A file written by hand with only what is on gets the same treatment: it ends
up listing everything. There is nothing to run.

**Starting over.** `moon config reset` writes `config.toml`, `default.toml`
and `reviewer.toml` again as they come from the factory, after showing what
it will overwrite and asking; `--yes` skips the question. Your other agents
and your sessions stay. `moon config init` only writes what is missing.

Three things to know. moon rewrites a file whole, so comments of your own do
not survive it. A line that names nothing moon knows is dropped, while a
value that is not one of the three is an error: the file is reported at the
next read, left as it is, and the agent runs with nothing on until it is
fixed. And older `enabled`, `edit`, `create` and `permissions` keys under
`[tools]` in `config.toml`, or a `tools.toml` from before agents carried
their permissions, are read once — the first time `default.toml` is written,
which starts from them — and never again.

## What keeps it safe

- **Only forward, never back.** Paths are relative to the directory you
  started moon in and must stay under it: no `..`, no `~`, no absolute path
  outside it (one inside it, pasted from your editor, works), no symlink that
  leads outside. `.git/`, the files the secrets filter refuses (`.env`, keys)
  and the globs in `[tools] deny` are never touched. Every write goes through
  the same check, before and after touching the disk.
- **No shell.** A command starts as a program with its arguments as a list:
  no pipes, no redirections, no `&&`. Its arguments go through the same
  lexical check as a path, the value of a `--flag=value` included, may not
  name a file the secrets filter refuses, and the flags in the last column of
  the tables above are refused whatever comes with them.
- **With your ok.** Nothing set to `ask` happens until you say yes.
- **Limits.** The step limit per message (8 unless you change it), ten calls
  per reply and three refused paths per turn; then the turn stops and says
  why. An edit needs the file read first in the conversation, and fails if the
  file changed since. A command is killed after 60 seconds, and the model gets
  at most 20 kB of what it printed.

What it cannot protect you from:

- The model can write a `Makefile`, a `build.rs` or a git hook that you will
  run later — or that it will, if `make` or `cargo test` is on. That is why
  edits and those commands ask by default, and why moon warns when there is no
  git repository to undo with.
- What a command prints is what the model reads: `grep -r` over a folder with a
  `.env` in it shows the model the `.env`, since the secrets filter applies to
  the paths named in the arguments, not to what the program opens on its own.
- A model that has read a file can put what it read in the URL it asks `curl`
  to fetch. The `ask` shows you the line.
- `cargo build`, `check` and `clippy` allow by default, and a `build.rs` runs
  code when they do. Set them to `ask` if that matters to you.
- Killing a command does not kill what it started (`make` running `cargo`).
- Slow commands hit the 60-second limit: `docker compose build`, `npm ci` or
  `pnpm install` on a cold cache may be killed before they finish.

## What you see

- The agent and what it may do at the left of the bottom row:
  `⏵⏵ committer · Read · 4 commands`, or `⏵ default · all off`.
- One line per tool call in the conversation, hanging from the message:
  `⎿  read  src/a.rs`, `⎿  ✎ edit  src/a.rs  +3 −1  applied`,
  `⎿  run   git diff --stat`, and `✗` with the reason when one fails.
- `waiting for your approval · 1 yes · 2 no · esc cancel the turn` in the
  status row while an approval is open, `running git diff --stat (2s)` while a
  command runs.
- `/context` lists the tools on offer and the root, the agent when it is not
  `default`, and what is on, by permission.

## Models

The model needs tool support. With Ollama, `ollama show <model>` lists `tools`
under capabilities when it has it (`qwen2.5-coder`, `llama3.1`, `qwen3`,
`mistral-nemo` do); a model without it answers the request with an error.
Some of them, `qwen2.5-coder` among them, write the call as JSON in the reply
instead of a proper tool call; moon recognizes a reply that is nothing but a
call and runs it all the same.
