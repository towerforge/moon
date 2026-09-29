# The harness: how the model acts on a project

*Design, 2026-09-22 · implemented the same day in `crates/agent`. Amended on
2026-09-24 (permissions and commands) and 2026-09-28 (named agents, see
[agents.md](agents.md)). This document describes the code as it is; the
decisions that changed it are dated at the end.*

With every permission off, moon is a chat client: the model reads what you
attach and nothing else. Permissions let it act — read and edit files under
the directory moon was started in, and run commands from a fixed catalogue —
each one `off`, `ask` or `allow`. Three rules hold whatever is set, and the
code makes them impossible to break rather than merely discouraged:

1. **Never a shell.** Nothing the model sends is ever handed to `sh` or
   `cmd`. `run_command` starts a program from the catalogue with its arguments
   as a list, and that is the whole of it.
2. **Only forward, never back.** Paths are relative to the start-up directory
   and must stay under it: no `..`, no `~`, no absolute path outside the root
   (one inside it, as pasted from an editor, is taken as relative), no symlink
   that leads outside. Enforced in one place, before and after touching the
   disk.
3. **Nothing unseen by default.** Edits, new files and every command that
   changes something ask by default: the diff or the command line is shown and
   nothing happens until you say yes. `allow` lifts that for one capability,
   and only because you set it. There is no `--yolo` and no pre-approval flag.

## Where the code lives

Everything that makes the model act is one crate, `moon-agent`, with four
folders named after the four ideas. It depends on `moon-core` only: no
terminal, no HTTP, no ratatui. The interface drives it; the providers do not
know it exists.

```
crates/
├── core/                     moon-core · ToolSpec, ToolCall, Permission, ToolsFile, the ids of the file capabilities
├── providers/{ollama,openai} · send the tool specs, parse the tool calls
├── agent/                    moon-agent · the model acting on the project
│   └── src/
│       ├── lib.rs            re-exports Harness, Agent, AgentDef, Sandbox, Tool
│       ├── harness/          THE LOOP · a pure state machine, no I/O of its own
│       │   ├── mod.rs        Harness: feed it events, it hands back commands; the limits
│       │   ├── fallback.rs   a call the model wrote as text (small models do) becomes a call
│       │   └── tests.rs      a scripted model, a scripted user; no provider, no terminal
│       ├── agents/           WHO TALKS TO THE MODEL · a prompt and a policy
│       │   ├── mod.rs        Agent (what runs), AgentDef (what is defined), the reserved names
│       │   ├── editor.rs     the editor's and the reader's prompts: `default`'s two faces
│       │   ├── reviewer.rs   the reviewer, as config init writes it
│       │   └── file.rs       one TOML per user agent
│       ├── tools/            WHAT AN AGENT CAN DO · a closed enum; there is no `bash` variant
│       │   ├── mod.rs        Tool, ToolSpec (the JSON schema the model sees), Pending
│       │   ├── catalog.rs    CATALOG, Entry, Category, Policy: every capability and its permission
│       │   ├── read_file.rs  read_file { path, range? } → the <file> block; remembers the hash
│       │   ├── list_dir.rs   list_dir { path? } → entries (skips target/, .git/, node_modules/)
│       │   ├── edit_file.rs  edit_file { path, old_string, new_string, replace_all? } → PendingEdit
│       │   ├── write_file.rs write_file { path, content } → PendingEdit (new file or full replace)
│       │   ├── run_command.rs run_command { command, args?, dir? } → Exec; runs it with no shell
│       │   └── diff.rs       the line diff and its +N −M, computed here, painted by the interface
│       └── sandbox/          THE BOUNDARY · every tool goes through it, nothing goes around it
│           ├── mod.rs        Sandbox { root }: the two checks, the deny rules, CRLF/BOM, atomic writes
│           └── glob.rs       the deny-list globs: `*`, `?`, `**`, never case-sensitive
├── tui/src/
│   ├── app/agent.rs          runs the harness's commands, the approval panel, the /agent picker
│   ├── app/perms.rs          the permissions panel, and where each step is written
│   ├── app/agent_prompt.rs   the prompt editor
│   ├── view/diff.rs          paints a diff with the theme (added in `ok`, removed in `alert`)
│   └── view/{panel,perms,agent_prompt}.rs
└── cli/                      `moon ask` gets no tools: nowhere to approve
```

Dependency direction, so nothing leaks the wrong way:

```
moon-core  ←  moon-agent  ←  moon-tui  ←  moon-cli
moon-core  ←  moon-provider-*  ←  moon-cli
```

`moon-agent` never imports a provider; the providers never import the agent.
The harness is "sans I/O": it decides, the interface acts.

## How a turn runs

A *turn* is everything between you pressing `Enter` and the model's final
answer. Without tools a turn is one request. With tools it is a loop, and the
harness owns it:

```
   you                  moon-tui (app/agent.rs)               moon-agent (harness)             provider
    │  Enter                     │                                    │                           │
    ├───────────────────────────►├── begin_turn, specs, prompt ──────►│                           │
    │                            ├────────────────── request + tools ─────────────────────────────►
    │                            ◄──────────────────────────── Delta / ToolCall / Done ───────────┤
    │                            ├── Event::ModelDone(calls) ────────►│                           │
    │                            │   read_file / list_dir run here, inside the sandbox            │
    │                            ◄── Command::Ask(Pending) ───────────┤  (edit, write, command    │
    │  ◄── approval panel ───────┤                                    │   at `ask`)               │
    │  1 yes / 2 no              │                                    │                           │
    ├───────────────────────────►├── Event::Verdict(Apply) ──────────►│                           │
    │                            ◄── Command::Run(Exec) ──────────────┤  (a command)              │
    │                            │   runs on its own thread, no shell │                           │
    │                            ├── Event::Ran(Output) ─────────────►│                           │
    │                            ◄── Command::Continue(results) ──────┤                           │
    │                            ├────────────────── request + tool results ──────────────────────►
    │                            ◄──────────────────────────── Delta … Done (no calls) ───────────┤
    │                            ├── Event::ModelDone([]) ───────────►│                           │
    │                            ◄── Command::Finished ───────────────┤                           │
```

The harness's surface:

```rust
pub enum Event   { ModelDone(Vec<ToolCall>), Verdict(Verdict), Ran(Output), Cancel }
pub enum Verdict { Apply, Skip }
pub enum Command {
    Continue(Vec<Message>),      // add these tool results and ask the model again
    Ask(Pending),                // show this edit or command and wait
    Run(Exec),                   // run this command off the main thread, feed back what it printed
    Step(Step),                  // a line for the conversation
    Finished,                    // the last reply had no calls
    Stopped { reason: Stop, results: Vec<Message> },
}
pub enum Pending { Edit(PendingEdit), Run(Exec) }

impl Harness {
    pub fn new(agent: Agent, sandbox: Sandbox, limits: Limits) -> Self;
    pub fn set_agent(&mut self, agent: Agent);      // a new policy or agent, between turns
    pub fn begin_turn(&mut self);                   // a new user message
    pub fn feed(&mut self, ev: Event) -> Vec<Command>;
    pub fn specs(&self) -> Vec<ToolSpec>;           // what goes in the request
    pub fn prompt(&self) -> String;                 // what goes in the system prompt
}
```

The harness never builds a request and never runs a process: it hands back
tool results and `Exec`s, and the interface, which owns the conversation and
the system prompt, sends the next request and runs the command. That keeps
the loop free of I/O and testable with a scripted model and a scripted
output, and keeps the interface responsive while `cargo test` runs.

**Limits**, so a small model cannot loop forever:

| Limit | Default | When hit |
|---|---|---|
| Rounds per turn (model → tools → model) | 8, set per agent as `max_steps` (1–20) | `Stopped(TooManyRounds)`; the text so far stays |
| Tool calls per reply | 10 | the rest are answered "too many calls, ask again" |
| Sandbox refusals per turn | 3 | `Stopped(TooManyRejections)` |
| `Esc` | — | `Stopped(Cancelled)`: nothing pending is written, a running command is killed |

Every tool result goes back to the model as a `Role::Tool` message, refusals
included ("`../x` is outside the project"), so a model that can correct itself
does.

**A call written as text.** On Ollama, `qwen2.5-coder:14b` answers a request
that offers tools with `{"name": "write_file", "arguments": {…}}` as plain
content and no `tool_calls`; `qwen3:4b` returns real calls.
`harness/fallback.rs` recognizes a reply that is nothing but calls (bare,
fenced, in `<tool_call>` tags, or an array of them), and the interface turns
that reply into the call: the JSON leaves the conversation, the history the
model gets back carries `tool_calls`, and the call runs like any other. A
reply that merely contains JSON, or names a tool the agent does not have, is
left alone.

The prompt matters as much as the parser: with the first prompt the same
model answered "Sure, I'll add five quotes to `text.txt`" and stopped, turn
after turn. Told that a reply announcing an action without the call is wrong,
it writes the call. What it still cannot do is call with something missing,
so the prompt tells it to pick a file name and say which. `edit_file` with an
empty `old_string` on an empty file, the model's way of saying "put this in
it", fills the file instead of failing.

## The sandbox contract

`Sandbox::resolve(path: &str) -> Result<PathBuf, Denied>` is the only way a
tool turns a string from the model into a path. It is called with the raw
string every time; nothing caches a resolved path across calls.

Checks, in order, and the reason the model reads back:

0. **An absolute path under the root** (compared by whole components with the
   root as given and as canonical) loses that prefix and goes on as a
   relative one, through every check below. `read_file` on a path that is not
   found looks for files ending in it (bounded walk, same skips and deny
   rules) and names them, so `src/x.tsx` leads to `frontend/src/x.tsx`.
1. **Lexical, before touching the disk.** Walk `Path::components()`: only
   `Normal` and `CurDir` are allowed. `ParentDir` → `outside the project`;
   `RootDir` or `Prefix` (absolute, drive letters) → `absolute paths are not
   allowed`; a leading `~` → the same. Empty → `empty path`.
2. **Physical, after joining to the root.** The root is canonicalized once at
   start-up (on macOS `/tmp` is `/private/tmp`; the check must compare like
   with like). For an existing target, `fs::canonicalize` must
   `starts_with(root)`. For a new file, the deepest existing ancestor is
   canonicalized instead. This is what stops a symlink from leading out.
3. **No writing through symlinks**, at any depth: if `symlink_metadata` on the
   target says symlink → `is a symlink`. Reading through one that stays inside
   is fine.
4. **Deny list.** `context::is_denied` (`.env*`, keys, anything with `secret`
   or `credential` in the name) with no `!` override for the model, plus
   `.git/` always, plus the globs in `[tools] deny`.
5. **Content.** `context::is_binary` → `binary file`; over `max_file_bytes` →
   `too large`; not valid UTF-8 → `not text`.
6. **Freshness, for edits.** `edit_file` and `write_file` on an existing file
   require a `read_file` of the same path earlier in the conversation, and the
   hash that read returned must equal the file's hash now. Otherwise `file
   changed since it was read; read it again`.

Writes are atomic: content to `<path>.moon-tmp` in the same directory, then
`rename`. Permissions of the original are preserved; a new file gets `0644`.

`sandbox/tests.rs` covers, one case each: `src/../../etc/passwd`,
`/etc/passwd`, `~/x`, `C:\x`, a symlink to `/tmp` inside the tree, a symlink
to a sibling inside the tree (readable), `.env`, `.git/config`, a PNG, a
300 kB file, an edit after the file changed, a new file three directories deep
(created), a new file under a symlinked directory (denied).

## The tools

| Tool | Arguments | Permission | Result to the model |
|---|---|---|---|
| `read_file` | `path`, optional `range` "40-120" | `read files` | the content in the same `<file>` block `@path` uses, plus `hash` |
| `list_dir` | optional `path` (default: root) | `read files` | names, one per line, directories with `/`; `target/`, `.git/`, `node_modules/` skipped |
| `edit_file` | `path`, `old_string`, `new_string`, optional `replace_all` | `edit existing files` | `applied` / `skipped` / the error |
| `write_file` | `path`, `content` | `create new files` (and `edit existing files` to replace one) | `applied` / `skipped` / the error |
| `run_command` | `command`, optional `args`, optional `dir` | each command of the catalogue | exit code, stdout and stderr, cut at 20 kB |

`edit_file` follows the shape most agents use, because small local models
have seen it: `old_string` must occur exactly once (or `replace_all`); if it
does not occur, the error says so and suggests reading again; if it occurs
more than once, the error says to add context or set `replace_all`.
`write_file` creates a file, or replaces one whole; replacing follows the
freshness rule and goes by the permission on editing.

A tool is offered only when its permission is on: the model never sees a tool
it may not use. There is no `delete`, no `rename`, no `glob` or `grep` as
tools; `grep` and `find` exist as catalogue commands instead.

## Permissions

Every capability — moon's three file tools and each command of the catalogue —
has one of three states, and nothing else:

| | |
|---|---|
| `off` | not offered: the model does not see the tool, or the command is not in the list it may call |
| `ask` | shown to the user first — the diff, or the command line — and waits for yes or no |
| `allow` | runs on its own |

Three rules, kept by `Policy` (`tools/catalog.rs`):

1. **Editing and creating files ask by default, and `allow` on them is the
   user's call.** At `allow` the harness applies the edit as it comes
   (`Harness::settle_or_ask`), the step line says `applied` after the fact,
   the prompt tells the model its writes land without waiting, and moon warns
   when this is set in a directory with no `.git/`.
2. **Editing and creating need reading**, so turning either on turns `read
   files` on, and turning `read files` off turns them off. Nothing else is
   coupled: a command is a choice of its own.
3. **Nothing on is tools off.** There is no separate switch.

A `Policy` belongs to an agent, and every agent is a file, `default.toml`
included ([agents.md](agents.md)). A policy is built through
`Policy::from_pairs`, which drops unknown ids and applies rule 2.

**The catalogue** (`tools/catalog.rs`) is one `const` list of `Entry`: an id as
it is shown, written in an agent file and, for a command, called (`read files`,
`git diff`, `ls`); a `Category` (`Editor`, `Files`, `Git`, `Build`, `Docker`,
`Network`) and, within `Build`, a section per tool; a `Kind` (`Read`, `Edit`,
`Create`, `Subfolders`, `Command`); what it turns to when switched on
(`allow` for what only looks, `ask` for what changes the repository, writes
files, runs the project's own code or reaches the network); a deny list of
flags; what it `needs` (another argument that must be present, such as `-d`
for `docker compose up`); and one line of help. Nothing outside the catalogue
can be named, and the catalogue is in the code, not in the configuration.

`Editor` holds moon's own three file tools, which stay tools rather than
commands: `read_file` carries the `<file>` block, the range and the hash the
freshness check needs, `edit_file` has no shell equivalent that shows a diff
first, and `write_file` writes content where `mkdir` and `touch` make an empty
folder or file. It also holds `commands in subfolders` (`off` or `allow`: on,
a call may carry a `dir` below the root, checked like any path) and, in the
panel only, the step limit, which the file keeps as the top-level
`max_steps`.

Some commands are left out on purpose: `docker run` and `docker exec` on their
own, which with `--privileged` or a mounted volume are root on the host; and
`npx`, `pnpm dlx` and `npm exec`, which download and run packages from the
internet.

## Commands

**Checking a call.** `prepare` (`tools/run_command.rs`) turns a call into an
`Exec`:

- The tokens of `command` and `args` are matched against the ids that are on,
  longest first, so `git diff --stat` and `command: "git", args: ["diff",
  "--stat"]` both work, and `git status` is refused while it is off.
- Every remaining token goes through a lexical check — no absolute path, no
  `~`, no drive letter, no `..` component, the value of a `--flag=value`
  included — and through the sandbox's deny rules by name, so `cat .env` and
  `git diff ../x` never start.
- The entry's deny list refuses the flags that would run something else or
  write somewhere (`find -exec`, `git --exec-path`, `git --output`,
  `make --eval`, `cargo --config`, `curl -d`, `docker -H`), and its `needs`
  must be met.
- The program is resolved on `PATH` at call time, with `PATHEXT` on Windows;
  the coreutils on Windows are looked for next to `git.exe` only
  (`<Git>/usr/bin`), never in `System32`, whose `find` is another program.
- `Exec.asks` is the permission in force, not the catalogue's default.

**Running it.** The harness hands back `Command::Run(Exec)` and pauses. The
interface runs it on a thread of its own (`run_command::execute`: no shell,
stdin closed, both streams read on threads of their own, `GIT_EDITOR=true`, no
pager, no colour, killed at 60 s or when the user presses `Esc`) and feeds
back `Event::Ran(Output)`. The model reads the exit code and both streams, cut
at 20 kB. A command at `ask` is `Command::Ask(Pending::Run(exec))` first: the
same approval panel as an edit, with the line it would run.

The model sees the commands that are on in the tool's description and in the
prompt, with which of them ask; with none on, the tool is not offered and the
prompt says there is no shell.

## The interface

- **The marker.** The permanent sign that the model can act is on the bottom
  row, left side, in `moon-soft`: the agent's name and what it may do,
  `⏵⏵ committer · Read · Edit · 4 commands`, or `⏵ default · all off` when
  nothing is on. The same convention Claude Code uses for its mode indicators.
- **Steps.** One muted line per tool call, hanging from the message with `⎿`:
  `⎿  read  src/a.rs`, `⎿  ✎ edit  src/a.rs  +3 −1  applied`,
  `⎿  run   git diff --stat`. A failure is visible where it happened, with
  `✗` and the reason.
- **Approval.** A panel docked under the conversation, in the input box's
  place: the title (`Edit file`, `Create file`, `Run command`) with the counts
  or the folder on the right, the file or the command under it, the diff or
  the command line as the body, and a question pinned at the bottom with two
  choices, `❯ 1. Yes` / `2. No`. `1`/`y` and `2`/`n` answer at once, `↑↓`/`jk`
  and `enter` choose and confirm, `pgup`/`pgdn`/`space` scroll, `esc` cancels
  the turn. While it waits, the provider request has already finished: nothing
  is timing out.
- **The status row** says `waiting for your approval · 1 yes · 2 no · esc
  cancel the turn`, or `running git diff --stat (2s) · esc to cancel`.
- **The permissions panel** is one per agent, opened from `/agent` with
  `ctrl+t` or by `/tools`; see [tools.md](tools.md#the-permissions-panel).
- `/context` lists the tools on offer and the root, the agent when it is not
  `default`, and the capabilities that are on, by permission.

## Configuration and sessions

`[tools]` in `config.toml` holds the two limits that are not permissions:

```toml
[tools]
max_file_bytes = 200000                    # bigger files are neither read nor edited
deny           = [".github/workflows/**"]  # on top of .git/ and the secrets filter
```

**The agent files** (`agents/<name>.toml`) hold the permissions: a
`[permissions]` table with every entry of the catalogue, in its group, `off`
included, each with its help as a comment — laid out by
`AgentFile::to_toml`, so a file is also the list of what there is — plus
`max_steps`, a description and an optional prompt. `moon config init`
writes `default.toml` (reading on, everything else off) and `reviewer.toml`;
`moon config reset` writes them again. Every step in an agent's panel
rewrites its file whole, through a temporary file; moon reads the folder
back at startup, when the picker opens and before every message
(`App::sync_agents`, the loop rebuilt only when the chosen definition
differs from the last one read), and at each read brings a file that names
fewer capabilities than there are up to the catalogue. They are files of
their own because `config.toml` is a commented template written by hand,
and rewriting it from the interface would lose the comments. A file that
does not parse is reported and left alone until it is fixed. The older
`enabled`, `edit` and `create` keys under `[tools]`, a `[tools.permissions]`
table, or a `tools.toml` from before, are read once, when `default.toml` is
first written.

Sessions stay JSONL. Assistant messages may carry `tool_calls`; tool messages
carry `tool_name` / `tool_call_id`; `export_markdown` prints `Role::Tool` as a
code block. The meta line records whether tools were on (`tools`,
`tools_edit`, `tools_create`) and the chosen agent (`agent`, only when it is
not `default`). Every field is optional, so every session file written so far
still loads. Resuming re-sends the history to the provider as is.

**`moon ask`** gets no tools: it has no terminal to approve in, and a
`--tools` on it would need a `--yes`, which is exactly the flag this design
refuses.

## What this does not protect against

The sandbox stops the model from running anything outside the catalogue. It
cannot stop it from writing a file that **you** — or a command it may run —
will execute later: a `Makefile`, `build.rs`, `.cargo/config.toml` (a
`runner` or a `rustc-wrapper`), a git hook, a CI workflow, `package.json`
scripts. The deny list covers `.git/` and `.github/workflows/`; the rest is
covered by the approval, which is why edits and the commands that run project
code ask by default.

Also:

- What a command prints is what the model reads: `grep -r x .` over a folder
  with a `.env` in it shows the model the `.env`, since the deny rules apply to
  the paths named in the arguments, not to what the program opens on its own.
- A model that has read a file can put it in the URL it asks `curl` to fetch.
- Killing a command does not kill its children (`make` spawning `cargo`).
- A write at `allow` lands with no one looking.
- The 60-second limit is the same for every command, so a slow build or a
  cold `npm ci` may be killed before it finishes.

## Platforms

moon ships for Linux, macOS and Windows. The sandbox is the first piece of
moon where a platform difference is a security difference, so CI runs the
whole suite on Linux and macOS, and a Windows job runs the `moon-core` and
`moon-agent` tests.

**Paths.**

- **Separators.** `\` is a separator on every platform *before* the lexical
  check, so `..\..\x` is a `ParentDir` on Linux too instead of one odd file
  name. On Unix a file whose name contains a backslash becomes unreachable to
  the model; acceptable.
- **Windows prefixes.** `Component::Prefix` covers `C:\`, drive-relative `C:x`,
  UNC `\\server\share` and verbatim `\\?\`; `Component::RootDir` covers a
  rooted `\x` with no drive. All of them are "absolute" and rejected, so the
  lexical rule is the same code everywhere.
- **Canonical form.** On Windows `fs::canonicalize` returns `\\?\C:\…`; on
  macOS it turns `/tmp` into `/private/tmp`. Root and target go through the
  same call, so `starts_with` compares like with like. A canonical path is
  never shown to you or to the model: the relative one is.
- **Case.** NTFS and APFS are case-insensitive by default, so the secrets
  filter, the `.git/` rule and the `deny` globs match case-insensitively on
  every platform: `.GIT/config` is denied on Linux too.
- **Reserved device names.** On Windows `CON`, `PRN`, `AUX`, `NUL`,
  `COM1`…`COM9` and `LPT1`…`LPT9`, with or without an extension, name a device
  in whatever directory they appear. Those stems are denied on every platform.
- **Alternate data streams.** On NTFS `notes.md:hidden` writes into a hidden
  stream. A `:` in any component is rejected on every platform, consistent
  with `@path:40-120`, where the colon already means a range. Trailing dots and
  spaces, which Win32 strips, are rejected too, so the path the freshness
  check keys on is the one on disk.
- **Regular files only.** After resolving, `file_type().is_file()` must hold:
  `read_file` on a FIFO would block forever.
- **Symlinks and junctions.** `symlink_metadata().is_symlink()` is true on
  Windows for both, and `canonicalize` resolves both. The physical check is
  the backstop on all three platforms.

**Writing.**

- **Line endings and BOM.** `read_file` normalizes to LF for what the model
  sees and for `old_string` matching, and remembers CRLF/LF and the BOM per
  file; `edit_file` and `write_file` write back in the file's own convention,
  and a new file follows the platform's default. Without this `old_string`
  would never match on Windows and every edit would flip a repository's line
  endings.
- **Atomic rename.** `fs::rename` replaces the target on all three, but on
  Windows it fails while another process holds the file without share-delete
  (an antivirus scan, some editors), so it is retried a few times with a short
  pause before reporting.
- **Permissions.** Preserving mode bits is `#[cfg(unix)]`. On Windows a
  read-only target makes the rename fail, which is the right outcome: it is
  reported, the attribute is not cleared.

**Commands.** The coreutils are looked for next to Git for Windows, never in
`System32` (see *Commands*). Symlink tests need Developer Mode on Windows, so
they run on Linux and macOS; the Windows job covers prefixes, reserved names,
streams, CRLF and the rename retries.

## Decisions

In the order they were taken. Each changed the design on purpose and is
recorded here rather than slipped in.

- **2026-09-22 — files only, every write approved.** The first version: four
  tools (`read_file`, `list_dir`, `edit_file`, `write_file`), one `editor`
  agent, a `/tools` panel with three boxes and a step limit, and no way to
  pre-approve anything.
- **2026-09-24 — commands, from a catalogue.** At the user's request, a fifth
  tool, `run_command`, limited to a fixed list of programs chosen one by one.
  Still no shell, and not planned.
- **2026-09-24 — one permission model.** The boxes and the command list
  became one question — what may the model do, and with what supervision —
  with one answer per capability: `off · ask · allow`. Approving every write
  became the default rather than the ceiling: `allow` on editing or creating
  writes without showing the diff, set by the user and warned about outside a
  git repository. The panel's state moved to `tools.toml`.
- **2026-09-25 — named agents.** `/agent` picks who talks to the model; the
  built-in `reviewer` and user agent files arrive. An agent could only
  restrict what the user granted.
- **2026-09-29 — every agent is a file.** `default` and `reviewer` had lived
  in the binary, with `default`'s permissions in `tools.toml`: two places
  for one thing. Now the agents folder is the whole truth, `tools.toml` is
  read once into `default.toml` and gone, files are brought up to the
  catalogue on every start, and `moon config reset` starts over.
- **2026-09-28 — an agent carries its permissions whole.** The two-layer
  intersection was dropped as confusing; `default` runs on `tools.toml`, every
  other agent on its own file, and `/tools` became an alias for the chosen
  agent's permissions. Details and the designs that were tried and reverted
  are in [agents.md](agents.md#how-it-got-here).

## Not done, on purpose

- A shell, or any way to run something outside the catalogue.
- Tools in `moon ask`.
- Undo of applied edits from inside moon: git is the safety net, and moon says
  so when there is none.
- Remembering approvals ("always allow edits to this file"): `allow` on the
  capability is the only way to stop asking.
- A timeout per command: every command gets the same 60 seconds for now.
