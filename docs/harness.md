# Editing files from moon: harness, agent and sandbox

*Design, 2026-09-22 · implemented the same day in `crates/agent` and wired into the interface; this is the shape the code follows. Amended 2026-09-24: permissions and commands, see "Permissions and commands" below.*

moon promises today that it "never runs tools or writes to disk on the model's
behalf". This proposal keeps that as the default and adds, behind an explicit
switch, one thing the model can do: **read and edit files under the directory
moon was started in**, one approved diff at a time. Three rules that the code
must make impossible to break, not merely discouraged:

1. **Only files, never a shell.** There is no shell tool: nothing the model
   sends is ever handed to `sh` or `cmd`. Since 2026-09-24 there is a
   `run_command` tool, limited to a fixed catalogue of programs the user ticks
   one by one in `/tools`; it starts the program with its arguments as a list,
   and that is the whole of it. The rule was "never commands" when this was
   written; the decision to change it is recorded, not slipped in.
2. **Only forward, never back.** Paths are relative to the start-up directory
   and must stay under it: no `..`, no `~`, no absolute path outside the root
   (one inside it, as pasted from the editor, is taken as relative), no symlink
   that leads outside. Enforced in one place, before and after touching the disk.
3. **Only with your ok.** Every write shows its diff and waits. Reads and
   listings inside the sandbox run without asking. There is no `--yolo`.
   *(2026-09-24: the wait became the default rather than the rule — `allow`
   on editing or creating files, set by the user in `/tools`, writes without
   showing the diff. See "Permissions and commands".)*

The rest of this document is where the code goes, how a turn runs, what the
sandbox checks, what the interface shows and in which order to build it.

## Where the code lives

Everything that makes the model act is one new crate, `moon-agent`, with four
folders named after the four ideas. It depends on `moon-core` only: no
terminal, no HTTP, no ratatui. The interface drives it; the providers do not
know it exists.

```
crates/
├── core/                     moon-core · types shared by everyone (see "Changes to existing crates")
├── providers/{ollama,openai} · send the tool specs, parse the tool calls (see below)
├── agent/                    moon-agent · NEW · the model acting on the project
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs            what the crate is; re-exports Harness, Agent, Sandbox, Tool
│       ├── harness/          THE LOOP · a pure state machine, no I/O of its own
│       │   ├── mod.rs        Harness: feed it events, it hands back commands; the limits
│       │   ├── fallback.rs   a call the model wrote as text (small models do) becomes a call
│       │   └── tests.rs      a scripted model, a scripted user; no provider, no terminal
│       ├── agents/           WHO TALKS TO THE MODEL · prompt + tool set + policy
│       │   ├── mod.rs        Agent { name, system_prompt, tools, policy }
│       │   └── editor.rs     the only one in v1: reads, lists, edits and writes inside the project
│       ├── tools/            WHAT AN AGENT CAN DO · a closed enum; no `bash` variant exists
│       │   ├── mod.rs        Tool enum, ToolSpec (the JSON schema the model sees), PendingEdit
│       │   ├── read_file.rs  read_file { path, range? } → the <file> block; remembers the hash
│       │   ├── list_dir.rs   list_dir { path? } → entries (skips target/, .git/, node_modules/)
│       │   ├── edit_file.rs  edit_file { path, old_string, new_string, replace_all? } → PendingEdit
│       │   ├── write_file.rs write_file { path, content } → PendingEdit (new file or full replace)
│       │   ├── diff.rs       the line diff and its +N −M, computed here, painted by the interface
│       │   └── tests.rs      the four tools against a real tree
│       └── sandbox/          THE BOUNDARY · every tool goes through it, nothing goes around it
│           ├── mod.rs        Sandbox { root }: the two checks, the deny rules, CRLF/BOM, atomic writes
│           ├── glob.rs       the deny-list globs: `*`, `?`, `**`, never case-sensitive
│           └── tests.rs      "..", absolute, "~", reserved names, symlink out, .env, .git/, binary, size, stale hash
├── tui/
│   └── src/
│       ├── app/agent.rs      NEW · the switch, the harness's commands on screen, the approval panel's keys
│       ├── app/tests_agent.rs NEW · a scripted provider drives whole turns through the interface
│       └── view/diff.rs      NEW · paints a diff with the theme (added in `ok`, removed in `alert`)
└── cli/                      `moon ask` gets no tools: nowhere to approve
```

Dependency direction, so nothing leaks the wrong way:

```
moon-core  ←  moon-agent  ←  moon-tui  ←  moon-cli
moon-core  ←  moon-provider-*  ←  moon-cli
```

`moon-agent` never imports a provider; `moon-provider-*` never import the
agent. The harness is "sans I/O": it decides, the interface acts.

## How a turn runs

A *turn* is everything that happens between you pressing `Enter` and the
model's final answer. Without tools a turn is one request. With tools it is a
loop, and the harness owns it:

```
   you                    moon-tui (app/agent.rs)             moon-agent (harness)            provider
    │  Enter                       │                                   │                          │
    ├──────────────────────────────►  Harness::start(messages)         │                          │
    │                               ├──────────────────────────────────►                          │
    │                               ◄── Command::Send(request+tools) ──┤                          │
    │                               ├──────────────────────────────────────────────────────────────►
    │                               ◄── Delta / ToolCall / Done ──────────────────────────────────┤
    │                               ├── Event::ModelDone(calls) ───────►                          │
    │                               │        read_file / list_dir run here, inside the sandbox    │
    │                               ◄── Command::Ask(PendingEdit{diff}) ┤   (for edit/write)      │
    │  ◄── approval panel ──────────┤                                   │                          │
    │  Enter (apply) / s (skip)     │                                   │                          │
    ├──────────────────────────────►├── Event::Verdict(Apply) ─────────►                          │
    │                               │        the sandbox writes atomically                        │
    │                               ◄── Command::Send(request + tool results) ─┤                  │
    │                               ├──────────────────────────────────────────────────────────────►
    │                               ◄── Delta … Done (no more calls) ─────────────────────────────┤
    │                               ├── Event::ModelDone([]) ──────────►                          │
    │                               ◄── Command::Finished ─────────────┤                          │
```

The harness's whole surface:

```rust
pub enum Event   { ModelDone(Vec<ToolCall>), Verdict(Verdict), Cancel }
pub enum Verdict { Apply, Skip }
pub enum Command {
    Continue(Vec<Message>),      // add these tool results and ask the model again
    Ask(PendingEdit),            // show this edit and wait
    Step(Step),                  // a line for the conversation
    Finished,                    // the last reply had no calls
    Stopped { reason: Stop, results: Vec<Message> },
}

impl Harness {
    pub fn new(agent: Agent, sandbox: Sandbox, limits: Limits) -> Self;
    pub fn begin_turn(&mut self);                       // a new user message
    pub fn feed(&mut self, ev: Event) -> Vec<Command>;
    pub fn specs(&self) -> Vec<ToolSpec>;               // what goes in the request
    pub fn prompt(&self) -> &'static str;               // what goes in the system prompt
}
```

The harness never builds a request: it hands back the tool results and the
interface, which owns the conversation and the system prompt, sends the next
one. That is what keeps it free of I/O and testable with a scripted model.

`Turn` keeps the limits that stop a small model from looping:

| Limit | Default | When hit |
|---|---|---|
| Rounds per turn (model → tools → model) | 8 | `Stopped(TooManyRounds)`, the text so far stays |
| Tool calls per round | 10 | the rest are answered "too many calls, ask again" |
| Sandbox rejections per turn | 3 | `Stopped(TooManyRejections)` |
| `Esc` | — | `Stopped(Cancelled)`: nothing pending is written |

Every tool result goes back to the model as a `Role::Tool` message, including
the rejections ("`../x` is outside the project"), so a model that can correct
itself does.

**A call written as text.** On Ollama 0.34, `qwen2.5-coder:14b` answers a
request that offers tools with `{"name": "write_file", "arguments": {…}}` as
plain content and no `tool_calls`, streaming or not; `qwen3:4b` returns real
calls. `harness/fallback.rs` recognizes a reply that is nothing but calls
(bare, fenced, in `<tool_call>` tags, or an array of them), and the interface
turns that reply into the call: the JSON leaves the conversation, the history
the model gets back carries `tool_calls`, and the call runs like any other. A
reply that merely contains JSON, or names a tool the agent does not have, is
left alone.

The prompt matters as much as the parser: with the first prompt the same
model answered "Sure, I'll add five quotes to `text.txt`" and stopped, turn
after turn. Told that a reply announcing an action without the call is wrong,
it writes the call (in a code fence, after a sentence, which the fallback
accepts). What it still cannot do is call with something missing: "create
another file with one quote" and no name gets a sentence back, so the prompt
tells it to pick a name and say which. `edit_file` with an empty `old_string`
on an empty file, the model's way of saying "put this in it", fills the file
instead of failing.

## The sandbox contract

`Sandbox::resolve(path: &str) -> Result<PathBuf, Denied>` is the only way a
tool turns a string from the model into a path. It is called with the raw
string every time; nothing caches a resolved path across calls.

Checks, in order, and the reason the model reads back:

0. **An absolute path under the root** (compared by whole components with
   the root as given and as canonical) loses that prefix and goes on as a
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
   `starts_with(root)`. For a new file, canonicalize the deepest existing
   ancestor instead. This is what stops a symlink from leading out.
3. **No writing through symlinks**, at any depth: if `symlink_metadata` on the
   target says symlink → `is a symlink`. Reading through one that stays
   inside is fine.
4. **Deny list.** `context::is_denied` as it is (`.env*`, keys, anything with
   `secret` or `credential` in the name) with no `!` override for the model,
   plus `.git/` always, plus the globs in `[tools] deny`.
5. **Content.** `context::is_binary` → `binary file`; over `max_file_bytes` →
   `too large`; not valid UTF-8 → `not text`.
6. **Freshness, for edits.** `edit_file` and `write_file` on an existing file
   require a `read_file` of the same path earlier in the *turn*, and the hash
   that read returned must equal the file's hash now. Otherwise `file changed
   since it was read; read it again`. This reuses the hash `Attachment`
   already computes.

Writes are atomic: content to `<path>.moon-tmp` in the same directory, then
`rename`, as `SessionStore::rewrite` already does. Permissions of the original
are preserved; a new file gets `0644`.

What the sandbox tests must cover, each one a case in `sandbox/tests.rs`:
`src/../../etc/passwd`, `/etc/passwd`, `~/x`, `C:\x` (Windows), a symlink to
`/tmp` inside the tree, a symlink to a sibling inside the tree (allowed to
read), `.env`, `.git/config`, a PNG, a 300 kB file, an edit after the file
changed, a new file three directories deep (created), a new file under a
symlinked directory (denied).

## The tools

| Tool | Arguments | Asks you | Result to the model |
|---|---|---|---|
| `read_file` | `path`, optional `range` "40-120" | no | the content in the same `<file>` block `@path` uses, plus `hash` |
| `list_dir` | optional `path` (default: root) | no | names, one per line, directories with `/`; `target/`, `.git/`, `node_modules/` skipped |
| `edit_file` | `path`, `old_string`, `new_string`, optional `replace_all` | **yes** | `applied` / `skipped` / the error |
| `write_file` | `path`, `content` | **yes** | `applied` / `skipped` / the error |

`edit_file` follows the shape crush and most agents use, because the small
local models have seen it: `old_string` must occur exactly once (or
`replace_all`); if it does not occur, the error says so and suggests reading
again; if it occurs more than once, the error says to add context or set
`replace_all`. `write_file` creates a file, or replaces one whole; replacing
follows the freshness rule.

There is no `delete`, no `rename`, no `glob`, no `grep` as tools, no network.
`grep` and `glob` are the first candidates for v2 as tools; deletion is not
planned. Commands are the fifth tool, added later: see "Permissions and commands".

## Permissions and commands (added 2026-09-24)

The user asked for commands the model may run, ticked one by one in
`/tools`, and then for something solid rather than two kinds of switch
(file boxes on one side, a command list on the other) for what is one
question: what may the model do, and with what supervision. The answer is
**one permission model**: every capability — moon's own file tools and each
command of a fixed catalogue — has one of three states, and nothing else.

| | |
|---|---|
| `off` | not offered: the model does not see the tool, or the command is not in the list it may call |
| `ask` | shown to the user first — the diff, or the command line — and waits: `Apply`/`Run` or `Skip` |
| `allow` | runs on its own |

Three rules, and only three, kept by `Policy` (`tools/catalog.rs`):

1. **Editing and creating files ask by default, and `allow` on them is the
   user's call.** Rule 3 of this document said every write waits; on
   2026-09-24, at the user's request, that became the default rather than
   the ceiling. At `allow` the harness applies the edit as it comes
   (`Harness::settle_or_ask`: a `write_file` on a file that exists goes by
   the permission on editing, a new file by the one on creating), the step
   line says `applied` after the fact, the prompt tells the model its
   writes land without waiting, and the panel warns when it is set in a
   directory with no `.git/`, since git is then the only way back.
2. **Editing and creating need reading**, so turning either on turns
   `read files` on, and turning `read files` off turns them off. Nothing
   else is coupled: a command is a choice of its own, and `make` at `allow`
   is the user's to make.
3. **Nothing on is tools off.** There is no separate switch.

**The catalogue** (`tools/catalog.rs`) is one `const` list of `Entry`: an
id as it is shown, written in `tools.toml` and, for a command, called
(`read files`, `git diff`, `ls`); a group (`Editor`, `Files`, `Git`,
`Build`, `Network`); a `Kind` (`Read` = `read_file` + `list_dir`, `Edit`,
`Create`, `Subfolders`, `Command`); what `Enter` turns it to (`on`: `allow`
for what only looks, `ask` for what changes the repository, writes files,
runs the project's own code or reaches the network); a deny list of flags;
and one line of help. `Editor` holds moon's own three — they stay tools and
are not turned into commands: `read_file` carries the `<file>` block, the
range and the hash the freshness check needs, `edit_file` has no shell
equivalent that shows a diff first, and `write_file` writes content where
`mkdir` and `touch` make an empty folder or file. `Files` holds the
programs that work on files, `mkdir` among them (`ask`). `Editor`
also holds `commands in subfolders` (called `run inside subfolders` while
it was in `Files`; the old name is still read), which is not a program:
`off` or `allow`, never `ask`, and on, a call may carry a `dir`, a folder
below the project root to run in, checked like any path so it can never be
above it. The step limit is the last row of `Editor` in the panel, though
in `tools.toml` it stays the top-level `max_steps`: it is a number, not a
permission. The user wanted both there, with the file tools, rather than on
the first level; the name keeps "commands" so it does not read as a limit
on where files are read or edited. `Network` holds `curl` and
`wget`, `ask` by default, with the flags that would send a file away
refused (`-d/--data*`, `-F`, `-T`, `-K` for `curl`; `--post-file`, `-i`,
`-e` for `wget`); what a model has read it can still put in a URL, which
the `ask` shows. Nothing outside the catalogue can be named, and the
catalogue is in the code, not in the configuration. `Agent` carries a
`Policy` and derives its tools from it (`Agent::for_policy`: the editor's
prompt when it may change files, the reader's otherwise).

**The tool.** `run_command { command, args?, dir? }`. The description the
model sees lists the commands that are on, and so does the prompt, with
which of them ask; with none on the tool is not offered and the prompt says
"no shell" as before. `prepare` (`tools/run_command.rs`) turns a call into
an `Exec`: the tokens of `command` and `args` are matched against the ids
that are on, longest first, so `git diff --stat` and `command: "git", args:
["diff", "--stat"]` both work and `git status` is refused while it is off;
every remaining token goes through a lexical check — no absolute path, no
`~`, no drive letter, no `..` component, the value of a `--flag=value`
included — and through the sandbox's deny rules by name, so `cat .env` and
`git diff ../x` never start; the command's own deny list refuses the flags
that would run something else or write somewhere (`find -exec`, `git
--exec-path`, `git --output`, `make --eval`, `cargo --config`). The program
is resolved on `PATH` at call time, with `PATHEXT` on Windows; the coreutils
on Windows are looked for next to `git.exe` only (`<Git>/usr/bin`), never in
`System32`, whose `find` is another program. `Exec.asks` is the permission,
not the catalogue's default.

**Running it.** The harness never runs a process: it hands back
`Command::Run(Exec)` and pauses, the interface runs it on a thread of its own
(`run_command::execute`: no shell, stdin closed, both streams read on threads
of their own, `GIT_EDITOR=true`, no pager, no colour, killed at 60 s or when
the user presses `Esc`), and feeds back `Event::Ran(Output)`. The model reads
the exit code and both streams, cut at 20 kB. This keeps the loop free of I/O
and testable with a scripted output, and keeps the interface responsive while
`cargo test` runs. A command at `ask` is `Command::Ask(Pending::Run(exec))`:
the same approval panel as an edit, with the line it would run, `Run` and
`Skip`. The approval and the diff remain the security boundary for what the
path check cannot see: the model can write a `Makefile` and then ask to run
`make`, and both go through you — unless you set them to `allow`.

**What it does not protect against**, on top of the list below: what a
command prints is what the model reads. `grep -r x .` over a folder with a
`.env` in it shows the model the `.env`, since the deny rules apply to the
paths named in the arguments, not to what the program opens on its own.
Killing the child does not kill its grandchildren (`make` spawning `cargo`).
A write at `allow` lands with no one looking.

**In the panel.** Two levels. On the first, the groups of the catalogue,
one row each with what is on in it (`allow: git status, git diff · ask:
git commit`, or `off`), `Enter` to open one, `←` to turn the whole of it
off and `→` to turn it on with its defaults, and the step limit as the last
row. Inside a group, its entries with their permission as a selector
(`◀ allow ▶`): `←`/`→` walk `off · ask · allow`, `Enter` and `Space` go
between off and the entry's `on`. `Esc` always saves — state, session meta
and `tools.toml` — and inside a group also steps back out, from the groups
also closes; there is no cancel and no other key to save. A program that is
not installed shows `not installed` and stays off, and a group's defaults
leave it out. The marker on the bottom row reads `⏵⏵ Read · Edit · Create ·
4 commands`. Two levels rather than one long list because the first level is
the overview that a flat list of thirty rows could not give, each screen
stays short, and the catalogue can grow without lengthening either. This
replaces the four boxes of the first design and the two-section panel that
came between: the boxes are the `Editor` group now.

**Where it lives.** `tools/catalog.rs` (`Entry`, `CATALOG`, `Policy`),
`tools/run_command.rs`, `Tool::RunCommand`, `Pending { Edit, Run }`,
`Agent.policy` and `Agent::system_prompt()`, `Harness::running` and
`Harness::settle_or_ask`; in the interface, `ToolsDialog` over a `Policy`
with its two `Level`s in `app/agent.rs`, `Action::Ran`, `App.running`, the
groups and the group in `view/panel.rs`, the `Run` approval next to the
diff. `moon_core::Permission` and the ids of the file capabilities
(`moon_core::config::ids`) live in core, since the configuration and the
file need them.

## The agent

`agents/editor.rs` is a value, not behaviour:

```rust
pub fn editor() -> Agent {
    Agent {
        name: "editor",
        prompt: PROMPT,   // call, do not announce; read before you edit; fill an
                          // empty file with write_file; no shell; say what changed
        tools: &[Tool::ReadFile, Tool::ListDir, Tool::EditFile, Tool::WriteFile],
    }
}
```

There is no policy field: every write waits for the user, and that is not a
setting.

Its prompt is appended after the base system prompt, `MOON.md` and the live
attachments that `context::build_system_prompt` already assembles, so the
model still knows the project the way it does today. A second agent later
(say, a read-only "reviewer" with `read_file` and `list_dir` only) is another
file in `agents/` and nothing else.

## Changes to existing crates

Small and mechanical; the design lives in `moon-agent`.

**`moon-core`** (`types.rs`, `config.rs`, `config.template.toml`):

- `ToolSpec { name, description, parameters: serde_json::Value }` and
  `ChatRequest.tools: Vec<ToolSpec>` (empty means "no tools", as today).
- `ToolCall` gains `id: Option<String>` (OpenAI needs it back; Ollama has none).
- `Message` gains `tool_calls: Vec<ToolCall>` (assistant) and
  `tool_name: Option<String>` / `tool_call_id: Option<String>` (tool). All
  `#[serde(default, skip_serializing_if …)]`, so every session file written so
  far still loads unchanged.
- `Config.tools: ToolsConfig { enabled, max_file_bytes, deny }`.

**`moon-provider-ollama`** (`wire.rs`, `lib.rs`): serialize `tools`; on the
assistant echo, `tool_calls`; on tool messages, `tool_name`. `ChunkMessage`
gains `tool_calls: Vec<{ function: { name, arguments: Value } }>` and the
stream yields `ChatEvent::ToolCall` for each. One wiremock test with a chunk
that carries a call.

**`moon-provider-openai`** (`lib.rs`): the same, with the OpenAI shapes:
`tool_calls` arrive as deltas with an `index`, an `id` and `function.arguments`
as **fragments of a JSON string**; the stream accumulates by index and yields
the calls when `finish_reason` is `tool_calls` (or at `[DONE]`). Tool messages
carry `tool_call_id`. This is the fiddliest piece of the whole feature; one SSE
test with the arguments cut across three chunks.

**`moon-tui`**:

| File | Change |
|---|---|
| `app/agent.rs` (new) | `impl App`: the `/tools` panel, runs each `Command`, `Panel::Approval`, verdicts, the step lines |
| `app/mod.rs` | `Panel::Approval(PendingEdit)`, `Generation::AwaitingApproval`, `Item::Step(StepLine)`, `tools_on: bool` |
| `app/chat.rs` | `start_generation` asks the harness for the request when tools are on; `ToolCall` events are forwarded instead of dropped |
| `app/keys.rs` | keys of the approval panel: `↑↓`, `Enter`, `s`, `Esc` |
| `app/slash.rs`, `commands.rs` | `/tools` |
| `app/status.rs` | activity `reading …` / `waiting for your approval …`; the `✎ edits` marker; hints |
| `app/render.rs` | welcome line when on; how a `Step` is drawn |
| `view/panel.rs`, `view/diff.rs` (new) | the approval panel: title with `+N −M`, the diff, `Apply` / `Skip` |
| `view/tests.rs`, `app/tests.rs` | one `TestBackend` snapshot of the panel; the toggle; a scripted turn end to end |

Dependency added to `moon-agent`: `similar` (line diff, pure Rust, small). The
harness computes the diff so the step line can say `+3 −3`; `view/diff.rs`
only paints it.

**`moon-cli`**: nothing in v1. `moon ask` has no terminal to approve in and
does not get tools; a future `--tools` on `ask` would need a `--yes`, and that
is exactly the flag this design refuses.

## The interface

*(2026-09-24: the panel is one list of permissions now, see "Permissions and commands"; what follows is the first design, kept for the record.)*

**Switch.** `/tools` opens a docked panel like the one Claude Code uses to
set up its auto mode: a question as the title, one line on what it means,
then a few rows walked with the cursor, each with its control on the right.
Four of them: *Read files* `[✓]` (the switch: `read_file` and `list_dir`),
*Edit existing files* `[✓]` (`edit_file`), *Create new files* `[✓]`
(`write_file`) and *Max steps per message* `◀ 8 ▶` (rounds of tool calls
before the turn stops). Ticking a writing box ticks *Read files*; unticking
*Read files* unticks all three, so what is ticked is always what will be on.
Opened while off, every box starts off. A muted line under them explains the row under the
cursor; it keeps its place, so the panel does not change height.
Both writing boxes off gives the `reader` agent, with its own prompt that says
it cannot change files, and the marker reads `⏵⏵ Read` alone. Create without
edit refuses `write_file` on a file that exists, so it cannot replace one
whole.
The scope is saved with the switch in the session's `meta` line
(`tools_edit`, `tools_create`, written only when off). `Enter` ticks the box under the cursor; `←`/`→` change the
number; `Esc` applies whatever is set and closes the panel. There is no
cancel and no `Continue` row. The panel is the only switch: there is no
`/tools on`. The three boxes have a setting each — `[tools] enabled`, `edit`
and `create` — for the state a conversation starts in, read in
`enable_tools()`; from the panel and on a resume the scope is set again right
after, so they only bite at startup. A file that predates `edit` and `create`
and only says `enabled = true` still gets all four tools: that is what their
`Default` is. `moon config init` writes reading on and both writing boxes off.
The state is saved in
the session's `meta` line, so a resumed conversation comes back as it was.
Turning it on checks `caps.tools` of the current model and refuses with a
notice if the model has none; `/model` repeats the check. Turning it on in a
directory with no `.git/` warns once: *not a git repository: moon cannot undo
what you apply*.

**Marker.** The welcome block with the directory scrolls away, so the
permanent sign is on the bottom row, left side, before the hints, in
`moon-soft`, the lighter of the two brand blues: one word per box that is
on, `⏵⏵ Read · Edit · Create`, the same convention Claude Code uses for its
own mode indicators. Off means nothing is drawn: moon stays quiet by
default.

**Steps in the conversation.** One muted line per tool call: `·` for reads,
`✎` for writes, with `+N −M` and the outcome. A rejection is visible where it
happened: `✗ denied · ../x is outside the project`.

**Approval panel.** The same docked panel `Panel::SessionAction` uses for
deleting a session: the file and the counts in the title, the two choices as
chips on the second row (the one the cursor is on painted on `moon`, like an
open help tab, so they never scroll away), and the diff as the body. `Enter`
applies, `s` skips, `Esc` cancels the turn, `PgUp`/`PgDn` scroll. While it
waits, the provider request has already finished: nothing is timing out.

```
 ☾ moon v0.3.0
   qwen2.5-coder:14b (ctx 32.8k) · ollama · http://localhost:11434
   ~/Towerforge/moon

 ❯ rename Item::Info to Item::Note

   · read  crates/tui/src/app/mod.rs
   ✎ edit  crates/tui/src/app/mod.rs · +3 −3 · applied
   ✎ edit  crates/tui/src/app/chat.rs · +1 −1 · waiting

 waiting for your approval · enter apply · s skip · esc cancel     context 12%
 ──────────────────────────────────────────────────────────────────────────────
 Apply edit · crates/tui/src/app/chat.rs                                +1 −1

   280 -        self.push_item(Item::Info("(empty reply)".into()));
   280 +        self.push_item(Item::Note("(empty reply)".into()));

   ❯ Apply     Skip
 ↑↓ choose · enter confirm · esc cancel turn
```

`/context` lists the tools, the root and the limits; `/help` gets the command
and the panel keys.

## Configuration and sessions

```toml
[tools]
enabled        = true                         # Read files: read_file and list_dir at startup
edit           = false                        # Edit existing files: adds edit_file
create         = false                        # Create new files: adds write_file
max_file_bytes = 200000                       # bigger files are neither read nor edited
deny           = [".git/**", ".github/workflows/**"]   # on top of the built-in secrets list
```

There is deliberately no `approve = "never"`, no `allow = [...]` and no
`--yolo`. If that ever changes it is a new decision, not a flag left in.
It changed on 2026-09-24, as a decision: `[tools.permissions]`, a table of
`"id" = "off" | "ask" | "allow"` by the names the panel shows, says what a
conversation starts with; the three older keys are honoured while the table
is empty (`ToolsConfig::startup_permissions`). Edits and new files may be
set to `allow` there too; `ask` is only the default.

**`tools.toml`** (2026-09-24). What the panel sets lives in a file of its
own next to `config.toml`: `max_steps` and a `[permissions]` table with
every entry of the catalogue, in its group, `off` included, each with its
help as a comment — laid out by `catalog::render_tools_file`, so the file
is also the list of what there is. `moon config init` writes it (reading on,
everything else off) next to `config.toml`, and `--force` rewrites both.
The panel rewrites it whole on every `Esc` (`ToolsFile::write_text`,
through a temporary file); moon reads it back at startup, when the panel
opens and before every message (`App::sync_tools_file`, applied only when
the parsed value differs from the last one seen; `ToolsFile::load` drops
the `off` lines, so a full file and a short one read the same). A file of
its own rather than `config.toml` because that one is a commented template
written by hand, and rewriting it from the interface would lose the
comments. While the file exists it wins over `[tools]` and over the
session's meta (`tools`, `tools_edit`, `tools_create`, still written, only
read on a resume when there is no file); a file that does not parse is
reported and ignored, so the configuration applies until it is fixed.

Sessions stay JSONL with the two record types they have. Assistant messages
may carry `tool_calls`; tool messages carry `tool_name` / `tool_call_id`;
`export_markdown` already prints `Role::Tool` as a code block. Resuming a
session re-sends that history to the provider as is.

## What this does not protect against

The sandbox stops the model from running anything *directly*. It cannot stop
it from writing a file that **you** will run later: a `Makefile`, `build.rs`,
`.cargo/config.toml` (a `runner` or a `rustc-wrapper`), a git hook, a CI
workflow, `package.json` scripts. The deny list covers `.git/` and
`.github/workflows/`; the rest is covered by you reading the diff before it
lands. That is why approval is always on and why there is no way to preapprove
a tool: the diff is the security boundary for everything the path check cannot
see.

## Platforms

moon ships for Linux, macOS and Windows. CI runs the tests on Linux and macOS
only; the Windows binary is cross-compiled for the release and never
executed. That has been fine for a chat client. The sandbox is the first piece
of moon where a platform difference is a security difference, so
**`ci.yml` gets `windows-latest` in its matrix** with this feature, even if at
first only to run the `moon-agent` tests there.

What has to be done on purpose, by area:

**Paths.**

- **Separators.** Treat `\` as a separator on every platform *before* the
  lexical check, so `..\..\x` is a `ParentDir` on Linux too instead of one odd
  file name. On Unix a file whose name contains a backslash becomes
  unreachable to the model; acceptable.
- **Windows prefixes.** `Component::Prefix` covers `C:\`, drive-relative `C:x`,
  UNC `\\server\share` and verbatim `\\?\`; `Component::RootDir` covers a
  rooted `\x` with no drive. All of them are "absolute" and rejected, so the
  lexical rule is the same code everywhere.
- **Canonical form.** On Windows `fs::canonicalize` returns `\\?\C:\…`; on
  macOS it turns `/tmp` into `/private/tmp`. Root and target go through the
  same call, so `starts_with` compares like with like. A canonical path is
  never shown to you or to the model: the relative one is.
- **Case.** NTFS and APFS are case-insensitive by default. `context::is_denied`
  already lowercases the name; the `.git/` rule and the `deny` globs must
  match case-insensitively too, on every platform, so `.GIT/config` is denied
  on Linux as well as on Windows.
- **Reserved device names.** On Windows `CON`, `PRN`, `AUX`, `NUL`, `COM1`…`COM9`
  and `LPT1`…`LPT9`, with or without an extension, name a device in whatever
  directory they appear: writing to `NUL` silently discards, reading `CON`
  blocks on the console. Those stems are denied on every platform; no project
  needs them.
- **Alternate data streams.** On NTFS `notes.md:hidden` writes into a hidden
  stream of `notes.md`. A `:` in any component is rejected on every platform,
  which is consistent with `@path:40-120`, where the colon already means a
  range. Trailing dots and spaces, which Win32 strips, are rejected too so the
  path string the freshness check keys on is the one on disk.
- **Regular files only.** After resolving, `file_type().is_file()` must hold.
  Devices, sockets and FIFOs are the Unix version of `CON`: `read_file` on a
  FIFO inside the project would block forever.
- **Symlinks and junctions.** `symlink_metadata().is_symlink()` is true on
  Windows for symlinks and junctions alike, and `canonicalize` resolves both.
  The physical check is the backstop on all three.

**Writing.**

- **Line endings and BOM.** Files on Windows are often CRLF and some start with
  a BOM. `read_file` normalizes to LF for what the model sees and for
  `old_string` matching, and remembers `CrLf`/`Lf` and the BOM per file;
  `edit_file` and `write_file` write back in the file's own convention, and a
  new file follows the platform's default. Without this `old_string` never
  matches on Windows and every edit would flip a repository's line endings.
- **Atomic rename.** `fs::rename` replaces the target on all three
  (`MoveFileEx` with `REPLACE_EXISTING` on Windows), but on Windows it fails
  while another process holds the file without share-delete: an antivirus
  scan, some editors. Retry a few times with a short pause before reporting.
  `SessionStore::rewrite` relies on the same call today, so this is behaviour
  moon already has, now with retries.
- **Permissions.** Preserving mode bits is `#[cfg(unix)]`. Windows has only the
  read-only attribute, and a read-only target makes the rename fail, which is
  the right outcome: report it, do not clear the attribute.
- The temporary file sits in the target's directory, so it is on the same
  volume and the rename is atomic on every filesystem that matters.

**Interface.** `✎`, `·`, `✗` and `❯` are in the same Unicode blocks moon
already draws (`✓`, `✗`, `❯`, `▲`, braille), so a terminal that shows moon
today shows these. Keys go through crossterm, the same on all three.

**Tests.** `tempfile` works everywhere. Creating a symlink in a test is
`std::os::unix::fs::symlink` under `#[cfg(unix)]`; on Windows it needs
Developer Mode or an elevated shell, so the symlink cases run on Linux and
macOS and the Windows run covers prefixes, reserved names, streams, CRLF and
the rename retries. The `canonicalize` path the symlink tests exercise is the
same code on Windows.

## Plan

This is the order it was built in. Each phase compiles, passes `make check`
and is useful on its own.

1. **`moon-core` types + `moon-agent` sandbox and tools.** No interface yet.
   All of "The sandbox contract" as tests on a `tempdir`; the tools against the
   same tree, and the Windows cases from "Platforms". `ci.yml` gets
   `windows-latest` in this phase. This is the part worth getting right first
   and it needs neither a model nor a terminal.
2. **Providers.** `tools` out, `tool_calls` in, tool messages back, for both.
   wiremock tests with a call cut across chunks. `moon ask` still ignores
   `ToolCall` events, so nothing changes for users.
3. **Harness + editor agent.** The state machine with a scripted model and a
   scripted user in `harness/tests.rs`: a clean turn, a skip, a cancel, the
   three limits, a rejection fed back and corrected.
4. **Interface.** `app/agent.rs`, the panel, the diff, the marker, `/tools`,
   status and hints, snapshot tests. First moment the feature is usable.
5. **Docs.** README: the "Your files, your rules" bullet, the commands table,
   the FAQ answer *Does moon edit files or run commands?* becomes *not unless
   you turn it on, and never commands*. `MOON.md` gets the new crate.
   `config.template.toml` gets the section, commented out.

## Out of scope for v1

- Any shell, network or process tool. Not planned. *(2026-09-24: a process
  tool exists after all, `run_command`, limited to the catalogue; a shell
  still does not, and still is not planned. See "Permissions and commands".)*
- `grep` / `glob` tools: v2, read-only, same sandbox.
- A second agent: the structure allows it; nothing needs it yet.
- Undo of applied edits from inside moon: git is the safety net, and moon says
  so when there is none.
- Tools in `moon ask`.
- Remembering approvals ("always allow edits to this file").
