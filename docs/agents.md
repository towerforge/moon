# Agents: a prompt and its permissions, by name

*Design, 2026-09-25 · reshaped on 2026-09-28 (one layer) and 2026-09-29
(every agent a file) into the model below, which is what the code does. It
rests on [harness.md](harness.md) and keeps every one of its rules. How to
use agents day to day is in [tools.md](tools.md#agents); this document is
the model and its reasons.*

Everything the model does in moon goes through a **named agent**, and an
agent is two things: a prompt, and its permissions, **whole**. Choosing an
agent is choosing what the model may do. There is no second layer to
reconcile and no second place: **an agent is one file** in
`~/.config/moon/agents/`, and what its panel shows is what its file says and
what the model gets.

- **`default.toml`** is where every conversation starts. `moon config init`
  writes it with reading on and nothing else; moon writes it again at
  startup if it is missing, so there is always an agent to run through.
- **`reviewer.toml`** is a sample `moon config init` writes: reading plus the
  read-only git commands, and a prompt that reports and never writes. Once a
  file, it is yours to change or delete.
- **Any other file** is an agent of yours, made and edited from `/agent`
  without leaving moon, or by hand.

## What an agent is

Two types, so the harness's surface did not change: `AgentDef` is an agent as
it is defined, `Agent` is the one the harness runs.

```rust
pub struct AgentDef {
    pub name: String,              // the file name
    pub description: String,       // one line, shown in the picker
    pub prompt: Option<String>,    // its own; None = moon's, the editor's or the reader's
    pub policy: Policy,            // its permissions, whole
    pub max_steps: Option<usize>,  // its own step limit; None = 8
}

impl AgentDef {
    pub fn agent(&self) -> Agent;  // the runtime agent
}
```

There is no rule to apply: the policy is the file's. It went through
`Policy::from_pairs`, so unknown ids are dropped and the coupling rule holds
(editing and creating need reading).

An agent is a value, not behaviour. It has no tools, no sandbox and no
configuration of its own: the five tools, the catalogue and every check in
[harness.md](harness.md) are shared by all agents, and an agent can neither
name, widen nor get around them. A capability outside the catalogue cannot be
written into an agent file, because the catalogue is in the code.

`editor` and `reader` are not agents you pick: they are moon's own two
prompts, given to any agent without a prompt of its own — the editor's when
the policy may change files, the reader's otherwise (`Agent::for_policy`).

## Agent files

```
~/.config/moon/agents/<name>.toml
```

The file name is the agent's name (`committer.toml` → `committer`): lowercase
letters, digits, `-` and `_`. Ids as the panel shows them, `off · ask ·
allow` as values:

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
"git status" = "allow"
"git diff"   = "allow"
"git add"    = "ask"
"git commit" = "ask"
```

- Everything is optional. No `prompt` means moon's own; no `max_steps` means
  8; an empty file is an agent with nothing on.
- `[permissions]` is the whole truth: what it lists is what the agent may do,
  what it leaves out is off.
- An `inherit` line from the two-layer design (below) still parses and is
  ignored.
- A file that does not parse is reported and skipped, and never rewritten;
  the rest still load. `default.toml` broken means `default` runs with
  nothing on until it is fixed.
- The folder is read at startup, when the picker opens and before every
  message, so a file edited by hand counts from the next message.
- **moon writes a file whole**: every capability of the catalogue in its
  group, `off` included, with its help as a comment, so the file shows
  everything there is to set. It does so on every step of the panel, and at
  startup for any file that names fewer capabilities than there are or one
  the catalogue no longer knows (`AgentFile::stale`, `sync_dir`) — that is
  how an update that adds commands reaches every agent, said once
  (`agents/committer.toml brought up to the catalogue · 3 new`). Comments of
  your own do not survive it.
- `ensure_default` writes `default.toml` when it is missing: from a
  `tools.toml` left next to the folder or the old `[tools]` keys of
  `config.toml` if there are any (read once, there), else reading on.
- `moon config reset` writes `config.toml`, `default.toml` and
  `reviewer.toml` again as they ship, asking first; the other agents stay.

`AgentFile::template()` is what a new agent starts from: `read files` on,
`max_steps = 8`, and a placeholder description and prompt.

### Trust

**An agent file grants permissions.** That is acceptable only because the
agents folder lives next to `config.toml` and carries the same trust: it is
the user's own configuration and nothing else writes to it. Project agents (`.moon/agents/` inside a repository) are therefore not a
small step: a cloned repository — or the model itself, with creating files
on — could grant itself anything in the catalogue. If they ever come, they
must not load without an explicit confirmation that shows the file.

## Managing agents: `/agent`

`/agent` opens a docked picker like `/model`:

```
 Agent                                                               3 agents
 a prompt and its permissions, whole: choosing the agent is choosing them

 ❯  1. default ✓          runs on your permissions: the editor when it may write…
    2. reviewer                    reads the project and reviews changes; never writes
    3. committer                     stages and commits what you approve; edits nothing

 ↑↓ move · enter select · ctrl+a new · ctrl+t permissions · ctrl+e prompt · ctrl+r rename · ctrl+d delete · esc close
```

Typing filters the list, and the row numbers jump, as in every picker.

| Key | What it does |
|---|---|
| `enter` | chooses the agent for the conversation and closes; it applies from the next message |
| `esc` | closes without changing anything |
| `ctrl+t` | the highlighted agent's permissions panel (below) |
| `ctrl+e` | its description and prompt, edited in moon (below) |
| `ctrl+a` | a new agent: asks for a name, writes the template and lands on its permissions panel |
| `ctrl+r` | renames it; the name is the file, and a conversation that chose it follows |
| `ctrl+d` | deletes the file, after a confirmation; if it was the chosen agent, the conversation falls back to `default` |

Every agent is a file, so every one is edited the same way. The one
exception is `default`, which every conversation starts with: rename and
delete refuse it with a word.

`/tools` stays for a while as an alias: it opens the permissions panel of the
agent the conversation is using.

### The permissions panel (`ctrl+t`)

The same two-level panel moon always had, one per agent. First the groups
(Editor, Files, Git, Build, Docker, Network), one row each with a summary of
what this agent may do in it, and the step limit on `Editor`:

- `enter` opens a group; `←`/`→` turn the whole of it off, or on with its
  defaults.

Inside a group, one row per capability with `◀ off · ask · allow ▶`:

- `↑↓` move; `←→` walk the values and stop at the ends (`commands in
  subfolders` is only `off` or `allow`); `enter` or space toggle between off
  and the catalogue's default for the row.
- On `max steps per message`, a digit sets the number (1 to 20).
- `esc` goes back to the groups; another `esc` back to the picker, or closes
  when the panel came from `/tools`.

**Every step is written at once** to the agent's file. There is nothing to
save or cancel. A program that is not installed is dimmed and refuses to
turn on.

### The prompt editor (`ctrl+e`)

A docked panel with two fields, the description and the prompt, in the same
input box the conversation uses. moon never launches an external editor.

- `tab` switches field; `enter` is a new line in the prompt and does nothing
  in the description, which is one line.
- `esc` saves on its way out (or just closes, with nothing changed); `ctrl+s`
  saves too.
- `ctrl+c` discards; with unsaved changes the first one warns
  (`unsaved changes · ctrl+c again to discard`) and the second one discards.
- An empty field is not saved: moon says why and the editor stays open.
- Saving the chosen agent rebuilds it; the new prompt applies from the next
  message.

## Choosing an agent

- **The marker** at the left of the bottom row always names the agent,
  `default` included: `⏵⏵ reviewer · Read · 5 commands` while it has
  permissions, `⏵ default · all off` when it has none.
- **The tools switch follows the chosen agent's policy**
  (`refresh_agent_state`): anything on turns the tools on, nothing on turns
  them off. Nothing is written anywhere by choosing.
- **The step limit** is the agent's `max_steps`, or 8 when it has none.
- **The session remembers it**: the choice is written in the session's meta
  line (`agent = "committer"`, only when it is not `default`, so every older
  session reads back the same). On resume, a name whose file is gone or broken
  falls back to `default` with a notice.
- `/context` names the agent and its description when it is not `default`.

`moon ask` has no agents: it has no terminal to approve in, and gets no tools.

## The prompt

What the model receives, in order:

```
base system prompt · MOON.md · live attachments
  → the agent's prompt            (its own, or the editor's / reader's)
  → what happens to writes         (from the policy — Agent::system_prompt)
  → the commands it may run        (from the policy, or "you have no shell")
```

The last two parts are always generated from the policy that is in force and
never come from the file: an agent's prose cannot claim a capability, because
the only sentences that state capabilities are written by `system_prompt()`
from what actually applies.

## Where the code lives

| Path | What it holds |
|---|---|
| `crates/agent/src/agents/mod.rs` | `Agent`, `AgentDef` (`agent`, `factory`, `builtin` for a run without a folder), `DEFAULT_AGENT`, `DEFAULT_STEPS` |
| `crates/agent/src/agents/editor.rs` | the editor's and the reader's prompts, moon's own |
| `crates/agent/src/agents/reviewer.rs` | the `reviewer` as `moon config init` writes it |
| `crates/agent/src/agents/file.rs` | `AgentFile`: parsing, `template`, `factory`, `to_toml` (the whole catalogue), `stale`, `def`; `ensure_default`, `sync_dir`, `defs_from_dir` |
| `crates/core/src/session.rs` | `SessionMeta.agent` |
| `crates/tui/src/app/agent.rs` | reloading and choosing agents, `rebuild_agent`, `refresh_agent_state`, the picker and its actions, the approval panel |
| `crates/tui/src/app/perms.rs`, `view/perms.rs` | the permissions panel: its two levels, and where each step is written |
| `crates/tui/src/app/agent_prompt.rs`, `view/agent_prompt.rs` | the prompt editor |
| `crates/tui/src/app/status.rs` | the marker and the panel hints |

## How it got here

The design went through several shapes in four days. They are gone from the
code; they are recorded so they are not proposed again without knowing why
they went.

- **Two layers: `effective = min(user, agent)`** (2026-09-25). An agent could
  only restrict what `/tools` granted, never widen it, and an `inherit` key
  said whether the capabilities it did not name followed yours or were off.
  Safe, but shown as a three-column table (`agent · yours · runs`) it was
  incomprehensible in use — "if I don't grant it, why does the agent ask for
  it?" — and since every conversation runs through an agent anyway, one
  column is enough. Replaced on 2026-09-28 by the one-layer model above,
  which is why agent files now grant and why project agents need a gate.
- **`ctrl+o` opening the file in `$EDITOR`** (2026-09-25). Removed the same
  day: moon does not launch programs for the user. The prompt is edited in
  moon itself instead (`ctrl+e`).
- **`/tools` focusing the chosen agent, `ctrl+t` jumping between its side and
  your grants** (2026-09-25). Dropped with the two layers.
- **`/agent` as a full-screen view** (2026-09-28), with a list and a detail in
  three sections, bare-letter keys, and later lazydocker-style bordered
  frames with two levels of focus. Three variants were built; all were
  reverted to the docked picker. What survived is the permissions panel and
  the prompt editor.
- **Hidden edit modes and editing raw TOML on screen.** Rejected at the design
  stage: moon's editing surfaces are cells, visible panels and one-line
  dialogs, never an invisible mode or free text where a cell fits.
- **No `ctrl` keys at all.** Tried because some combinations did not reach
  one terminal; the `ctrl` keys came back. When a specific combination fails,
  give it a synonym instead.
- **Built-in agents and `tools.toml`** (2026-09-28). `default` and `reviewer`
  lived in the binary and `default`'s permissions in `tools.toml`, a leftover
  of the two-layer days. Looking at the folder showed one file where three
  agents were expected, and permissions in two places. Replaced on
  2026-09-29 by the model above: every agent a file, `tools.toml` gone (read
  once into `default.toml`), and `moon config reset` for a fresh start.

## Open decisions

- **Project agents** (`.moon/agents/`): are they wanted at all? If so, the
  confirmation gate described under *Trust* comes first.
- **A `model = "…"` per agent**: left out on purpose — it ties agents to
  providers, and `/model` already exists.
- **Delegation**: one agent calling another. Not planned. If it comes, the
  shape would be a `Tool::Agent { name, task }` offered to agents that declare
  it; the harness, still without I/O, hands back a `Command::Delegate` and
  pauses; the interface runs a second `Harness` on the same sandbox, depth 1,
  no parallelism, its approvals titled with the agent's name. With agents
  carrying their permissions whole, what the called agent may do needs a rule
  of its own — probably no more than the caller.
