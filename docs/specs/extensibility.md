# Extensibility: Skills, Memory, Agents, MCP

Status: design proposal. No code lands in this PR — implementation is
phased across follow-up branches, smallest first.
Related: `docs/specs/agentic-chat-ui.md` (the read-only capabilities
menu surface that this proposal makes editable).

## Goal

Let users create, edit, and delete their own **skills**, **memory**,
**agent profiles**, and **MCP servers** from inside edytlab — the way
Claude Code lets you drop a markdown file under `~/.claude/skills/`,
edit `CLAUDE.md`, point at a stdio MCP server, or define a subagent.

Today the dispatcher's built-in Rust tools are the only thing the
agent can reach for. Everything else in the `+` menu is a placeholder.
This proposal makes the four placeholder groups real.

## Non-goals (in this proposal)

- **Hot-reloading skill / agent definitions while a turn is in
  flight.** All four loaders snapshot at agent-rebuild time. Edits
  take effect on the next turn.
- **Plugin marketplaces.** Plugins are useful only once Skills, Agents,
  MCP, and Hooks each have their own surface; plugins are a *bundle*
  of those. We'll spec them in a follow-up proposal after the four
  primitives are live.
- **Real subagent orchestration.** Phase 1 ships agents as *profiles*
  (a saveable bundle of model + tool whitelist + system prompt body)
  that the user swaps into manually. Multi-agent delegation is its own
  effort.
- **Hooks.** Important, but they need a sandboxing story (the hook is
  arbitrary shell) and overlap with skills less than the other four.
  Specced in a separate doc once we settle the sandbox model.

## Filesystem layout

```
~/.edytlab/
├── settings.json               # global preferences (model, theme, …)
├── memory.md                   # global memory (Claude Code `~/.claude/CLAUDE.md` equiv.)
├── skills/
│   ├── mixing-glossary.md      # one skill per markdown file
│   └── mastering-loudness.md
├── agents/
│   ├── precision-editor.md     # one profile per markdown file
│   └── creative-mashup.md
└── mcp.json                    # MCP server registrations
```

Per-project (i.e. inside the user's edytlab project dir, which is the
folder the user opened via `open_project`):

```
<project>/
└── .edytlab/
    └── EDYTLAB.md              # project memory, takes precedence
```

The leading dot (`.edytlab/`) inside the project mirrors the
`.audiograph/` directory the M22 stem cache already uses, so VCS
ignore rules and "where do edytlab artifacts live?" stay consistent.

### Why filesystem (not a settings database)

- **User-editable in any editor.** Power users can `vim ~/.edytlab/skills/foo.md`
  without launching the app, which matches the Claude Code expectation
  and lets people commit skills into their dotfiles repo.
- **VCS-friendly.** Per-project skills under `.edytlab/` can be
  checked in; team-shared skills travel with the project.
- **Simple to back up / migrate.** Tarball the directory.
- **Diffable.** Markdown diffs review well; JSON-blob-in-sqlite does
  not.

## Skills

A skill is a unit of *additional system-prompt instructions* the agent
loads conditionally. Mirrors Claude Code's `SKILL.md` model.

### File format

```markdown
---
name: mixing-glossary
description: Domain glossary for typical mixing terminology — vocal lead, sidechain, bus, etc.
trigger: keywords
keywords: [mix, mixing, sidechain, bus, headroom, eq]
enabled: true
---

# Mixing Glossary

When the user uses the words below, prefer the definitions in this
glossary rather than guessing from context.

- **Lead vocal**: the most prominent vocal track.
- **Sidechain**: …

(remainder of the skill body — appended verbatim to the system
prompt when the trigger matches.)
```

Frontmatter fields:

| field | required | meaning |
| --- | --- | --- |
| `name` | no | display name shown in the capabilities menu. Defaults to the filename stem. If supplied, it MUST equal the stem — the loader rejects mismatches rather than silently picking one, so manual editors don't get burned by a frontmatter / filename split. The filename stem is always the canonical id. |
| `description` | yes | one-line summary shown in the capabilities menu |
| `trigger` | yes | `always` \| `keywords` \| `regex` |
| `keywords` | when `trigger=keywords` | flat array of substrings (case-insensitive) checked against the user message |
| `pattern` | when `trigger=regex` | a Rust regex; same case-insensitivity rule |
| `enabled` | no, default `true` | quick disable without deleting the file |

### Loader

`crates/skills/` (new crate). Public surface:

```rust
pub struct Skill {
    pub name: String,
    pub description: String,
    pub trigger: Trigger,
    pub body: String,
    pub source_path: PathBuf,
    pub enabled: bool,
}

pub enum Trigger {
    Always,
    Keywords(Vec<String>),
    Regex(regex::Regex),
}

pub struct SkillLibrary { /* … */ }

impl SkillLibrary {
    pub fn load_default() -> Result<Self>;            // scans ~/.edytlab/skills/
    pub fn load_from(dir: &Path) -> Result<Self>;     // for tests + project overlays
    pub fn matches(&self, ctx: &TriggerContext) -> Vec<&Skill>;
}

pub struct TriggerContext<'a> {
    pub user_message: &'a str,
    /// Previous user turns this conversation, oldest first. The loader
    /// concatenates these into a single haystack so a skill triggered
    /// on turn 1 stays active on turn 2's follow-up even if the
    /// keyword isn't repeated.
    pub history: &'a [String],
    /// Conversation mode detected by `agent_loop`, if any (`mashup`,
    /// `edit`, …). Skills can opt into a mode via a future
    /// `modes: [mashup]` frontmatter field; matching is OR-ed with
    /// keyword / regex triggers.
    pub mode: Option<&'a str>,
}
```

The agent loop calls `library.matches(&ctx)` once per turn and
concatenates each matched skill's `body` into the system prompt
*after* the base system prompt and *before* `SessionContext`. Skills
never override the base prompt — they only append.

Trigger matching against `history` is the mitigation for the "skill
flickers off on a follow-up turn" failure mode: once a skill has fired
in the conversation it stays sticky for the remainder of the
conversation (or until the user clears the transcript). The
implementation can shortcut by caching the matched-skill set per
conversation id and only re-evaluating when a fresh turn introduces
new keywords.

### IPC commands

```
list_skills() -> Vec<SkillSummary>
read_skill(name) -> SkillContent { frontmatter, body }
upsert_skill(name, content) -> ()
delete_skill(name) -> ()
```

`upsert_skill` validates the frontmatter, ensures `name` matches the
filename stem (or renames the file if not), and atomically rewrites
the file via `tempfile::NamedTempFile::persist`.

### Editor UI

A new **Settings → Skills** tab listing every skill with: name,
description, trigger summary, on/off toggle, edit, delete. Edit opens
a two-pane modal — frontmatter form on the left, markdown body
textarea on the right, with a live preview of the rendered system
prompt fragment. "New skill" creates a stub file with sensible defaults.

The existing `CapabilitiesMenu` (the `+` popover from PR #55) starts
displaying the skill list in its `Skills` group as soon as
`list_skills` returns. Toggling a skill in the popover writes
`enabled: false` to its frontmatter so the menu and the editor stay
the single source of truth.

## Memory

Two files, both markdown, both always-on:

- `~/.edytlab/memory.md` — global. Always loaded.
- `<project>/.edytlab/EDYTLAB.md` — per-project. Loaded only when a
  project is open. Precedence is "global first, then project" — the
  project file gets the last word.

Mirrors Claude Code's `CLAUDE.md` precedence rule (user > project).
The two files are spliced into the system prompt under a clearly
delimited section so the agent can quote them when explaining its
choices:

```
<edytlab-memory scope="global">
… contents of ~/.edytlab/memory.md …
</edytlab-memory>
<edytlab-memory scope="project">
… contents of <project>/.edytlab/EDYTLAB.md …
</edytlab-memory>
```

### IPC commands

```
read_memory(scope: "global" | "project") -> String
write_memory(scope, contents) -> ()
```

`write_memory` rejects `scope="project"` when no project is open.

### Editor UI

**Settings → Memory** tab with two collapsible sections (Global,
Project) and a markdown textarea each. Saves are debounced and
atomic. Empty files are allowed — write_memory just truncates.

## Agents (profiles only)

A profile is a saveable bundle of:

- `model` — provider + model id override (optional; falls back to the
  current global default).
- `tools` — whitelist of tool names. `null` = all tools.
- `system_prompt` — body of the profile, appended to the system prompt
  the same way skills are. Distinct from skills only by intent: a
  profile is "the agent's whole personality for this kind of work",
  a skill is "a slice of domain knowledge".

### File format

```markdown
---
name: precision-editor
description: Careful, asks before destructive edits. Audio-engineer voice.
model:
  provider: anthropic
  id: claude-opus-4-7
tools: [load, gain, set_track_gain, normalize, render_preview]
---

You are a precision audio editor. Confirm any operation that mutates
the session before running it. Prefer non-destructive ops (gain,
set_track_gain) over destructive ones (fade, reverse) where the
result is equivalent. …
```

Tools list of `null` (or omitted) means "all registered tools".

### IPC + agent integration

```
list_agent_profiles() -> Vec<AgentProfileSummary>
read_agent_profile(name) -> AgentProfileContent
upsert_agent_profile(name, content) -> ()
delete_agent_profile(name) -> ()
set_active_agent_profile(name | null) -> ()
get_active_agent_profile() -> name | null
```

When `set_active_agent_profile` is called with a non-null name, the
agent is rebuilt with:

- `LlmConfig.model` overridden to the profile's choice (if set).
- The dispatcher's `tool_schemas()` filtered to the whitelist before
  going to the model.
- The profile body appended to the system prompt.

`null` restores the default agent.

### Editor UI

**Settings → Agent profiles** lists every profile with: name,
description, model summary, tool-count badge, edit, delete, "use
this". The composer header gains a small profile-picker chip so the
user can swap profile per turn without going to settings.

## MCP servers

Real JSON-RPC client work. This is the biggest of the four — owns a
follow-up branch by itself.

### Registration file

`~/.edytlab/mcp.json`:

```json
{
  "servers": {
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": { "GITHUB_TOKEN": "<keychain:github_mcp_token>" },
      "enabled": true
    },
    "music-library": {
      "url": "https://mcp.example.com/sse",
      "enabled": true
    }
  }
}
```

Two transports: stdio (local subprocess) and SSE (remote URL). Secrets
in `env` use the `<keychain:slot>` placeholder; the value is fetched
from the OS keychain at server-launch time so secrets never live in
plain text on disk. Matches the Claude Code `.mcp.json` shape closely
enough that users can lift configs between the two.

### Runtime

`crates/mcp/` (new crate):

```rust
pub struct McpClient { /* … */ }
pub struct McpRegistry {
    clients: HashMap<String, McpClient>,
}

impl McpRegistry {
    pub fn load_default(keychain: &Keychain) -> Result<Self>;
    pub fn tools(&self) -> Vec<RemoteToolDescriptor>;
    pub async fn invoke(&self, server: &str, tool: &str, args: Value) -> Result<Value>;
}
```

The dispatcher gains a `register_remote(McpRegistry)` method that
extends `tool_schemas()` with the union of every server's tools.
Namespacing rules:

- The advertised name is `<server>__<tool>` when the combined length
  is ≤ 64 characters and matches `^[a-zA-Z0-9_-]{1,64}$` (the
  Anthropic Messages-API tool-name regex, which is the strictest
  constraint of the three providers we ship).
- When the combined name would exceed 64 chars, the loader truncates
  the prefix to `<server[0..N]>__<tool>` and appends a `_<8-hex>`
  blake3 suffix derived from `(server, tool)`, choosing `N` so the
  whole identifier is exactly 64 chars. The hash suffix is what
  guarantees collision-freedom when two long server names truncate to
  the same prefix.
- Characters outside the allowed regex (most commonly `.` in scoped
  npm packages) are replaced with `_` before length checks. The
  display name in the capabilities menu still shows the un-mangled
  `<server>::<tool>` form; the mangling is for the wire protocol
  only.
- `McpRegistry::tools()` returns `RemoteToolDescriptor { wire_name,
  display_name, server, tool }` so the dispatcher can translate both
  directions without re-deriving the mangling.

`invoke` dispatches to the right client transparently — tool callers
(and the model) don't know whether a call is local Rust or remote MCP.

Tool-call lifecycle events (`agent://tool-call`,
`agent://tool-call-end`) work identically for remote tools, so the
existing chat UI badge / chip / capabilities surfaces just work.

### IPC commands

```
list_mcp_servers() -> Vec<McpServerSummary>     # id, transport, status (running | stopped | error), tools count
read_mcp_server(id) -> McpServerConfig
upsert_mcp_server(id, config) -> ()
delete_mcp_server(id) -> ()
restart_mcp_server(id) -> ()
```

### Editor UI

**Settings → MCP servers** with: add, edit, delete, start/stop,
inspect tools. Edit form has transport tabs (stdio vs SSE) and a
secrets section that writes to the keychain rather than to disk.

## Agent loop changes (cross-cutting)

`crates/ai/src/agent_loop.rs::run_turn` already takes `dispatcher`,
`store`, `engine`. After this proposal lands it also takes:

```rust
pub struct AgentRuntime<'a> {
    pub dispatcher: &'a Mutex<ToolDispatcher>,  // existing
    pub store: &'a Mutex<session::Store>,       // existing
    pub engine: &'a Mutex<audio_engine::Engine>,// existing
    pub skills: &'a SkillLibrary,               // NEW
    pub memory: &'a MemoryCache,                // NEW
    pub profile: Option<&'a AgentProfile>,      // NEW
    pub mcp: &'a McpRegistry,                   // NEW (after MCP phase)
}
```

System-prompt assembly order, top-to-bottom:

1. Base system prompt (existing).
2. Profile body (if a profile is active).
3. Matched skills (in deterministic alphabetical order so identical
   inputs produce identical prompts).
4. Memory: global, then project.
5. SessionContext (existing).

Filtering rules:

- Tool whitelist: intersection of (a) capabilities-menu toggles from
  PR #55, (b) profile tools whitelist if active. Both must agree for
  a tool to be exposed; if either is empty the tool is hidden.

## IPC additions, all-up

Every new command is `#[tauri::command] async fn` returning
`CmdResult<...>`; same error-string boundary as the rest of
`commands.rs`. The TS bridge layer mirrors each in
`tauri-bridge.ts` with hand-aligned types — same convention used
today.

```
# skills
list_skills, read_skill, upsert_skill, delete_skill

# memory
read_memory, write_memory

# agent profiles
list_agent_profiles, read_agent_profile, upsert_agent_profile,
delete_agent_profile, set_active_agent_profile,
get_active_agent_profile

# mcp
list_mcp_servers, read_mcp_server, upsert_mcp_server,
delete_mcp_server, restart_mcp_server
```

## Phasing

Ordered smallest-blast-radius first. Each row is a PR.

| Phase | PR | Touches | Acceptance |
| --- | --- | --- | --- |
| 1 | **Memory editor** | new `crates/memory` (loader), `commands.rs` (2 cmds), Settings → Memory tab | Memory contents make it into the system prompt; round-trip edit + save works |
| 2 | **Skills loader + read-only menu** | new `crates/skills`, `list_skills` cmd, capabilities menu lists real skills, no editor yet | Drop a `.md` under `~/.edytlab/skills/`, restart, see it in the menu, see it injected when the trigger fires |
| 3 | **Skills editor** | `read_skill` / `upsert_skill` / `delete_skill`, Settings → Skills tab | Create / edit / delete from the UI, no file-system shell needed |
| 4 | **Agent profiles** | new `crates/agent_profiles`, full CRUD + active selection + agent rebuild plumbing, Settings → Agent profiles tab, composer profile-picker chip | Switching profile changes the model and tool whitelist on the next turn |
| 5 | **MCP servers** | new `crates/mcp`, dispatcher remote-tool registration, keychain secret resolution, Settings → MCP servers tab | Local stdio MCP server's tools appear in the menu and dispatch end-to-end |

Phases 1, 2, 3 are small enough that two could combine if reviewers
prefer. Phase 4 stands alone — it modifies the agent build flow.
Phase 5 stands alone — it introduces a new transport.

## Risks and open questions

- **Trigger keyword false positives.** A skill keyed on `"mix"` will
  fire on `"mixed feelings"`. The mitigation is the `regex` trigger
  type for users who care, plus an `enabled` toggle for
  damage-control. We can add a `min_keyword_length` knob later if
  this proves painful.
- **MCP secrets in plaintext on disk.** Solved by the `<keychain:…>`
  placeholder; the editor never lets the user type a raw secret into
  `env`, it always routes through the keychain dialog.
- **Profile model overrides bypass `get_active_provider`.** The
  composer's existing provider/model picker becomes confusing if a
  profile is active. We resolve this by greying out the global model
  picker when a profile pins a model, with a "clear profile" affordance.
- **Hot-reload semantics.** "Edits take effect on the next turn" is
  simple but means a long-running turn sees stale skills. Acceptable
  for v1; the design doesn't preclude a later watcher-based reload.
- **Per-project memory and per-project skills.** This proposal only
  has *memory* as a per-project file. We may want per-project skills
  too (a project ships a `mixing-glossary.md`). Easy to bolt on later:
  `<project>/.edytlab/skills/*.md` merged on top of the global
  library, project wins on name collisions.

## Acceptance gates for the implementation phases

Each phase PR must, before merge:

- `cargo fmt --all -- --check` clean
- `cargo clippy --workspace --all-targets -- -D warnings` clean
- `cargo test --workspace` passes; new tests for the loader, the
  prompt-assembly order, and the IPC command happy paths
- `pnpm --filter @edytlab/desktop test` passes; new editor UI tests
- `pnpm --filter @edytlab/desktop exec tsc --noEmit` clean
- The PR description names the slot of system-prompt assembly the
  change touches, so reviewers can verify prompt-order invariants
