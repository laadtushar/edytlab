# edytlab — System Architecture

> **Audience:** Engineers contributing to the core, extending the tool surface, or adding LLM providers.
> For a product-level overview start with the [root README](../README.md).

---

## Table of Contents

1. [High-Level Architecture](#1-high-level-architecture)
2. [Rust Workspace Layout](#2-rust-workspace-layout)
3. [Frontend (Tauri Shell)](#3-frontend-tauri-shell)
4. [AI Subsystem](#4-ai-subsystem)
5. [Tool Dispatch System](#5-tool-dispatch-system)
6. [Session Graph (DAG)](#6-session-graph-dag)
7. [Audio Engine](#7-audio-engine)
8. [ML Pipeline](#8-ml-pipeline)
9. [Memory and Skills](#9-memory-and-skills)
10. [Agent Profiles and MCP Servers](#10-agent-profiles-and-mcp-servers)
11. [IPC and Event System](#11-ipc-and-event-system)
12. [Security Model](#12-security-model)
13. [Data Flow: Single User Turn](#13-data-flow-single-user-turn)
14. [Extension Points](#14-extension-points)

---

## 1. High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│  PRESENTATION LAYER  (Tauri WebView — React 19 + Vite 7 + Tailwind 4)      │
│                                                                             │
│  ┌──────────────┐  ┌───────────────┐  ┌──────────────┐  ┌───────────────┐ │
│  │  Timeline    │  │  Chat Panel   │  │  GraphView   │  │   Settings    │ │
│  │  (WaveSurfer)│  │  (streaming)  │  │  (@xyflow)   │  │   (all CRUD)  │ │
│  └──────────────┘  └───────────────┘  └──────────────┘  └───────────────┘ │
└─────────────────────────────────┬───────────────────────────────────────────┘
                                  │  tauri::command (IPC) + Tauri events
┌─────────────────────────────────▼───────────────────────────────────────────┐
│  APPLICATION LAYER  (apps/desktop/src-tauri — Rust)                        │
│                                                                             │
│  commands.rs (~90 commands)  ·  state.rs (AppState)  ·  events.rs          │
└───────────┬──────────┬────────────────────┬──────────────┬──────────────────┘
            │          │                    │              │
     ┌──────▼──┐  ┌────▼────┐  ┌──────────▼───┐  ┌──────▼──────┐
     │  crates/│  │ crates/ │  │    crates/   │  │   crates/   │
     │  ai     │  │ tools   │  │    session   │  │   audio-*   │
     │  agent  │  │ 94 tools│  │    DAG store │  │   ml-*      │
     └──────┬──┘  └────┬────┘  └──────────┬───┘  └──────┬──────┘
            │          │                  │              │
     ┌──────▼──────────▼──────────────────▼──────────────▼──────┐
     │  SHARED STATE  (Arc<Mutex<_>> in AppState)                │
     │  Store · Engine · Agent · Clipboard · PlanNotify          │
     └───────────────────────────────────────────────────────────┘
                               │
     ┌─────────────────────────▼──────────────────────────┐
     │  EXTERNAL SERVICES (network only for LLM tokens)   │
     │  Anthropic · OpenRouter · OpenAI · Groq · Gemini   │
     │  Ollama — a local daemon by default, no key        │
     └────────────────────────────────────────────────────┘
```

### Design Invariants

| Invariant | Where Enforced |
|-----------|---------------|
| Audio bytes never leave the machine | Engine runs 100% in-process; ONNX models are local |
| API keys never touch edytlab servers | Keys go directly to provider endpoints |
| Every edit is non-destructive | Session DAG — original files are read-only |
| All tool calls are deterministic | Tools mutate `SessionState` only, no side effects outside the store |
| Concurrent access is deadlock-free | Store lock dropped before Engine lock (documented in CLAUDE.md) |

---

## 2. Rust Workspace Layout

```
edytlab/
├── Cargo.toml                    # Workspace root — resolver = "2", edition 2021
├── rust-toolchain.toml           # Pinned toolchain: 1.88 + rustfmt + clippy
├── apps/
│   ├── desktop/src-tauri/        # Tauri 2 shell
│   └── cli/                      # edytlab-cli: one headless agent turn (E2E tests, smoke runs)
└── crates/
    ├── ai/                       # LLM abstraction, agent loop, keychain
    ├── agent_profiles/           # Per-session model + tool-whitelist profiles
    ├── audio-analysis/           # BPM, key, beat-grid, transient detection
    ├── audio-decoder/            # symphonia-based file decode (WAV, MP3, FLAC, Ogg Vorbis)
    ├── audio-dsp/                # Sample-level DSP shared by tools and renderer (no deps)
    ├── audio-engine/             # DSP graph + offline render
    ├── audio-time/               # Pitch-shift / time-stretch primitives (Phase 2)
    ├── mcp/                      # MCP server lifecycle + JSON-RPC dispatch
    ├── memory/                   # Global/project markdown memory fragments
    ├── ml-demucs/                # Stem separation via ONNX Demucs (inference is a stub, #385)
    ├── ml-pipeline/              # Shared ONNX runtime gate, model downloader (no callers yet) and inference cache (the installers carry the runtime, #383)
    ├── ml-whisper/               # Transcription via ONNX Whisper-base (decoder is a stub, #384)
    ├── recorder/                 # Microphone capture to WAV (cpal)
    ├── session/                  # DAG data model, node store, fork/diff/compare
    ├── skills/                   # User skill library with trigger evaluation
    └── tools/                    # 94 deterministic audio-editing tools
```

### Dependency Graph (simplified)

```
apps/desktop/src-tauri
    ├── crates/ai
    │   ├── crates/session
    │   └── crates/tools
    │       ├── crates/audio-engine
    │       │   └── crates/audio-decoder
    │       ├── crates/ml-demucs
    │       │   └── crates/ml-pipeline
    │       └── crates/ml-whisper
    │           └── crates/ml-pipeline    (the ONNX Runtime gate only; it builds its own `ort` session)
    ├── crates/recorder           (cpal input)
    ├── crates/memory
    ├── crates/skills
    ├── crates/agent_profiles
    └── crates/mcp
```

**Key principle:** `session` and `audio-engine` have no dependency on `ai` — the AI layer is a consumer, not a foundation.

---

## 3. Frontend (Tauri Shell)

### Technology Stack

| Layer | Technology |
|-------|-----------|
| Runtime | Tauri 2 (Rust + WRY WebView) |
| UI framework | React 19 with concurrent features |
| Build tool | Vite 7 |
| Styling | Tailwind CSS 4 |
| Waveform | WaveSurfer.js 7 |
| Graph view | @xyflow/react 12 |
| Animations | CSS transitions on shared tokens (no animation library — see [motion-audit.md](./motion-audit.md)) |

### Component Tree

```
App.tsx
├── AppHeader
│   ├── Logo + project path
│   ├── PlaybackControls (transport)
│   └── SettingsGear → Settings modal
├── MainContent (split pane)
│   ├── LeftPane (one view at a time — `LeftView` in src/lib/views.ts)
│   │   ├── Timeline (wavesurfer, tracks, markers, playhead)
│   │   │   ├── Ruler (a click opens the label lane's name box)
│   │   │   ├── MarkerLayer
│   │   │   └── LabelLane (below the tracks, passed in as `belowLanes`)
│   │   ├── TranscriptPane
│   │   └── GraphView (xyflow DAG visualization)
│   └── RightPane
│       └── Chat
│           ├── MessageBubble[]
│           │   └── ToolBadge[]
│           ├── ThinkingIndicator
│           ├── EmptyState (no audio loaded)
│           └── ChatInput + CapabilitiesMenu
├── ABCompareBar (when compareMode active; lists what each side's render leaves out)
├── ShortcutsOverlay (? key)
├── ErrorBanner (API key / render errors)
└── RenderWarningsNotice (stored effects a render leaves out: an ErrorBanner in its warning tone)
```

The tree is conceptual: `PlaybackControls`, `MainContent`, `LeftPane`/`RightPane` and `ChatInput` are regions of `App.tsx`, `AppHeader.tsx` and `Chat.tsx`, not components of their own.

### State Management

All session state lives in Rust (`AppState`). The frontend is intentionally thin — it:
1. Sends commands via `tauri-bridge.ts`
2. Receives Tauri events (`agent://text-delta`, `agent://tool-call`, `agent://node-created`, `agent://done`, …)
3. Derives local UI state from the command responses

Local React state: `head`, `audioPath`, `rendering`, `leftView`, `compareMode`, `markers`, `tracks`, `showShortcuts`, `selection`. Most of it is not persisted; where you were — head, zoom, selection and playhead — is saved to `<project>/.audiograph/view.json`, and the zoom, selection and playhead are restored on open (`src/lib/viewState.ts`, `src-tauri/src/project.rs`). The saved head is not: it can lag the store's `HEAD`, and obeying it would move `HEAD` back on disk, so opening a project (and launching) keeps the head the store reports (#507).

### Tauri Bridge (`src/lib/tauri-bridge.ts`)

Type-safe wrapper around `@tauri-apps/api/core`. Every command and event has a TypeScript signature that mirrors the Rust return type. See [API Reference](./api-reference.md) for the full catalogue.

```typescript
// All commands return Promise<T> and throw on Rust Err(_)
await bridge.setApiKey("sk-ant-...");
const head = await bridge.getSessionHead();

// Events use unlisten pattern
const unlisten = await bridge.onTextDelta((chunk) => {
  appendToMessage(chunk);
});
// later:
unlisten();
```

### WaveSurfer Quirks

- `wsRef.current.zoom()` throws `"No audio loaded"` until `duration > 0`. Always guard:
  ```typescript
  if (!wsRef.current || duration === 0) return;
  ```
- `onWheel` JSX prop is passive in Chromium/Tauri. Use `addEventListener("wheel", handler, { passive: false })` via `useEffect` for Ctrl+scroll zoom.
- Multiple `window.addEventListener("keydown")` handlers do **not** stop each other via `e.stopPropagation()`. Guard with a state flag instead.

---

## 4. AI Subsystem

### Core Types

```rust
// crates/ai/src/lib.rs

pub struct LlmConfig {
    pub provider: Arc<dyn LlmProvider>,
    pub api_key: String,
    pub model: String,
    pub base_url_override: Option<String>,
    pub effort: Option<Effort>, // reasoning effort; read via effective_effort()
}

pub enum AgentEvent {
    TextDelta(String),
    ToolCallStart { name: String, id: String },
    ToolCallEnd   { id: String, ok: bool, view: Option<ToolView> },
    ToolCallNotRun { id: String },   // announced, never dispatched: not a failure
    NodeCreated(NodeId),
    Done,
    Plan { steps: Vec<serde_json::Value> },
    PlanRejected,
    PlanUnavailable { reason: String, first_edit_held: bool },
}

pub struct Agent {
    cfg:          LlmConfig,
    http:         reqwest::Client,
    dispatcher:   Arc<Mutex<ToolDispatcher>>,
    store:        Arc<Mutex<Store>>,
    engine:       Arc<Mutex<Engine>>,
    clipboard:    Arc<Mutex<Option<Vec<f32>>>>,
    conversation: Vec<Message>,
    plan_notify:  Arc<Notify>,
    memory:       Option<Arc<MemoryStore>>,
    skills:       Option<Arc<Mutex<SkillLibrary>>>,
    profile_body: Option<String>,
    tool_whitelist: Option<Vec<String>>,
}
```

### Agent Turn Lifecycle

```
User text
    │
    ▼
┌───────────────────────────────────┐
│  Build system prompt              │
│  · Base instructions              │
│  · Memory fragments (global +     │
│    project, if any)               │
│  · Matching skill bodies          │
│  · Active profile body            │
└─────────────┬─────────────────────┘
              │
    ┌─────────▼─────────┐
    │  POST to provider  │◄─── LlmProvider::serialize_request()
    │  endpoint (SSE)    │     LlmProvider::parse_stream_chunk()
    └─────────┬──────────┘
              │  stream chunks
    ┌─────────▼──────────────────────┐
    │  Tool call loop (max 20/turn)  │
    │  1. Collect tool_use blocks    │
    │  2. Dispatch to ToolDispatcher │
    │  3. Append tool_result         │
    │  4. Re-request if more calls   │
    └─────────┬──────────────────────┘
              │
    ┌─────────▼──────────────────────┐
    │  Emit AgentEvents via Tauri    │
    │  (text deltas + tool events)   │
    └────────────────────────────────┘
```

### Tool budget

A turn may make at most `MAX_TOOL_CALLS_PER_TURN` (20) tool calls, counted over
every step of the turn. It is protection against a model that never stops
calling tools, not an error condition: one sensible mastering pass is 7 to 10
calls. The budget is stated to the model in its system prompt (one line, built
from the constant, appended to the mode's base prompt), so it can plan within it.

When a step's calls would go past the budget, the loop does not run that step.
Each of its `tool_use` blocks is answered with an `is_error` `tool_result` saying
it was not run because the budget was reached (and `ToolCallNotRun` is
emitted for each, so no badge is left "running" and none reads as a failure). It then makes **one last
request with tools off** (`tool_choice: none`; the tool definitions stay in the
request because the history holds tool calls, which Anthropic rejects without
them) so the model says what was done and what is left. That text streams as
normal and the turn ends with `Done`, not an error. Edits already made stay as
ordinary undoable nodes. If the last request itself fails, the turn ends with that
request's error.

The check comes before the Plan first gate below, so a step that cannot run is
never shown for approval.

### Plan first

With **Plan first** on, no edit runs without approval. The approval is a
`Plan` event, answered by `approve_plan` / `reject_plan`, and which one is shown
depends on whether the model wrote a plan:

| Situation | What happens |
|-----------|--------------|
| The model writes a plan | The plan card is shown; the turn waits. Approving lets every edit run, as before. |
| No plan, with Plan first on (the model did not write one, or planning failed in any way) | `PlanUnavailable { first_edit_held: true }`, then the turn goes on. The first model step with a call that would change the session is **held before any of it dispatches**, and its concrete tool calls are shown on the same card. Approve: that step runs and the rest of the turn is ungated. Decline: nothing runs, each held call reads "not run" (`ToolCallNotRun`), the model is told the user declined, and the turn ends with `PlanRejected`. Edit the descriptions: nothing runs, each held call reads "not run", and the model is given the revision (each step as `tool — description`) and proposes again, which is held in turn. |
| No plan, mashup request, Plan first off | `PlanUnavailable { first_edit_held: false }` and the turn proceeds with no gate. |
| A turn whose calls only read the session, or only fill the app's caches (`audition_effect`, `render_preview`) | Never held. |

With no answer within five minutes nothing runs, each held call is answered "not
run" (`ToolCallNotRun`), and the turn ends with `PlanTimeout`.

The toggle survives a restart (#569). `set_plan_first` applies the choice and
writes it to `~/.edytlab/settings.json` (`src-tauri/src/settings.rs`, beside
`recents.json` and `mcp.json`), replacing the file atomically and keeping any
field it does not know; setup reads it before the window can ask for anything,
and `get_plan_first` tells the chat box what is in force. A missing, unreadable
or corrupt file means off and never blocks launch. A write that fails still
applies the choice until the app quits, and `set_plan_first` reports it. It is
not in the keychain, because the keychain may be waiting on an unlock prompt
when the agent is built (#497).

"Would change the session" is `Tool::mutates()`, which defaults to `true`: a new
tool, and every MCP tool, is held unless it is deliberately marked read-only.
A tool that writes only the app's own caches is not a mutation, where a cache is
content-addressed, bounded and rebuildable: `.audiograph/previews/` and
`.audiograph/auditions/` (both `PreviewCache`, size-capped, evicting their least
recently used entries) and `derived/track-<hash>.wav` (no size cap; at most one
per clip list a node holds, and the reclaim sweep removes the ones only history
holds). That is why `audition_effect` and `render_preview` run without a card. A
tool that leaves any other file, such as a temp file that outlives the call, is on
the other side of the line, so `render_preview` stores even a ranged render in the
cache (under a key that names the range) and fails rather than write elsewhere
(#535). One more thing a read-only tool may write is audio put back:
`render_preview` of a node whose audio a history sweep removed first rebuilds it
(`rederive::materialize`), which writes `derived/<hash>.wav` files for that node
and the history it is rebuilt from. Each is byte-identical to the file that was
there, under the name the node already references, so the project holds nothing it
did not already name; the reclaim sweep bounds them, not a cap.
`crates/tools/tests/read_only_tools.rs` pins the read-only list by name, so
widening it is a reviewed change, and checks that each tool on it leaves nothing
in the project but cache entries and audio put back (exactly the files a swept
node names), and names only cache entries in its result.
`ToolDispatcher::would_mutate` answers for a specific call and shares its
pre-dispatch checks with `invoke`, so a call that would be refused (turned off,
unknown, invalid arguments) is not held.

The shared approval path (arming the gate, the five-minute timeout, the card's
steps, the bookkeeping for a step that does not run) is
`crates/ai/src/approval.rs`.

### Request size and small-context models

A request is the system prompt, the tool schemas and the conversation. For a
one-line message the tool schemas are nearly all of it, which a local model
with a small context cannot take: the native end-to-end run sent "Make track 0
louder by 6 dB." and a local server with an 8,192-token context refused it
(#395). `crates/ai/tests/request_size.rs` drives the real `Agent` against a mock
server, takes the first streaming request, and prints its parts. Read them with

```
cargo test -p ai --test request_size -- --nocapture
```

On v0.6.0 (`d7fe4d5`, 2026-10-16). The full list has grown with the tools since the slim set landed (#479), when it was 54 KB for Anthropic and 57 KB as chat-completions JSON; the slim request has not:

| Request | System prompt | Tool schemas | Total |
|---------|---------------|--------------|-------|
| Anthropic (every tool) | 1.2 KB | 62 KB | 63.4 KB |
| A chat-completions provider sent every tool | 1.2 KB | 64.7 KB | 66 KB |
| Ollama (slim set, with the names line) | 2.3 KB | 6.3 KB | 8.7 KB |

The system prompt is a kilobyte, so the fix is fewer and terser tools, and only
for the provider that needs it. `LlmProvider::tool_set()` says which:

- **`ToolSet::Full`** (the default, and every hosted provider): every tool the
  turn permits, as the dispatcher lists them. **Anthropic must stay `Full`.**
  Its prompt cache keys on the whole tools-plus-system prefix, so a tool list
  that varied with the message would make every message a cache miss. That path
  is not touched. `request_size.rs` pins that Anthropic is sent every registered
  tool with the `cache_control` breakpoints, and ratchets the system prompt and
  the tool schemas separately, so growth shows up as a failing test whose
  message says to raise the constant deliberately.
- **`ToolSet::Slim`** (Ollama): `crates/ai/src/tool_selection.rs` builds the
  list once per turn from the tools the turn permits (so a disabled tool is
  never brought back), in this order of priority:
  0. `PINNED_WHEN_BLOCK_CUT` (`list_session`), only when the session block had
     to cut a list (see "The session block has a budget" below). It is sent
     outside the budget, so it takes nothing from the rest, and nothing is
     pinned for a session that fits;
  1. tools the user's message names, by name, by a distinctive word of it
     ("reverb", "pitch it up", "reversed") or by what people say instead
     ("remove the reverb" for `remove_effect`, "less harsh" for `de_esser`,
     "export each track separately" for `export_multiple`), the first
     mentioned first. Words that appear in many tool names, such as `track` or
     `region`, name none. The tools an approved plan names count here too, and
     a plan step's name is split at underscores, so `add_reverb` brings in
     `reverb`;
  2. the core list `SLIM_CORE_TOOLS` (the edits a message does not name: cut,
     pan, undo, export, gain);
  3. tools the assistant's latest message names, so "yes, do it" after "I could
     add a limiter or a de-esser" is shown those two;
  4. tools the system prompt points at in backticks (a profile or a matched
     skill);
  5. tools the model has already called in the conversation;
  6. tools the user named in earlier messages, the latest message first.

  They are added while the compacted descriptors stay within
  `SLIM_TOOLS_BUDGET_BYTES`, less what the session block takes past its
  allowance (see "The session block has a budget" below), then sorted by name. If everything permitted
  already fits (a small agent profile), all of it is sent. Each description is
  cut to its first sentence and the keywords the model can do without
  (`default`, `examples`, `title`, `additionalProperties: false`) are dropped;
  names, types, enums, bounds and `required` are kept. A tool whose
  description says it is not implemented in this build (`separate_stems`,
  `transcribe`) is never selected and never listed: a small model that is
  offered one calls it, and it can only fail. The same goes for the tools
  that only work on a transcript (`NEEDS_TRANSCRIPT_TOOLS`: `cut_words`,
  `remove_fillers`), because the only thing that makes a transcript is
  `transcribe`. `duck_under_speech` is not one of them: with `voice_tracks` it
  keys on a track's audio and needs no transcript, and a test fails if a tool
  with a `voice_tracks` property is put on that list. A test fails when
  `transcribe` stops being a stub, and says to delete that list.

  **The names line.** A model that is not shown a tool does not know it
  exists, so the permitted tools that were not sent are named, names only, in
  one line at the end of the system prompt ("More tools you can call by exact
  name…"). It comes from the same whitelist-filtered list as the selection, so
  a tool the profile or the Capabilities menu took away is in neither. The
  built-in tools come first and are always all listed (they come to about
  1 KB). The MCP tools follow, in name order, while the names stay within
  `SLIM_NAMES_BUDGET_BYTES` (1,614 B), and the line then ends "and N more",
  which counts MCP tools only. The order matters: an MCP wire name is
  `<server>__<tool>` and can sort ahead of every built-in name, so a cap that
  walked the names alphabetically would cut built-in tools that sort late to
  make room for MCP tools the user may never use (with sixty MCP tools that
  left 35 built-ins neither sent nor named). The line tells the two apart by
  the `__`, and a test fails if a built-in name ever has one. The budget is
  tight on purpose: the request that names a dozen tools sends the most
  schemas, and `request_size.rs` holds it 500 B under the bound with MCP
  tools registered. When everything was sent there is no line. A call to a
  listed tool by name works: the dispatcher validates it against the tool's
  full schema and the error says what is missing.

  **The set can change between turns.** It is chosen from the conversation
  so far, so a message that names a tool the earlier ones did not changes it,
  and a local server then has to process the prompt again from the tools on.
  What the user named is sticky (item 6) and so is what the model called
  (item 5): such a tool stays in the set until the budget needs the room for
  something newer, so a message that names nothing new keeps what the earlier
  ones named, and a tool does not drop out at the next message that does not
  repeat its name and come back at the next that does. What the assistant
  named is not: item 3 reads only its latest message, so a proposal is kept
  for the next message and is gone once a later reply does not repeat it. If
  it proposes a limiter and a de-esser, the user asks "what's a de-esser?"
  and the answer is about that alone, then "ok do both" is no longer shown the
  limiter (the de-esser stays, because the user named it). Making every
  assistant mention sticky would let a reply that lists what the model can do
  take the budget for the rest of the conversation. The names line follows the
  set, so it changes with it and only then.

  MCP tools are part of the registry, so the selection applies to them on
  Ollama like any other tool: one is sent in full when a message names it
  (their names are matched by the same rules), and otherwise it is listed by
  name.

**The session block has a budget (#573).** The system prompt carries a block
that lists the session (`crates/ai/src/session_context.rs`): the head node, its
length, every track with its clips, gain, pan and chain, every bus with its id,
its sends and its chain, the master chain, the selection and the markers. It
grows with the session, and nothing used to cap it: a six-track mix with three
buses left Ollama's first request 82 B under the bound, and thirty tracks went
past it. `render_block_within` now writes it within a byte budget that depends
on the tool set (`block_budget`):

- `SESSION_BLOCK_MAX_BYTES`, 8,000 B, for a provider sent every tool. It has
  the room, but pays for the block on every turn, uncached, since the block
  changes with every edit. A thirty-track mix with eight buses and forty
  markers (about 5.5 KB) is listed in full.
- `SLIM_SESSION_BLOCK_MAX_BYTES`, 2,300 B, for Ollama. The block also shares the
  request with the slim tools: what it takes past `SLIM_SESSION_ALLOWANCE_BYTES`
  (300 B, about a one-track session's block, which is what
  `SLIM_TOOLS_BUDGET_BYTES` was set against) comes off the tool budget
  (`slim_tools_budget`). So a session of any size makes about the request a
  one-track one does, and what a big session costs instead is the tools at the
  end of the order of priority above, which stay in the names line. At the cap
  the budget still holds the whole core, so a message that names nothing loses
  nothing.

A session that fits is written exactly as before. One that does not keeps the
fixed parts (head, length, master chain, selection, headers; a chain lists at
most 16 effects and a bus at most 8 sends) and cuts the three lists. Each gets
a share of what is left, tracks twice what buses and markers get, and a list
that needs less passes the rest on. Within a list the items the user's message
names come first (a track by its name or as "track 3", a bus or a marker by its
name), then for buses the ones a listed track sends to, then the rest in order;
what is shown keeps the list's own order. A cut list ends with a line that says
how many it left out of how many and how to reach them, such as
``…and 192 more tracks (200 in all), not listed here. `list_session` with what "tracks" pages through them; a tool's `track` takes any index up to 199.``
(#618). `list_session` (`crates/tools/src/tool/list_session.rs`) is a read-only
tool, so Plan first never holds it. It takes `what` (`tracks`, `buses` or
`markers`), `offset` and `limit`, and returns one page in the block's own lines
(they are written once, in `crates/tools/src/session_lines.rs`, and `ai` calls
that), a summary that says what range it showed of how many, and `next_offset`
while there is more. A page ends at 100 items or `PAGE_MAX_BYTES` (4,096 B of
the result as the model receives it), whichever comes first, so a
two-hundred-track page and a thousand-marker page each fit an 8,192-token
context (`ollama_a_page_of_a_huge_session_fits_an_8k_context`). On Ollama the
two-hundred-track session lists seven tracks, two buses and nineteen markers,
and the tool, pinned (item 0 above), lists the rest. The block is rendered once per turn with the rest
of `system`, so the thinking-block rule below is unaffected. `request_size.rs`
runs a thirty-track session and a two-hundred-track one with fifty buses and a
thousand markers, and holds every session's first request
`SESSION_HEADROOM_BYTES` (500 B) under the bound; the six-track mix with the
message that sends the most tools now leaves 1,080 B, and the two-hundred-track
session with sixty MCP tools registered still fits, 130 B under (it was 536 B
before the pinned `list_session` added its 393 B of schema to a cut session).

Three things stay as they were. The set and the names line are worked out once
per turn, so every round trip sends the same tools in the same order and
llama.cpp can keep the prompt it has already processed. The dispatcher still
validates a call against the tool's full schema, so shortening what the model
reads loosens nothing. And permission is the whitelist, not the slim set: a
permitted tool the model calls without having been shown it still runs.

When the context is still too small (a long conversation, or a large profile),
a 400 or 413 whose body says so becomes `Error::ContextTooSmall`
(`crates/ai/src/context_window.rs`) instead of "the model provider returned an
error". It recognises llama.cpp's `exceed_context_size_error`, the
`context_length_exceeded` code, and the wording of OpenAI, Anthropic and Gemini,
and carries the token counts when the server gave them. Its text names no
hosted provider and says what to change, using only what the app can do today:
reopen the project (`rebuild_agent`, which opening a project and changing the
model both run, builds the agent afresh with an empty conversation; there is no
new-chat or clear-conversation control), choose a model with a larger context,
and for a local server raise its context (for Ollama its context length, for
llama.cpp `--ctx-size`). The server's own body is logged at `warn`, with the
status, where the error is produced. Ollama's own server typically
truncates an over-long prompt to its context length instead of refusing it, so
for Ollama the bound on the first request is what protects the model; the error
covers servers that do refuse.

### LlmProvider Trait

The single extension point for new LLM providers. Located at `crates/ai/src/provider.rs`.

```rust
pub trait LlmProvider: Send + Sync + Debug {
    fn id(&self) -> &'static str;
    fn base_url(&self) -> &str;
    fn default_model(&self) -> &str;
    fn classifier_model(&self) -> &str;
    fn translate_model(&self, model: &str) -> String;
    fn apply_auth(&self, req: RequestBuilder, api_key: &str) -> RequestBuilder;
    fn endpoint_path(&self) -> &str { "/v1/messages" }
    fn wire_format(&self) -> WireFormat { WireFormat::AnthropicMessages }
    fn requires_api_key(&self) -> bool { true }
    fn tool_set(&self) -> ToolSet { ToolSet::Full } // Slim for Ollama only
    fn supports_effort(&self) -> bool { false } // true for Anthropic only
    fn list_models_path(&self) -> &str { "/v1/models" }
    fn serialize_request(&self, req: &MessagesRequest) -> Value;
    fn parse_stream_chunk(&self, raw: &str) -> Result<Vec<StreamEvent>, ProviderError>;
    fn label(&self) -> &str { self.id() }
}
```

### Supported Providers

| ID | Base URL | Auth | Default Model | Notes |
|----|----------|------|---------------|-------|
| `anthropic` | `https://api.anthropic.com` | `x-api-key` header | `claude-sonnet-5-5` | Native Anthropic format. The classifier is `claude-haiku-5-5` |
| `openrouter` | `https://openrouter.ai/api` | `Authorization: Bearer` | `anthropic/claude-sonnet-5.5` | Anthropic-compatible API. Its ids are its own and dotted (`anthropic/claude-haiku-5.5` is the classifier), so the defaults are consts of their own; an unqualified id is still prefixed with `"anthropic/"`, which is not guaranteed to resolve there |
| `openai` | `https://api.openai.com` | `Authorization: Bearer` | `gpt-6-luna` | Full translation: Anthropic shape → chat-completions → back. `gpt-6-luna` takes tools on Chat Completions only with `reasoning_effort: "none"`, which is sent for it (`openai_reasoning_effort`) |
| `groq` | `https://api.groq.com/openai` | `Authorization: Bearer` | `llama-3.3-70b-versatile` | Chat-completions; reuses `OpenAIProvider`'s translation |
| `gemini` | `https://generativelanguage.googleapis.com/v1beta/openai` | `Authorization: Bearer` | `gemini-3.8-flash` | Gemini's OpenAI-compatible endpoint; reuses the same translation. The classifier is `gemini-3.5-flash-lite` |
| `ollama` | `http://localhost:11434/v1` | none (`requires_api_key() == false`) | `llama3.2` | Local daemon, OpenAI-compatible; reuses the same translation. The only provider sent the slim tool set (see [Request size](#request-size-and-small-context-models)) |

Every provider's base URL can be overridden per provider from Settings (`<provider>_base_url` in the keychain).

`wire_format()` names the shape of a provider's requests and replies. `AnthropicMessages` is the default, `ChatCompletions` is what OpenAI, Groq, Gemini and Ollama speak, and `Responses` (`crates/ai/src/responses.rs`, #474) is OpenAI's Responses API, the only route Sign in with ChatGPT's plan tokens are accepted on. The Responses format differs throughout: the system prompt is `instructions`, the conversation is `input` items (`function_call` and `function_call_output` joined by `call_id`), function tools sit in one `namespace` tool, `store` is `false` and `stream` is `true` always, and the fields the preview refuses (`UNSUPPORTED_FIELDS`) are never sent. A turn succeeds only at `response.completed`; `response.failed`, `response.incomplete`, `error` and a stream that ends first are errors. Its one-shot calls (the classifier, the plan) are streamed too. No provider uses it yet: a provider that will holds a `ResponsesCodec` and hands it `serialize_request` and `parse_stream_chunk`.

### Reasoning Effort

Anthropic's Messages API takes `output_config: {"effort": "low" | "medium" | "high" | "xhigh" | "max"}`; Settings exposes it for Anthropic only (`<provider>_effort` in the keychain, `get_effort_for` / `set_effort_for`). `LlmConfig::effective_effort()` drops it for any provider whose `supports_effort()` is false, so OpenRouter and the chat-completions providers never receive the field. Three consequences are handled in `agent_loop.rs`:

- The classifier request never carries it: its answer is one word, and a classifier model without the parameter answers `effort` with a 400 while `classify_mode` swallows errors. It is sent with `thinking: {"type": "disabled"}` (Anthropic wire format only; the Haiku models accept it at their default effort) and `max_tokens` 256, because a model that thinks first would otherwise spend the cap before its one word and the mode would silently fall back to `general`.
- A set effort raises `max_tokens` (`Effort::min_max_tokens`), because thinking counts against it; the one-shot reply reader takes the first `text` block, not `content[0]`. With no effort set the caps are `DEFAULT_MAX_TOKENS` (a turn's step) and `PLAN_MAX_TOKENS` (the plan request), both 8192: Anthropic's 5.x models think by default whether or not an effort is sent. The 8192 goes to every provider, and OpenAI is reported to reject a request above a model's output limit, so a chat-completions body (a streamed step, the plan, the classifier, the Settings probe) is clamped to the limit `models::max_output_tokens` holds for its model; an id not in that table goes out unchanged.
- `thinking` / `redacted_thinking` blocks are kept in the assistant history and replayed first, signature intact, for as long as the turn that produced them lasts; block and delta types the client does not model are skipped. The next turn drops them (below).

### Thinking blocks and the request prefix

On Anthropic's 5.x models (Fable 5.1, Opus 5.5, Sonnet 5.5, Haiku 5.5) a `thinking` block's signature is bound to the request that produced it: the top-level `system`, the `tools` set, and every message before it. Replayed under a different prefix it is a 400 `invalid_request_error` ("Invalid `signature` in `thinking` block. The block is bound to a different conversation"). The API enforces this by default for accounts created on or after 2026-08-31, and for any request that sets `thinking.block_binding.prefix_mismatch_behavior`.

edytlab changes the prefix between user turns: `system` is rebuilt every turn (the classifier's mode, matched skills, memory, the agent profile, and the session context: the head node id, the tracks, the buses and the master chain with their effects, the selection and the markers), and `tools` follows that turn's whitelist (the per-turn disabled-tools list) and, for a provider sent `ToolSet::Slim` (Ollama), the tools that turn's message names (see [Request size](#request-size-and-small-context-models)). So `run_turn` calls `strip_thinking` once at the start of every turn, removing every `thinking` and `redacted_thinking` block from the history before the new user message is added. The docs list removing all of them as valid; the model only loses that reasoning. Text, `tool_use` and `tool_result` blocks, and their pairing, are untouched.

Within a turn nothing changes: `system` and `tools` are built once before the request loop and sent unchanged on every step, so the thinking a step produced is replayed, signature intact, with the tool results that follow it. The slim tool set and the names line it adds to `system` are chosen at that same point, once per turn and after `strip_thinking`, so they sit inside this rule (and a local server keeps the prompt it has processed only while they stay put). That is the one case where replaying is valid, and it holds only while that stays true: **a change that rebuilds `system` or `tools` between a turn's steps must strip there too, and so must one that edits or removes an earlier message** (client-side compaction, trimming old tool results, rewriting a stored `tool_use` or `tool_result`), because that changes the prefix of every block after it. Anthropic's server-side compaction and context editing are documented as valid, since the check compares the conversation as it was sent. `tests/prior_turn_thinking.rs` pins both halves with a mock provider (a changed `system`, a changed tool set, and a fixed prefix inside a turn), and `tests/live_thinking_binding.rs` is the `#[ignore]`d check against the real API with the strict check switched on (`ANTHROPIC_E2E_KEY`).

Providers that do not keep thinking in the history (everything but Anthropic's own API, see `supports_effort`) have no such blocks, so this is a no-op for them. A follow-up would keep reasoning across turns with an append-only history and a frozen `system` (the session context sent in the user message or as a mid-conversation system message); it is not done.

### OpenAI Translation Layer

OpenAI uses a different request/response format. `OpenAIProvider` translates bidirectionally:

**Request** (Anthropic → OpenAI):
- System blocks → `{role: "system"}` message
- User tool_results → `{role: "tool", tool_call_id}` messages
- Assistant tool_use blocks → `tool_calls` array in `{role: "assistant"}`

**Response** (OpenAI → canonical StreamEvents):
- OpenAI streaming state is per-message (tracks block indices in a `Mutex`)
- `finish_reason` mapping: `stop` → `end_turn`, `tool_calls` → `tool_use`, `length` → `max_tokens`
- Tool call id synthesis: `call_<message_id>_<index>` when OpenAI omits the id

### Keychain Integration

```rust
// crates/ai/src/keychain.rs — every entry lives under the service "app.edytlab.desktop"
pub fn load_api_key(provider_id: &str) -> Option<String>        // logs an unreadable keychain, reads as None
pub fn try_load_api_key(provider_id: &str) -> Result<Option<String>, KeychainError>
pub fn save_api_key(provider_id: &str, key: &str) -> Result<Persistence, keyring::Error>   // where the save landed
pub fn delete_api_key(provider_id: &str) -> Result<(), keyring::Error>
pub fn load_active_provider() -> Option<String>
pub fn try_load_active_provider() -> Result<Option<String>, KeychainError>
pub fn save_active_provider(provider_id: &str) -> Result<Persistence, keyring::Error>
pub fn persistence() -> Persistence                              // Durable | SessionOnly { reason }
// plus load_/save_/delete_ for `base_url`, `model` and `effort`, keyed the same way
```

Keychain slots (accounts):
- `<provider>_api_key` — one per hosted provider (`anthropic_api_key`, `openrouter_api_key`, …)
- `active_provider` (stores provider id string)
- `<provider>_model` (per-provider model choice)
- `<provider>_base_url` (per-provider endpoint override)
- `<provider>_effort` (per-provider reasoning effort; absent means the model's default)

Builds from before multi-provider support stored the Anthropic key as `anthropic_api_key`, which is already the new name for that slot, so it is read as-is — there is no migration step.

On Linux the `keyring` crate is built with `linux-native`, `sync-secret-service` and `crypto-rust` (not `linux-native-sync-persistent`, whose builder would make the kernel-keyring entries older builds wrote unreachable). The default store is the **Secret Service** (GNOME Keyring, KWallet from Plasma 5.97/6, KeePassXC) over D-Bus with an encrypted session, so values survive a reboot (#394). The kernel keyring (`keyutils`) stays compiled for two jobs:

- **Fallback.** When no Secret Service answers (no session bus, nothing owning `org.freedesktop.secrets`), a save goes to the kernel keyring and lasts until reboot. `persistence()` reports `SessionOnly { reason }` (it looks up an account nothing writes, `persistence_probe`, so it can never raise an unlock prompt), `get_keychain_persistence` hands it to the frontend, and the app shows a banner and a note in Settings. A save falls back only after a second read confirms the service is unreachable: `keyring` words a failed `create_item` on a live service the same way, and that must fail the save rather than park the key where a reboot erases it. A read looks twice for the opposite reason: the Secret Service client gives each D-Bus call two seconds and the first call is where a service that is not running yet gets started, so a slow start looks unreachable with the key stored. With no kernel copy to return, the read asks the Secret Service once more before calling the value missing; once one read has found it unreachable on both looks, the slots read after it get one look each (startup reads eight slots, and two seconds twice each is about half a minute), until a read it answers clears that.
- **Migration.** A value that earlier builds left in the kernel keyring is moved into the Secret Service on first read and the old copy deleted, if it is still there (same boot, within the persistent keyring's three-day expiry). A failed move keeps the old copy. A move the Secret Service refuses (locked, unlock prompt dismissed) is remembered for the rest of the process and not tried again, since each try is another prompt; the value is still read from the kernel keyring until the app restarts. The same rule settles a kernel copy that sits next to a Secret Service one. Every save to the Secret Service removes the kernel copy (and overwrites one that cannot be removed, so it never goes stale), so a kernel copy that coexists with a durable one was saved after it, while the Secret Service was briefly unreachable, and a read prefers it and moves it across. A copy equal to the durable one is just dropped, a blank one is ignored, and a kernel keyring that cannot be read changes nothing. The `save_*` functions return the `Persistence` they landed in (`SessionOnly` is a save that fell back), and the save commands (`set_api_key`, `set_api_key_for`, `set_active_provider`, `set_base_url_for`, `set_effort_for`, `set_active_model`) pass it on in the shape `get_keychain_persistence` answers with: several writes in one command (a key and the active provider) count as session-only if either fell back, with the first reason, and a clear writes nothing and is durable. Settings shows the launch banner's words after a Save in which any write fell back, and keeps its dialog up until the person goes on.

A locked or refused store is an error, never a silent "nothing stored": `try_load_api_key` and `try_load_active_provider` return it, the `Option` loaders log it, and startup keeps the first error's reason so `has_api_key` rejects with it instead of answering `false` (which would show the first-run Welcome over a stored key). Startup also stops reading at the first refusal (locked, or an unlock prompt dismissed), since every further slot would raise the same prompt, and a key or provider save that goes through afterwards reads back the models startup skipped; and switching to a provider that needs no key reads nothing. Startup's reads run on a thread of their own, because an unlock prompt can wait on the user for as long as they like: the window comes up, `has_api_key` and the other commands that answer from or write over what the reads fill in wait for them (`AppState::startup_reads_settled`), and the frontend says it is waiting for the keyring. A startup the keychain refused is recorded (`AppState::keychain_refused`) until a save goes through, and MCP autostart and restart skip servers whose env or headers carry a `<keychain:slot>` reference meanwhile, with the reason on the registry. `get_active_provider`, `get_base_url_for` and `get_effort_for` reject rather than answer a default they never read, and Settings does not write a base URL or reasoning level it did not read. The agent rebuild (`rebuild_agent_from`) reads the keychain too, for the base URL and level of the provider it builds for and for the key of another provider a profile pins, so while the refusal stands it reads none of them and leaves the agent unbuilt: it cannot be built from guesses, since the stored URL may be a gateway's. `send_message` then says why instead of "call set_api_key first", and the first key or provider save that goes through lifts the refusal and builds the agent. Clearing a value removes both copies, and fails if the kernel copy cannot be removed, because the next read would move it straight back. All keychain I/O is serialized behind one lock, since the Secret Service mishandles concurrent D-Bus access. `Display` of a `keyring::Error` never carries the secret but its `Debug` can (`BadEncoding`), so errors are carried as text only.

### Model Catalogue

`crates/ai/src/models.rs` fetches the live list for every provider: Anthropic's `GET /v1/models` with the saved key (newest first; deprecated models are listed and labelled with the day they retire; retired ones are never asked for), OpenRouter's `GET /api/v1/models`, and OpenAI's, Groq's, Gemini's and Ollama's OpenAI-style models endpoint under the configured base URL. Anthropic's list is the one that cannot be assumed: with no key, or when the fetch fails (a 10 s timeout, a rejected key, an unreadable answer), `list_models_for` answers with edytlab's built-in list of the current models (`anthropic_fallback_models`) and says so, as a `Catalogue { models, source }` whose source is `Live` or `Fallback { reason }`. Settings shows "Showing edytlab's built-in list (reason)" for a fallback, and warns when the saved model is missing from a live Anthropic list (it may have been retired). The command reads the saved key when none is typed; it stays in the Rust process.

Results are cached for 10 minutes, per provider, endpoint and a hash of the key held only in memory (so a changed base URL or a rotated key fetches again), and only live lists are cached, so a fallback is never served after the key is fixed. The OpenAI list keeps the ids that chat and can hold the agent's request (it drops `gpt-4` and `gpt-3.5-turbo`, whose context is smaller than one) and sorts the newest family first, with no generation written into the order. The combo picker in Settings surfaces these alongside free-form input so new model ids work immediately.

The same file holds the one table of per-model output limits (`max_output_tokens`, applied by `clamp_max_tokens`): the OpenAI models with an output limit of 8192 or less (the `gpt-3.5-turbo` and `gpt-4-turbo` families and the `gpt-4-*-preview` snapshots at 4,096, and `gpt-4` at 8,192, which lowers nothing and is there so the figure is written down), and the Groq and Gemini models with a documented limit under it. The OpenAI rows are for models OpenAI is retiring (the last of them on 2026-10-23, dates in the table's doc), so they can be pruned as the dates pass. Each figure is taken from the provider's own documentation, cited next to it; a model nobody has sourced is left out, so its requests go out as built. When the caps in `prompt.rs` are raised, check this table first.

### OAuth client

`crates/ai/src/oauth/` is a generic OAuth 2.0 and OpenID Connect client for signing in through the system browser (#474): the authorization-code flow with PKCE for a public client (no secret), redirected to a loopback listener. Nothing in it names a provider. The issuer, endpoints, client id, scopes, callback host and path, and extra parameters such as `resource` all come from the caller or from the issuer's discovery document; Sign in with ChatGPT (`crates/ai/src/chatgpt/`, below) is the layer on top, and OpenRouter may be another.

- `pkce.rs`: the S256 challenge of a verifier of 86 unreserved characters, and 256-bit `state` and `nonce`, all from the OS random generator (`ring::rand::SystemRandom`).
- `loopback.rs`: binds `127.0.0.1` (preferred port, else an ephemeral one) before the caller's `open_url` opens the browser, without `SO_REUSEADDR`, and with `SO_EXCLUSIVEADDRUSE` on Windows. The redirect URI names `127.0.0.1`, never `localhost`, which also resolves to `::1`, an address the listener does not hold. It accepts one `GET` of the configured path with a `Host` header naming the bound host and port and this attempt's `state`; anything else gets a 404 or 400 and it keeps waiting, until a five-minute timeout or a `CancelHandle`. That includes a wrong or missing `state` (any web page can make the browser request the callback URL, so such a request must not end the sign-in) and a request whose `Sec-Fetch-Mode` is not `navigate`. `state` is compared in constant time and checked before anything else; with the right `state`, `access_denied` is its own error. The browser gets a fixed page that echoes nothing, with `Cache-Control: no-store` and no CORS headers. Without `SO_REUSEADDR` the preferred port stays taken for about a minute after an attempt (TIME_WAIT), so a quick retry gets an ephemeral port: callers use the port the listener reports. `Callback::check_issuer` applies RFC 9207's `iss` check before the code is exchanged. The wait holds nothing that is not `Send`, so it can run on a spawned task or in a Tauri async command.
- `oidc.rs`: discovery (its `issuer` must match, and an `https` issuer may not name a plain-`http` endpoint), a JWKS cache with one rate-limited refetch for an unknown `kid`, and ID-token verification: RS256 only (with `ring`, 2048-bit keys or larger), no `crit` header, exact `iss`, `aud` exactly the client id, `exp`, `nbf` and `iat` with 60 s of skew, and every claim typed strictly so a wrong JSON type fails rather than skips a check. A sign-in's ID token must carry the attempt's `nonce`; one from a refresh response is checked against the original instead (OpenID Connect Core §12.2: same `iss`, `sub` and `aud`, no new nonce).
- `token.rs`: the code exchange, refresh and revocation as form POSTs, with typed success and error bodies. A success must be a `Bearer` token, `expires_in` is capped at a day, and an error body's text is cut short and stripped of control characters. An error code is read from RFC 6749's `"error": "<code>"` or from the wrapped `"error": {"code": ...}` that Codex CLI also reads from OpenAI's auth server.

Tokens, codes and the verifier are `SecretString`: `Debug` prints `<redacted>`, and it has no `Display`, `Serialize` or `PartialEq`, so it cannot reach a log, an error or an IPC reply, or be compared in variable time; a test fails to compile if it, or another type holding a secret, gains one. The HTTP client never follows redirects (a 307 would resend the form, refresh token included), caps what it reads, and sends only to `https` or a loopback address; the authorization URL handed to the browser is held to the same rule.

### ChatGPT session

`crates/ai/src/chatgpt/` is Sign in with ChatGPT for ChatGPT plan usage (#474; the protocol is §3 of `docs/specs/2026-10-10-chatgpt-subscription-sign-in.md`), built on the OAuth client. It has no Tauri dependency, and nothing calls it yet: the `chatgpt` provider and the desktop commands come next.

- `spec.rs`: the protocol's constants and rules. The scopes, `resource`, `agent_name_hint` on a first registration only, the `urn:uuid` host ID on every request, `login_hint` (never `id_token_hint`) on a returning sign-in, and `prompt=consent` only to turn plan usage back on. The callback rules: a registration's callback must hand back an issued client id (never `dynamic_agent_client`), and a returning sign-in's may not name another. Plan usage needs `chatgpt.tokens.use.direct` in the token response's granted scopes. A failed refresh is temporary when its status is a 5xx, 429 or 408 (whatever code the body carries), or the network failed; unusable on a 4xx carrying `invalid_grant`, `invalid_refresh_token`, `token_expired`, `refresh_token_expired`, `refresh_token_invalidated` or `refresh_token_reused`; and refused for any other reason (`invalid_client` and the like).
- `store.rs`: `registration.json` in `<app config dir>/chatgpt/` holds the host ID and one record per issued client (issuer, verified `sub`, email, granted scopes, the claims a refreshed ID token is compared with, `saved_at`, `signed_out`). No token is in it. It is written atomically (a new file renamed over the old), `0600` in a `0700` directory on Unix (an existing directory is narrowed to `0700`, a symlinked one refused), and survives a reboot even where the keychain does not. One that does not parse is set aside as `registration.json.corrupt`. The refresh token sits behind the `RefreshTokenStore` trait, which the desktop app will implement over the keychain slot `chatgpt_refresh_token`; its calls must return in bounded time, since the session holds its refresh lock while they run, so an implementation fails rather than waiting on an unlock prompt. Access tokens live in memory only, and ID tokens are not kept once verified.
- `session.rs`: `ChatGptSession`. `sign_in` saves the host ID before the first attempt and the issued client id before the code is exchanged, checks RFC 9207's `iss` when discovery promises it, verifies the ID token with `aud` the issued client id, and on a returning sign-in requires the saved account's `sub`; the session in use changes only once all of that holds, `registration.json` is written before the refresh token, and tokens it refuses are revoked. A returning sign-in, signed out or not, reuses the saved account's client; `SignInIntent::AnotherAccount` registers a new one for a different account, and if another account is still signed in, its record is marked signed out and its refresh token revoked in the same step, since the keychain has one slot. `bearer()` hands out the access token, refreshing within five minutes of expiry but not before `earliest_refresh_at` while the token is valid. Refresh is single-flight in the process (a `tokio::sync::Mutex`) and across processes (an `fs4` lock on `refresh.lock`, which falls back to one process-wide lock per session directory, with a warning, where the file system has no locks). Under the lock, on every refresh, it re-reads the refresh token and `registration.json`: a token another window wrote is taken with the account as the registration now has it, and if the registration says signed out or names another client, this window's session ends in memory and nothing is deleted. The refresh, the end of a sign-in and sign-out run on tasks of their own, so a caller that stops waiting cannot leave a rotated token unsaved. The rotated refresh token is saved before the new access token is handed out; a store that refuses it (`TooLong` on Windows' Credential Manager, or a locked keychain) leaves the session in memory, and the status says so. An unusable refresh token ends the session and is removed from the store only if it is still the one there; a temporary failure keeps every credential and backs off (15 s, doubling to 5 min). `sign_out` signs the account out from any window: under the lock it takes the stored refresh token (whatever this window's state), clears it and marks the account signed out, keeping the registration and the host ID; then, with the lock let go, it revokes that token at the discovery document's `revocation_endpoint`, which must be on the issuer's origin, retrying a network failure or a 5xx, and reports whether the revocation was confirmed. A keychain it cannot read is reported as an unconfirmed revocation, and the token it holds is left for a later sign-out to revoke. A `registration.json` that does not parse is set aside only under the lock.

Tokens go only to the issuer: `Endpoints` has one constructor in the app, `Endpoints::production()` (`https://auth.openai.com`, with the documented `/api/accounts/authorize` and `/api/accounts/oauth/token`). Tests point the session at a mock issuer with `Endpoints::loopback_for_tests`, which exists only with the crate's `test-endpoints` feature and accepts only a loopback address. Only `ai`'s dev-dependency on itself turns the feature on, so no build of the desktop app or the CLI has it.

### Constants

```rust
pub const DEFAULT_MODEL: &str = "claude-sonnet-5-5";
pub const CLASSIFIER_MODEL: &str = "claude-haiku-5-5";
pub const MAX_TOOL_CALLS_PER_TURN: usize = 20;
```

---

## 5. Tool Dispatch System

### Tool Trait

```rust
// crates/tools/src/dispatcher.rs

pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    /// `{ "name", "description", "input_schema" }` — the Anthropic tool shape.
    fn schema(&self) -> Value;
    /// Can a call change anything the user owns? Defaults to `true`, so a new
    /// tool is held for approval under Plan first until marked read-only.
    fn mutates(&self) -> bool { true }
    /// May this tool run without the store lock held (#421)? Defaults to `false`:
    /// a new tool runs under the locks until someone opts it in.
    fn runs_off_the_lock(&self) -> bool { false }
    /// Called with `args` already validated against `input_schema`.
    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> Result<ToolResult>;
}

pub struct ToolContext<'a> {
    pub store:         &'a mut session::Store,
    pub engine:        &'a mut audio_engine::Engine,
    pub user_message:  &'a str,
    pub clipboard:     &'a mut Option<crate::Clipboard>,
    pub allowed_tools: Option<&'a HashSet<String>>,  // the turn's whitelist, None = all
}

impl ToolDispatcher {
    pub fn new() -> Self                    // empty
    pub fn default_dispatcher() -> Self     // registers all 94 built-in tools
    pub fn register(&mut self, tool: Box<dyn Tool>)
    pub fn unregister_prefix(&mut self, prefix: &str) -> usize  // MCP `<server>__` tools
    pub fn tool_schemas(&self) -> Value     // sent to the LLM
    pub fn would_mutate(&self, name: &str, args: &Value, allowed: Option<&HashSet<String>>) -> bool
    pub fn invoke(&self, name: &str, args: Value, ctx: &mut ToolContext) -> Result<ToolResult>
    // `invoke` is these two in a row. The split lets a caller admit a call under the
    // dispatcher's lock and run it after dropping it:
    pub fn prepare(&self, name: &str, args: &Value, allowed: Option<&HashSet<String>>) -> Result<Prepared>
    pub fn run(prepared: &Prepared, args: Value, ctx: &mut ToolContext) -> Result<ToolResult>
}
```

### Off-lock tools

The agent calls `tools::Shared::dispatch` (`crates/tools/src/shared.rs`), not `invoke`. It admits the call under the dispatcher's lock, alone, and drops that lock. Then:

- A tool that does not say `runs_off_the_lock` takes the store, engine and clipboard locks, in that order, for the whole call. This is what every tool did before.
- A tool that does is run on a **staged** store handle with no lock held, a fresh `Engine` and an empty clipboard. Its appends stay in memory. Then the store's lock is taken for `Store::commit`:
  - **Published**: the head did not move, so its nodes are written and the result is returned.
  - **Conflict**: the head moved while the tool ran, which means the user edited (a marker, a fader, an undo, a clip move). That edit is kept. The tool's work is thrown away and the tool is run again on the new head, so its node parents off the user's, unless the edit changed what the call's arguments mean (below). The tool runs off the lock at most `OFF_LOCK_ATTEMPTS` (2) times, the first run and one re-run. If both lose, it runs once under the locks, so it always finishes.
- A read-only tool stages nothing, so it never conflicts and is never re-run.

`Shared::dispatch` also says where the call left the head (`Dispatched::moved_head`, #594), and the agent loop sends `agent://node-created` with that head only when it is set. A locked run reads the store's head before and after the tool, under the lock; an off-lock run reads its staged handle's head before and after, and reports it once the commit has published it. So the event names the head the store is on, never a tool's `node_id` (`name_node` returns the node it labelled, and `apply_diff` and `apply_recipe` return none though they move the head), a tool that moves the head several times is reported once, one that moves it and then fails is still reported, and a user's edit that lands while an off-lock tool runs is not reported as the agent's.

Which tools run off the lock is pinned by name in `crates/tools/tests/off_lock_tools.rs`: the pure audio edits, `load`, `mix_to_new_track`, and the analysis tools, among them `select_region` and `duck_under_speech`, which with `voice_tracks` decode and scan every clip of the voice tracks (#570). That test also scans each opted-in tool's source, and the helper files they share (`util.rs`, `speech_detect.rs`, `ducking.rs`), with comments and literals blanked, `#[cfg(test)]` items skipped and the rest of the file read, for what a staged run cannot do (clipboard, engine, nested dispatchers, history rewrites, `progress`, `rederive`, `materialize`, recipes, sweeps), and allows only the `crate::` modules (and the `super::` paths that name the same ones) it has been told are safe to reach, so a helper in another file does not slip through. One spelling in one file is allowed by name (`ALLOWED`): `select_region`'s `crate::progress::report(`, which sends the selection to the window. It appends nothing, so it is never run again and reports once, and the progress sink takes no lock. The scan reads text, so it also finds the spellings that hide a second `Store` (`type S = Store;`, `<Store>::open`, a `use` group rustfmt wraps onto several lines), but not a path put together by a macro; review is the guard for that.

**A re-run must mean what the first run meant.** A call names a track by its position (`track: 1`) and a time by its seconds, as the model saw them. Before this change a user's edit waited for the tool, so the tool always ran on the session the model had seen. Off the lock the edit can land first, and running the same arguments on the new head would edit another track and report success: with the tracks `[drums, vocals, bass]`, `time_stretch {track: 1}` (vocals) loses to the user removing drums, and the re-run on `[vocals, bass]` would stretch bass. Undoing a `remove_track` while the tool runs does the same the other way, and a clip dragged on the target track makes `fade`'s seconds land on other audio. So `Shared::dispatch` remembers, for a call that names tracks (`tools::shared::addressed_tracks`: any top-level argument with `track` in its name that holds a whole number or a list of them, and nothing else: `off_lock_tools.rs` rejects an opted-in tool that names a track inside an object or a list of objects, or in an integer whose name lacks `track`, at any depth of its schema), each named track's id and clips as they were at the head its first run began on. A later run, the re-run or the locked run, starts only if every named position still holds that track with those clips. If not, nothing is applied and the call returns a `ToolResult::Error` that says what changed, where each track the call named is now if it is still in the session (`"vocals" is now track 0`, so the model does not send the same position again), and lists the tracks as they are now, for the model to read and call again; the user's edit is kept. An edit elsewhere does not stop a re-run: a marker, a fader, a track added or removed after the named one, a clip moved on another track. A call that names no track (`load`) is always re-run. The check is about the tracks a call names, and one off-lock tool also edits tracks it does not: with sync-lock on, `insert_silence` opens its gap on every other track, shifting their clips after `at` later by the same amount and splitting a clip that straddles it, so the tracks stay aligned. That is safe to run again. The shift is made on the head the run lands on, to whatever clips the other tracks have by then, so it can only keep them aligned, and the track it names is checked like any other. It is the only such tool on the list (`cut_range`, the other sync-lock edit, is not off the lock); a tool that opts in and edits a track it does not name must make the same argument. Clips are compared whole, so a user who edits the very track the agent is working on, during the seconds a tool takes, makes the agent call again instead of getting a wrong edit.

The derived-audio sweep waits while a staged handle is alive (`Store::staged_in_flight`), a count kept per project folder and shared by every `Store` opened on it in the process, whatever the spelling of the folder (opening a project builds a new `Store`, and the reclaimer reads through one of its own, so a count per `Store` would read zero for exactly the sweeps that can run mid-tool), because the output of an off-lock tool is named by no node until it commits (rule 0 in `crates/tools/src/reclaim.rs`). `compact_session` refuses to apply for as long, and Save As keeps a handle that writes nothing while it copies a project's audio, so that nothing its copied nodes name is deleted under it (#571). Files written under `derived/` by a run that lost the race are orphans, and the next sweep removes them. (`mix_to_new_track` writes to `<project>/generated/`, which nothing sweeps, so a run of it that loses the race to an edit that changes the mix leaves its render there.) The WAVs these tools write under a content hash are written to a temporary name and renamed into place, so a name that exists is always a whole file. A folder has one live `Store` (#501, #534, #566), and the rule is enforced by `session`, not by each caller. Opening a project that has a live `Store` reads the folder again into that `Store` (`Store::reread_into`, in place under the lock) rather than building a second one, so the agent mid-turn and the window stay one writer behind one lock. "Live" is any store something still holds, not only the one the window has open: the registry (`crates/session/src/live.rs`) maps each folder to a weak handle, so opening A, then B, then A again during a turn on A finds the store the turn holds. Folders are told apart by identity (`FolderId`: device and inode on unix; on Windows the volume serial number and the 128-bit file id from `GetFileInformationByHandleEx`/`FileIdInfo`, because the 64-bit file index is not guaranteed unique on ReFS, which Dev Drive uses), never by path, so `Music` and `music` on a case-insensitive disk, or two paths to one folder through a bind mount, are one folder; the in-flight count above is keyed by the same identity. The first store on a folder keeps a handle on it open while any store lives (the pin), so the identity cannot be reused by a new folder. Two costs of that, accepted: a project that is open holds its folder open, so the volume it is on cannot be ejected or unmounted until the project is closed or another is opened (and a store a turn still holds keeps it open past that), and a folder that cannot be opened for reading fails to open as a project (`identify` failing is an `Error::Io` for the open, where before only `.audiograph/` had to be usable). The registry owns nothing (a store nobody holds is gone, and its entry is pruned). Its lock is a leaf: nothing is taken while it is held and no store is dropped under it (what a lookup upgrades is kept and dropped after the guard), but it may be taken while a store or engine lock is held, as `batch_apply` and the replay in `tools::rederive` do from inside a tool; that cannot deadlock, because the registry never waits for anything under its lock.

A path can come to name another folder while a store is open on the old one, because the store writes by path: the project is restored from a backup over itself, or trashed and made again. The new folder has a new identity, so a store opened on it would start with nothing in flight beside the old store's, and the old store's off-lock tool could have a file swept from under it by the new store's open sweep. Each store therefore registers the canonical path it reads its folder through and keeps it while it lives (the path moves when the project folder is renamed and the store is read again through the new name, `Store::reread_into`; a staged edit keeps the path it started with, since that is what it writes through), and a write open (shared, private or raw) of a path that a live store of a *different* folder writes through fails with `Error::FolderReplaced` until that store is gone. A path nothing writes through any more is free: a new project can be made at the name a renamed one used to have. When the only holders of the old store are the window's and the idle agent's, `open_project_with` lets them go (`release_replaced_project`: the agent's lock is free, no staged edit is in flight, and the handle count is exactly the window's, the agent's and the caller's) and opens the path as the new folder; when a turn, a staged edit or a command holds it, the open is refused and the user is told to wait for the assistant's turn to end and open the project again.

There are four ways to open a store. `Store::open_shared` is the project the window has and the agent works in: it returns the live `LiveStore` or builds and registers one, and `AppState::set_open_project` accepts nothing else, because a `LiveStore` can only be made by the registry. `Store::open_private` is for code that writes to a folder of its own (`batch_apply`'s per-file projects): it fails with `Error::AlreadyOpen` when the folder has a live store, shared or private, and it holds a claim for as long as it lives, so the folder cannot be opened shared meanwhile. `batch_apply` therefore refuses, before it starts, a run whose `<output_dir>/<file name>` is a project that is open, and asks for another `output_dir`. `Store::open_read_only` is a reader that refuses every write (`Error::ReadOnly`) and so can sit beside a live store: the reclaimer plans its sweep on one. The raw `Store::open` consults no live store and is for a folder only the caller can reach (tests). What keeps it out of shipped code is two things, and neither is the whole: it exists only in `session`'s test build and under its `raw-open` feature, which the crates that use it in tests enable in `[dev-dependencies]` only, so a plain `cargo build` or `cargo check` of a crate that calls it does not compile; and `crates/session/tests/no_unregistered_open.rs` lexes the source of every crate and app and fails on a raw open outside items marked `#[cfg(test)]` (brace-matched, so a `#[cfg(test)]` helper hides that helper and nothing after it). Cargo unifies a dev-dependency's features into a crate's own code whenever the dev-dependency is built, which `cargo test` and `cargo clippy --all-targets` (the CI steps) do, so there the test is what fails and the feature fails only a release build. A raw open reached through a name that is not `Store` (a macro, a trait object) is seen by neither. A live store keeps the spelling of the folder it was made with, so a re-read goes through that spelling to keep a staged edit committable, unless the spelling no longer reaches the folder (a symlink removed since): then the folder is read through the spelling the open was given, into the same store, and an edit staged under the old one is refused at its commit. The open sweeps with that store's lock held, because a tool takes the lock to stage, so none can stage between the sweep's in-flight check and its deletes.

### The Tools

There are 94, registered in `ToolDispatcher::default_dispatcher()` with their implementations under `crates/tools/src/tool/`. The full list, with every parameter, is [tools-reference.md](./tools-reference.md) — generated from the registry, so it cannot drift from what the agent can call.

Two of them are stubs: `separate_stems` (Demucs, [#385](https://github.com/laadtushar/edytlab/issues/385)) and `transcribe` (Whisper, [#384](https://github.com/laadtushar/edytlab/issues/384)) return an error in this build. `cut_words`, `remove_fillers` and the `text` mode of `select_region` need a transcript, which only `transcribe` produces, so they cannot run yet either. `duck_under_speech` and `select_region`'s `speech_passage` do not wait on it: with no transcript they key on a voice track's audio (`voice_tracks`, found by `speech_detect.rs` from voiced sound, not level, and joined into passages by the same rule in both). A phrase needs the words, so `select_region` refuses `voice_tracks` with `text`.

### Tool Input/Output Contract

All tools:
- Receive validated `serde_json::Value` input (dispatcher validates against schema before dispatch)
- Return `ToolResult::Ok(Value)` or `ToolResult::Error(String)` — never panics
- Mutate state only through `ToolContext` — no global side effects
- Append a new `SessionNode` to the store when the session state changes (non-destructive)

---

## 6. Session Graph (DAG)

### Data Model

```rust
// crates/session/src/node.rs
pub struct SessionNode {
    pub id:         NodeId,          // blake3 hash of serialized state
    pub parent:     Option<NodeId>,  // None for root nodes
    pub created_at: DateTime<Utc>,
    pub label:      Option<String>,  // Human-readable (from name_node tool)
    pub reasoning:  Option<String>,  // Agent-provided justification
    pub state:      SessionState,
    pub op:         Option<NodeOp>,  // tool + parameters that made it, when known
}

// crates/session/src/state.rs
pub struct SessionState {
    pub tracks:         Vec<Track>,
    pub bus_routing:    BusGraph,
    pub master_chain:   Vec<EffectInstance>,
    pub tempo_map:      TempoMap,
    pub key_map:        Option<KeyMap>,
    pub transcript:     Option<Transcript>,
    pub sample_rate:    u32,
    pub length_samples: u64,
    pub annotations:    Vec<Annotation>,
    pub sync_lock:      bool,
}

pub struct Track {
    pub id:       TrackId,
    pub name:     String,
    pub clips:    Vec<Clip>,
    pub gain_db:  f32,
    pub pan:      f32,
    pub muted:    bool,
    pub soloed:   bool,
    pub effects:  Vec<EffectInstance>,
    pub sends:    Vec<Send>,
}

pub struct Clip {
    pub source_path:           PathBuf,   // never modified
    pub start_in_track:        u64,       // frames, at the source's own rate
    pub source_offset:         u64,       // frames
    pub length:                u64,       // frames
    pub content_hash:          Option<[u8; 32]>,
    pub time_stretch_factor:   Option<f32>,
    pub pitch_shift_semitones: Option<f32>,
    pub beat_grid:             Option<Vec<f32>>,
    pub volume_envelope:       Vec<EnvelopePoint>,
}
```

### Store Operations

```rust
// crates/session/src/store.rs
impl Store {
    pub fn open(project_dir: &Path) -> Result<Self>  // Creates <project_dir>/.audiograph/ if absent
    pub fn head(&self) -> Option<NodeId>             // Current node pointer
    pub fn set_head(&mut self, id: NodeId) -> Result<()>
    pub fn get(&self, id: NodeId) -> Result<SessionNode>
    pub fn append(&mut self, node: SessionNode) -> Result<NodeId>  // parent := head, id := hash(state)
    pub fn list_nodes(&self) -> Result<Vec<SessionNode>>
    pub fn annotations_for(&self, head: NodeId) -> Result<Vec<Annotation>>
    pub fn diff(&self, a: NodeId, b: NodeId) -> Result<SessionDiff>
    pub fn fork(&mut self, parent: NodeId) -> Result<NodeId>
    pub fn merge(&mut self, a: NodeId, b: NodeId) -> Result<NodeId>
    pub fn revert_to(&mut self, target: NodeId) -> Result<NodeId>  // lands on target itself (ids hash state); history is kept
    pub fn set_label(&mut self, id: NodeId, label: Option<String>) -> Result<()>
    pub fn stage(&self) -> Result<Store>             // a handle for a tool that runs off the lock (#421)
    pub fn commit(&mut self, staged: Store) -> Result<Commit>  // Published, or Conflict if the head moved
    pub fn staged_in_flight(&self) -> usize          // staged handles alive; the sweep waits while > 0
}
```

A **staged** handle (`Store::stage`) is how a long tool runs without holding the store's lock. It starts at the shared head, reads node files from disk, and keeps its `append`s (and the `set_op` on them) in memory. It refuses everything that rewrites history: `set_head`, `set_label`, `remove_node`, `detach_parent`, `append_branches`, so `fork`. `Store::commit`, under the lock, writes the staged nodes and then the head file only if the shared head is still the one the handle started at. Node ids hash state alone, so that is an exact comparison, and a head that moved away and came back is no conflict. On `Conflict` nothing is written. See [Off-lock tools](#off-lock-tools).

### DAG Operations

```
head → N3 → N2 → N1 → root

Fork:
head → N3 ──────────────── (existing branch)
             └→ N4 → N5   (forked branch — new head)

Revert:
  set_head(N2)  →  head now points to N2, N3/N4/N5 still exist (no deletion)

A/B Compare:
  prepare_compare(A, B) renders both into .audiograph/previews/, held until end_compare
  accept_b(B) moves head to B
```

### Undo and redo

A node's id is the hash of its state alone, so an edit that returns to an earlier state (mute, then unmute) lands on a node that already exists, and that node keeps the parent it had the first time it was reached (`Store::append` skips the write; pinned by `a_state_reached_again_keeps_its_first_parent`). The graph does not record the route the user took, so undo cannot follow `parent` (#398).

The frontend keeps the path instead: `apps/desktop/src/lib/headTrail.ts`, held by `useSession`, is one reducer for the head, the undo trail and the redo list. Every head move in the frontend goes through it (`setHeadLocal` for a head that moved on, `resetHead` for a different project); a new head clears redo. Undo goes to the last head on the trail and asks the node for its `parent` only once the trail is spent. The History graph draws the path over the parent edges.

**Where the rule lives (#568).** The store keeps the file (`trail.json`, below) and the window keeps the rule (`headTrail.ts`). Nothing else decides undo: the CLI has none, the agent's "undo" is `revert_to`, which the window records as an ordinary step, and the browser plan (#389) reuses this UI and its reducer. Moving the rule into the store would take two or three PRs, put a file write and sync into every edit (which #521 avoided), and leave the browser tests unable to check it, because the e2e fake backend gives fixed answers. It moves when one of these arrives: a client that does not run this UI (undo in the CLI, a native app, an MCP server), two tabs or users on one project, an agent tool that undoes along the path, or another bug where the window and the store disagree about the head after #594.

The History graph's "Set as head" is one of those moves. `GraphView` hands the choice to App (`onSetHead`), which calls `set_head_to` and then records the step with `setHeadLocal`, so undo returns from the jump (#453). A plain click on a bubble is not one: it previews the node (its mix is rendered and the bubble is marked with a dashed outline, the status bar says `previewing …`) and leaves `head`, the trail and the saved path alone, because the store did not move (#522). The preview is `useGraphPreview` in App, apart from `useSession`'s head, and it ends on any head move; the head ring stays on the head. `set_head_to` emits no event of its own, so the views of session state do not wait to be told: the track list is refreshed by the move itself, and the label lane (`useMarkers`), the transcript, the sync-lock toggle and the render-warnings notice (`useRenderWarnings`, which asks for the node on screen) re-read when `head` changes. The History graph is read again on the same terms, from one counter: `useSession`'s `graphVersion` goes up on a head move (a project opening counts, even on the head the last one had), when the agent's turn ends (`agent://done`: `name_node` sets a label and moves no head, so no `node-created` follows), and when `forgetRemoved` runs, which a compaction's end and a refused undo or redo both do (`compact_session` deletes nodes and makes none). App hands it to `GraphView` as `refreshKey`, so causes that land in one render are one read. A preview click is none of these (#602).

The path is kept with the project, so it survives a restart (#453). It is `<project-dir>/.audiograph/trail.json` (`session::trail`), beside `head`, not in `view.json`: the view is disposable by design, and losing the path makes undo skip steps. The file holds `{ head, trail, redo }`, each list bounded to the last 500 heads (`MAX_TRAIL`, the same bound the reducer keeps).

- **Saving.** After every move that changes the path (an edit, an agent step, Set as head, undo, redo) `useSession` hands the whole path to `save_head_trail`. The save is not a turn in the session's line of edits (`lib/sessionQueue.ts`, #498): it writes and syncs a file, and no edit waits for the disk. `lib/trailWriter.ts` keeps at most one save in flight and lets the newest replace any waiting, so a slow disk cannot leave an older path in the file after a newer one. The file is written to a temporary file in the same directory and renamed over `trail.json`, as the head file is, so a reader sees the old file or the new one. Opening a project is not a move and saves nothing; a different project drops a save still waiting for the one left.
- **Reading.** `get_head_trail` runs as a project opens: at launch (`useSession`, beside `get_session_head`) and in `restoreView` for every project opened after (`resetHead(head, saved)`). A missing, unreadable or corrupt file is an empty path, never an error, and never stops a project opening. The backend drops ids that are not node ids and entries naming nodes the project no longer has, so a path read as a project opens names no node a compaction removed. That covers the file, not the path the window holds while the project is open: the next bullet does.
- **After a compaction (#595).** `compact_session` deletes every node off the head's chain and the chain's tail past `keep_last`, and the window's path can name some of them: the redo list, and a step that returned to an earlier state (mute, then unmute, leaves the mute off the chain). When the agent's `compact_session` call ends, `useSession` asks the store which nodes it has (`get_graph`), drops the entries naming the others (the reducer's `drop`: the path closes up over them, as a read does) and saves the path, in the session's line, so an undo pressed meanwhile waits for it. Undo and redo also skip a step the store refuses when its list of nodes no longer has it, which covers a compaction the window did not hear of (one run from the CLI): the step is dropped and the next one tried. Whether a node is gone is read from that list, never from the error's text, and any other refusal is shown with the step kept. The compaction does not rewrite `trail.json` itself: the window is its one writer (`lib/trailWriter.ts`), a second writer would race the window's saves, and a read drops the removed nodes anyway.
- **When it applies.** The reducer takes the saved path only if it ends at the head the project opened on (`restore`, and `saved` of `reset`). A path that ends elsewhere (the last save was lost to a crash, another client moved the store) is not the way to this head and is ignored: undo then steps back through each node's `parent`, as it did before the path was kept, which is right unless the history revisited a state. An edit made before the saved path was read wins over it.
- **Which project.** The save goes to the project that is open when the command runs, and it is dropped unless its head is a node of that project, so a save sent for the project just left cannot put its path in the next one. Save As copies `trail.json` with the rest of `.audiograph/`.

### Storage Format

Nodes are stored as content-addressed JSON files under `<project-dir>/.audiograph/` (`session::STORE_DIR`; layout documented in `crates/session/src/store.rs`):
```
<project-dir>/
  project.json     # project metadata (name, notes, export tags), beside the store
  .audiograph/
    nodes/
      <hex[0..2]>/<blake3-hex>.json   # sharded by the first two hex chars
    head           # plain text: current NodeId
    trail.json     # the path taken to the head, which undo follows
    view.json      # where you were: head, zoom, selection, playhead
    derived/       # audio written by destructive edits, named by content hash
    clipboard/     # copy_region blobs
    previews/      # cached preview renders, keyed by node id (a ranged render by node id and range)
    auditions/     # cached audition_effect excerpts, keyed by content
```

A clip does not name a cache entry. `derived/` and `clipboard/` hold the project's own audio, and a clip may name a file in either; everything else under `.audiograph/` is a cache or a record, and `previews/` and `auditions/` evict their oldest entries to stay under a cap. So `load` of a WAV from any of them copies it into `derived/` instead of naming it where it lies (#580): the file's own bytes, under the name a transcode of the same audio would take, so loading the same audio twice stores it once. A WAV from anywhere else is still named in place, which is what keeps a render of an untouched track the source's own bytes.

The one thing eviction leaves alone is an entry being read again: the A/B bar loads a side from its file on every switch, so `prepare_compare` holds both sides' entries in `previews/` (`PreviewCache::hold`) until `end_compare` or the next compare (#575). The cap is on the entries nobody holds. A held entry is outside it, so a long session whose two sides do not fit in the cap together (an hour-long mix) can still be compared, and a render made while the bar is open evicts only unheld entries to fit; the one refusal is still a render bigger than the cap on its own. One compare is open at a time, so that is one pair. The window holds the mix it plays the same way (#607): the transport loads the file again whenever the timeline mounts, as it does on a switch to the Graph or Transcript tab and back, and an entry the agent's renders had evicted meanwhile was a "Could not load the mix" for the mix the user was just playing. `render_preview`, the window's render for Play, Preview and a graph preview, holds the entry it returns (`AppState::window_mix`, `window_mix.rs`), and the next one replaces the hold, as does a project change; the agent's tools render with no hold and never touch the slot. So `previews/` is bounded by the cap plus one pair plus the window's one mix. A mix gone anyway (a cache cleared by hand) is asked for again by the timeline that finds it missing, once for each path. Dropping the last hold on an entry evicts the cache back under its cap there and then, so once the bar closes the pair counts again and is usually what goes, being the least recently used.
That is also why nothing that frees disk deletes outside `derived/`. The history sweep (`reclaim::sweep`) and the orphan sweep when a project opens (`reclaim::sweep_orphans`) work in `derived/` only, and so does `compact_session` (#589, #593): a pruned node can name the user's own recording where it lies, so `plan_compaction` lists only the files that resolve, as canonical paths, inside `.audiograph/derived/` (so `..` and a symlink cannot lead out), and `apply_compaction` checks each one again before it deletes it, leaving any other path in place and counting it in `kept_files`. A loaded file loses the undo steps that named it, never the file. Clipboard blobs and cached previews are not compaction's to delete either.

Annotations (markers, regions, labels) are part of `SessionState`, so they live inside each node's JSON rather than in a directory of their own.

The label lane (`LabelLane`, below the tracks) edits them directly: double-click the lane to add a label, typing its name in a box where it will sit (a click on the ruler opens the same box at that point); double-click a label to rename it; drag it to move it, and a region moves whole, keeping its length; right-click to delete it. The app asks for text in the page and never with `window.prompt`, which a macOS webview answers with `null` (`noWindowPrompt.test.ts` guards it). Each gesture goes through the session's line (`inOrder`) as `add_marker`, `update_marker` or `remove_marker`: one node each, and a gesture that changes nothing (a label dragged back to where it was) appends none. Labels, and the transcript's words, stay with the audio across the tools that shift time: `remap_after_cut`, `remap_after_insert` and `remap_after_scale` (`crates/tools/src/tool/util.rs`) are called by `cut_range`, `insert_silence`, `paste_region`, `trim`, `change_speed`, `time_stretch`, `truncate_silence`, `cut_words` and `remove_fillers` (`labels_follow_edits.rs`); `repeat_selection` appends at the end and moves nothing. `export_labels` writes them as Audacity label text and `import_labels` reads that back.

### Save As

Save As (`save_project_as`, `project::copy_project`) copies the folder without its caches: the folders directly under `.audiograph/` named in `tools::preview_cache::CACHE_DIRS` (`previews/` and `auditions/`), the same list the asset scope grants by name, so a new cache is left out of copies and readable by the webview by being added there (#576). A folder of the same name anywhere else, the user's own or one under `derived/`, is copied. The copy is a snapshot of the head when it starts (#571), made in two parts. The store's own state (`nodes/`, `head`, `trail.json`, `view.json` and `project.json`) is copied holding the store's lock, so no edit is half-way through and the copied head's node is beside it. The audio and the rest of the folder are then copied with no lock held, while a staged handle the copy keeps (it writes nothing) counts as in flight, so the orphan sweep, the reclaimer and compaction delete nothing until the copy is done; audio is named by its content and otherwise only added to. Before the window moves to the copy, the copy is checked for its head, the head's node, and every file the head plays from `derived/` and `clipboard/` that was there when the lock was let go, and a copy missing one is an error that names it.

The copy's node files are the original's, byte for byte, so their clips name the original's absolute paths, and Save As opens the copy straight away, while the original is still there. The copy reads its own audio all the same (#612): `session::relocate` takes a clip that names a file in another project's `.audiograph/derived/` or `clipboard/`, or in its `generated/`, as this project's file of the same name when this project has it. Those three folders name their files by a hash of the audio with its rate and channel count (`tools::provenance::audio_hash`, whichever writer wrote the file since #620; a mixed-down track by the blake3 of its WAV file, which holds the layout too), so a file of one name is the same audio. A project written before #620 still has edit outputs and generated tones and noise named by the blake3 of their samples alone, which does not say at what rate or in how many channels; nothing is renamed, and a replay counts that older name as a match for the file it writes and rebuilds under the recorded one. A read only ever points a clip at a file that is there: a file of another project's `derived/` that is gone (the copy's own taken by its history sweep, and the original deleted or out of reach) is rebuilt by `rederive` into this project's `derived/` under its name, never into the other project's folder, and found there from then on. The rule is applied wherever the store reads a node, and the store keeps what it works out about each folder for as long as it is open, so the sweeps, which keep what the store's nodes name, keep exactly what the copy plays. Before it, a copy's open swept its whole `derived/` as unnamed and left it playing from the original's folder, and deleting the original then cost it every edit a replay cannot redo. A folder copied by hand and a moved project are read the same way. A copy that a build without the rule already swept is put right when it is opened while the original is still there (`tools::repair`): the files its nodes name that the original has and it lacks are copied in under the same names, planned under the store's lock from the nodes the open lists once for the orphan sweep and for this (a project that is not a copy costs one pass over its clips and no file system call), and copied on a thread of their own, which holds a staged handle so that the sweeps and compaction wait until it is done. It copies the head's audio first, then what no replay can rebuild and the blobs and generated audio, and other history only while `derived/` stays within the history sweep's cap, so the sweep and the repair never undo each other; what it leaves, a replay rebuilds into the copy. It only adds files, never over one (a no-replace rename, or a hard link; on a volume with neither, every copy fails and is logged), keeps each source's modification time for the history sweep's oldest-first order, stops while the disk still has 1 GiB free or at the first copy refused as full, and removes the staging files a killed repair left in `clipboard/` and `generated/`. A file it cannot copy is logged and skipped, and the next open tries again. Every file of the user's keeps pointing where it is, a take loaded in place from inside the original's folder included: Save As copies it, but the copy's clips go on naming the original's, which is not named by its content and is not rebound while it exists. Node files are not rewritten, since a node's id is the hash of its state and the ids are the file names, the parents and the head; so a copied node reads with paths its id was not hashed from, and `revert_to` moves the head to the node it names (`Store::set_head`) instead of appending its state, which would be a duplicate under another id.

---

## 7. Audio Engine

### Architecture

```
SessionState
    │
    ▼
┌────────────────────────────────────────────────┐
│  graph.rs — DSP graph construction             │
│  · One node per clip                           │
│  · Gain nodes per track                        │
│  · Mixdown bus                                 │
└────────────────────┬───────────────────────────┘
                     │
          ┌──────────▼──────────────┐
          │  mixer.rs — apply gains  │
          └──────────┬──────────────┘
                     │
          ┌──────────▼──────────────────────────────┐
          │  render.rs — offline render pipeline     │
          │  · Decode sources (audio-decoder)        │
          │  · Resample to session rate (rubato)     │
          │  · Mix tracks                            │
          │  · Encode to WAV (hound)                 │
          └──────────┬──────────────────────────────┘
                     │
          ┌──────────▼──────────────┐
          │  encode.rs — WAV writer  │
          │  (hound, 32-bit float)   │
          └──────────────────────────┘
```

### Public API

```rust
// crates/audio-engine/src/lib.rs

pub struct Engine;

impl Engine {
    pub fn new() -> Self
    pub fn render_to_wav(
        &self,
        state: &SessionState,
        out: &Path,
        range: Option<TimeRange>,  // None = full session
    ) -> Result<RenderReport>
}

pub struct RenderReport {
    pub frames_written: u64,
    pub sample_rate:    u32,
    pub channels:       u16,
    pub peak_dbfs:      f32,
    pub warnings:       Vec<String>,  // what the render left out; empty when nothing was
}
```

There is no native playback path. All playback happens in the webview — WaveSurfer and `<audio>` elements reading rendered files over the asset protocol, including the A/B crossfade — so the engine never opens an output device. The only native audio I/O in the workspace is microphone capture in `crates/recorder` (cpal input). A cpal output crate (`audio-io`) and a `play_state` entry point existed until [#388](https://github.com/laadtushar/edytlab/issues/388) removed them: nothing called them, and `play_state` pushed a whole mix into a ~250 ms ring buffer that dropped whatever did not fit.

### What the Render Processes

Every track and every clip, with per-clip volume automation, per-track effect chains, sends into buses with their own chains, and the master chain (`crates/audio-engine/src/render.rs`, `effect_chain.rs`). The effect-chain tools edit a track's chain, the master chain with `scope: "master"`, or a bus's chain with `scope: "bus"` and the bus's name (a bus effect is heard only when a track sends to it); `audition_effect` takes the same three targets and stores nothing. `tempo_map` is read by tools — `select_region`'s beat ranges, for one — not by the renderer.

**A stored effect the renderer cannot run does not fail the render** (#520, #531). Only five kinds stream in a chain: `gain`, `limiter`, `low_pass_filter`, `high_pass_filter` and `notch_filter` (`STREAMABLE` in `effect_chain.rs`), and `add_effect`, `set_effect_bypassed` (switching on) and `audition_effect` refuse any other. A project saved by an older build can still hold one, on a track, a bus or the master. `effect_chain::build` leaves such an entry out, as it does a bypassed one, and renders the rest; `skipped_effects` lists every switched-on entry it left out, in signal order (tracks, then buses, then master), each with its scope, its index in the **stored** chain, its kind and why (`NotYetStreamable` or `Unknown`), and `render_warnings` turns each into a sentence. Both read the stored chains, so neither needs a render. Where the list goes:

- `RenderReport.warnings` carries the sentences. The tools that render (`render_preview`, `render_final`, `mix_to_new_track`, `export_multiple`, `audition_effect`) add a `warnings` array and a `Warning: …` line in `summary` when there are any and nothing when there are none. A cached preview has no report, so a hit recomputes them from the node's chains.
- The window gets the same findings as data, not as sentences. `get_render_warnings(node)` returns `RenderWarning[]` (`{ scope, effect_index, kind, reason }`) for any node, `render_range` returns `warnings`, and `prepare_compare` returns `a_warnings` and `b_warnings`; `render_preview` still answers a bare path. The window words them (`lib/renderWarnings.ts`): a notice above the timeline for the node on screen (`useRenderWarnings` asks again whenever that node changes, since a head move emits no event), a note after Export Selection naming the file and what it was written without, and each side's list under the A/B bar. A node id hashes the state, so the answer for an id never changes.

### Fast Path

A whole-session render that reduces to one track playing one source untouched is written by copying the source file's bytes instead of decoding and mixing. The conditions (one unsoloed track with one clip covering the whole source from frame 0, unity gain and pan, no track effects or sends, no active master-chain effect, no volume envelope or stretch/pitch/beat-grid metadata, source rate equal to session rate, no render range) live in `single_track_unity` (`graph.rs`) and `is_unity_passthrough` (`render.rs`). Everything else goes through the mixer.

---

## 8. ML Pipeline

### ONNX Runtime Setup

```rust
// crates/ml-pipeline/src/lib.rs
// Uses ort 2.0.0-rc.13 (pinned with `=`) with load-dynamic feature, built for ONNX Runtime API level 23
// Execution providers: CoreML (macOS) → CUDA (if available) → CPU, behind ort's `coreml` and `cuda` features
// Model files: loaded from disk, cached by blake3 hash
```

The runtime is loaded dynamically from `ORT_DYLIB_PATH`, from the directory the installer puts it in, or from a `libonnxruntime` next to the binary. This avoids linking ONNX into the binary (reduces binary size; allows model updates without recompilation).

`ort` is built for ONNX Runtime API level 23 (its `api-23` feature, declared once in the root `Cargo.toml` together with the exact `=2.0.0-rc.13` pin), and it refuses a library older than its API level, so any runtime from 1.23 on loads (1.31.0 was tried). The level is 23 and not 24 because the runtime to ship is 1.23.2, the newest release with a macOS build for Intel Macs (1.24 and later are arm64 only), so raising the level would strand them. A unit test in `ml-pipeline/src/runtime.rs` fails if it is raised. The reasoning is in the [decision on #383](https://github.com/laadtushar/edytlab/issues/383#issuecomment-6106785529).

`ml_pipeline::runtime::ensure()` does the loading, and every code path that builds an `ort` session (`ModelRegistry::load`, `WhisperModel::load`, `DemucsModel::load`) calls it first. It looks at `ORT_DYLIB_PATH` (a relative path is resolved against the executable's directory only), then the library file name in the bundle directory, then next to the executable. The bundle directory is where the installer puts the library, and the app registers it once in its setup hook (`ml_pipeline::runtime::set_bundle_dir`, refused unless the path is absolute and refused a second time, so nothing running later can redirect the search): `Contents/Frameworks` on macOS, found from the executable at `Contents/MacOS` (an executable anywhere else, such as a `cargo run` build, has none), Tauri's resource directory on Linux (`<exe>/../lib/edytlab`, which is `/usr/lib/edytlab` for a deb and `<mount>/usr/lib/edytlab` for an AppImage, and the executable's own directory in a `target/` build, where it is not counted as a bundle), and none on Windows, because the DLL sits next to the exe, which is searched anyway. `runtime::bundle_dir_for` is that layout as a pure function over the executable path. There is no working-directory, bare-name or system search: a library found in the directory the app was started from could have been planted there (CWE-427), and a system copy may be the wrong version. A candidate that is not an absolute path is refused outright, because the file that was checked and the file the system loader then opens could be two different files. `ort` 2.0.0-rc.12 could not report a library it failed to load: building the error re-entered its own initialisation and the thread blocked forever (through `ort::init_from` too). rc.13 fixed that, and `ort::init_from` now returns an error, but `ort`'s own load on first use (`Session::builder`, `ort::api()`) still panics when it fails, which aborts a release build, and it hands a bare or relative path to the system loader to search. So `ensure()` opens the library itself by absolute path and makes the checks `ort` would (the `OrtGetApiBase` entry point, version 1.23 or newer, the C API version), and hands it to `ort::init_from` only once they pass. A missing library is `Error::MissingRuntime { searched }` and one that fails the checks is `Error::RuntimeLoad`, where a model load used to hang while the agent held the dispatcher, store and engine locks. On macOS a library that is found is not opened on a system older than the runtime's minimum: dyld reports a too-new minimum only lazily, as an addendum to a symbol error, so the check is ours and the answer is `Error::RuntimeUnsupported { needs, running }` (`kern.osproductversion` against `min_macos` in `apps/desktop/src-tauri/ml-runtime.json`, which `runtime.rs` includes at build time, so the pin has one home; a version that cannot be read does not refuse, and neither does `10.16`, the stand-in macOS answers a process that has `SYSTEM_VERSION_COMPAT=1` set, which no macOS ever had; dyld still decides). On Windows the library is opened with `LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32`, so its own imports (`msvcp140.dll`, `msvcp140_1.dll`, `vcruntime140*.dll`) come from its directory and `System32` and are not searched for in the working directory or on `PATH`. `runtime::info()` is `ensure()` with the answer kept: the file that loaded, the version the library reports and which of the three places it came from. The `ml_runtime_status` command returns it for the line in Settings ("ONNX Runtime: 1.23.2 (bundled)", "not installed in this build", or the reason it cannot be used), loading the library on the blocking pool if it has not been. The tools turn either into a tool error that still says the feature is not implemented.

**What the installers carry.** Every release installer after v0.6.1 carries ONNX Runtime 1.23.2 (CPU), the version `ml-runtime.json` pins, with its `LICENSE` and `ThirdPartyNotices.txt`. Each release leg fetches it with `scripts/fetch-ml-runtime.sh`, which checks the archive and the unpacked library against the manifest's SHA-256s, and builds with its platform's overlay, `apps/desktop/src-tauri/tauri.ml-runtime.<linux|macos|windows>.conf.json`, passed with `--config`; the base `tauri.conf.json` names none of it, so a build that has not fetched anything still builds. Where each file goes:

| Installer | Library | Notices |
|---|---|---|
| `.app` / `.dmg` | `Contents/Frameworks/libonnxruntime.dylib` (`bundle.macOS.frameworks`, signed with the app in a signed build) | `Contents/Resources/licenses/onnxruntime/` |
| `.deb` / AppImage | `/usr/lib/edytlab/libonnxruntime.so` (`usr/lib/edytlab` inside the AppImage) | `/usr/lib/edytlab/licenses/onnxruntime/` |
| `.msi` / NSIS | `onnxruntime.dll` beside `edytlab-desktop.exe`, with `msvcp140.dll`, `msvcp140_1.dll`, `vcruntime140.dll` and `vcruntime140_1.dll` (14.40 or newer, copied from the build machine's Visual Studio by `scripts/installer-ml-runtime.py stage-vc-runtime`; the app links its own C runtime statically, so only the library needs them) | `licenses/onnxruntime/` and `licenses/vc-runtime/NOTICE.txt` in the install folder |

After the build and before any upload, `scripts/installer-ml-runtime.py check` opens each installer (`dpkg-deb -x`, `--appimage-extract`, the `.app` and its `.tar.gz`, the mounted `.dmg`, `msiexec /a`, `7z`) and fails unless every file the overlay adds is there byte for byte, the library has the manifest's `library_sha256` (a signed macOS build re-signs the dylib, so there its signature and team are checked instead), every Visual C++ runtime DLL the installed `onnxruntime.dll` imports sits beside it, and the app binary from that installer, run as `edytlab-desktop --ml-runtime-status` (which prints the `ml_runtime_status` answer as JSON and exits without a window), loads that file as 1.23.2 and calls it bundled. `crates/ml-pipeline/tests/runtime_installers.rs` holds the overlays to `runtime::bundle_dir_for`, and `crates/tools/tests/release_ml_runtime.rs` holds both release workflows to fetch, overlay and check before they upload.

`ml_pipeline::fetched_model_path` downloads, verifies and caches a pinned artifact (URL, size and SHA-256 per file): it streams to a `.part` file, resumes with `Range`, re-hashes the `.part` from disk before renaming it into place, refuses a truncated or wrong-hash file and fetches it again once, can be cancelled, and runs its HTTP request on its own thread so it is safe to call from async code. It holds an OS lock on `<root>/<id>.lock` for the length of a fetch, so a second edytlab process (or the CLI) on the same data directory waits instead of writing into the same `.part`, and the re-hash means a writer that ignores the lock still cannot get an unverified file given its final name. Only https URLs are accepted (plain http only for localhost), and at most five redirects are followed. It has no callers and no model is pinned, so nothing is downloaded today. The app now looks where the installers will put the runtime library, but nothing ships it or sets `ORT_DYLIB_PATH` yet (the release workflows are the next step), so no model can load in a user's install and Settings says the runtime is not installed in this build. Shipping the runtime and the download UI is the rest of [#383](https://github.com/laadtushar/edytlab/issues/383).

### Whisper (Transcription)

```
Input: WAV (any sample rate)
    │
    ▼  resample to 16 kHz mono (rubato)
    │
    ▼  log-mel spectrogram (80 bins, 30-second window)
    │
    ▼  Whisper encoder + decoder (ONNX Whisper-base) — NOT IMPLEMENTED
    │
    ▼  word-level timestamps via DTW alignment
    │
Output: Vec<WordTimestamp> stored in SessionState.transcript

NOT IMPLEMENTED IN THIS BUILD. `WhisperModel::transcribe` returns
`NotImplemented` once the input passes validation, whatever model is
loaded. The diagram is the intended design, not current behaviour.
```

Four of its pieces exist ahead of the rest. They need no ONNX Runtime and no model, they are checked in CI, and nothing calls any of them:

- the log-mel spectrogram (`crates/ml-whisper/src/mel.rs`, compared with OpenAI's `log_mel_spectrogram`, [#554](https://github.com/laadtushar/edytlab/pull/554));
- the decoding of token ids to text from the export's `tokenizer.json` (`tokenizer.rs`, byte-level BPE, [#555](https://github.com/laadtushar/edytlab/pull/555));
- the greedy decoding loop (`decode.rs`, a port of `whisper/decoding.py`'s main loop, language detection, token suppression and timestamp rules, [#556](https://github.com/laadtushar/edytlab/pull/556)), with every model call behind a `DecoderStep` trait that the tests drive with fixed logits;
- word timing from the decoder's cross-attention (`timing.rs`, a port of `whisper/timing.py`'s median filter, DTW and punctuation merge, [#557](https://github.com/laadtushar/edytlab/pull/557)), tested on made-up attention weights.

What is not written is everything that needs the runtime and the model: the ONNX encoder and decoder behind `DecoderStep`, loading the model, chunking long audio into 30-second windows, and the tool wiring.

Designed to run entirely on-device. There is no transcription speed to quote until the decoder exists ([#384](https://github.com/laadtushar/edytlab/issues/384)).

### Demucs (Stem Separation)

```
Input: stereo audio (any sample rate)
    │
    ▼  resample to 44100 Hz (engine native rate)
    │
    ▼  htdemucs ONNX model
    │  (waveform encoder + spectrogram encoder + dual-path transformer + decoder)
    │
Output: 4 stems (vocals / drums / bass / other)
        each written as a separate file, added as new tracks
```

NOT IMPLEMENTED IN THIS BUILD. `DemucsModel::separate` returns
`NotImplemented`; a valid model loads and separation still fails,
because the ORT decode loop is what is missing. The diagram is the
intended design, not current behaviour.

Two of its pieces exist ahead of the rest, are checked in CI without a
model, and are called by nothing: the STFT and iSTFT that htdemucs runs
outside its ONNX graph (`crates/ml-demucs/src/spec.rs`, compared with
PyTorch, [#549](https://github.com/laadtushar/edytlab/pull/549)), and the
cut, pad, weight and overlap-add of a track into the network's 7.8-second
segments (`separate.rs`, behind a `SegmentModel` trait and tested with a
fake model, [#550](https://github.com/laadtushar/edytlab/pull/550)).
Nothing implements `SegmentModel` with ONNX Runtime yet.

Model variants available (`SUPPORTED_MODEL_IDS`):
- `htdemucs` (default)

`htdemucs_ft` is **not** supported: it is a bag of four fine-tuned
models, one per stem (four times the download and four times the
inference), and there is no pinned ONNX export of it ([#385](https://github.com/laadtushar/edytlab/issues/385)).
So there is no out-of-memory fallback to a smaller model either.
`htdemucs_6s`, with guitar and piano stems, is **not** supported
either: the tool rejects any id outside the list above. This section
used to list it (#233).

---

## 9. Memory and Skills

### Memory System

Two scopes, both stored as plain Markdown:

| Scope | Path | Edited via |
|-------|------|-----------|
| Global | `~/.edytlab/memory.md` | `read_memory` / `write_memory` commands |
| Project | `<project>/.edytlab/EDYTLAB.md` | same commands with `scope = "project"` |

Memory fragments are injected into the system prompt on every turn:

```xml
<edytlab-memory scope="global">
…global notes…
</edytlab-memory>
<edytlab-memory scope="project">
…project-specific notes…
</edytlab-memory>
```

The agent sees memory on every turn but has no tool to write it: none of the 94 registered tools touches memory. You edit it in Settings → Memory (the `read_memory` / `write_memory` commands), which is how BPM, speaker names or style preferences carry across turns and sessions.

### Skills System

Skills extend the agent's capabilities without modifying core code. They are Markdown files with YAML frontmatter stored under `~/.edytlab/skills/`.

```yaml
---
description: "Compress and EQ for podcast voice"
trigger: keywords
keywords: [podcast, voice, spoken word, interview]
enabled: true
---

When working on spoken-word content, apply gentle compression
(ratio 3:1, attack 10ms, release 80ms) before EQ...
```

Trigger types:
- `always` — injected into every turn
- `keywords` — injected when any keyword appears in the user's message
- `regex` — injected when the pattern matches the user's message

Matching skills are appended to the system prompt before the turn executes.

The skills in `apps/desktop/src-tauri/resources/skills/` ship inside the app, and `install_bundled_skills` keeps them current in the user's folder at every launch without touching what the user changed (`bundled_skills.rs`, #574). `~/.edytlab/skills/.bundled.json` records the hash of the text it installed for each file. A file whose hash still matches is replaced when the bundled text changes; an edited one is left alone, with the new text written beside it as `<name>.bundled.md` (the loader skips that name); a deleted one is not brought back. An install from before the record counts a file as unedited only if its text is a version that shipped before the record existed, listed in `pre_manifest_skills.json`. That list is frozen: it describes installs with no manifest and nothing else, so adding to it would make such an install take a skill added later for one the user deleted. A new or changed bundled skill needs no entry; `the_pre_manifest_list_never_changes` holds the list to that.

---

## 10. Agent Profiles and MCP Servers

### Agent Profiles

Override model, tool whitelist, and system-prompt addendum per session. Stored under `~/.edytlab/agents/`.

```yaml
---
description: "Podcast production — fast, focused"
model.provider: anthropic
model.id: claude-haiku-5-5
tools: [load, cut_range, normalize, trim, transcribe, render_final]
---

Focus on efficient spoken-word editing. Keep operations minimal.
```

Active profile is selected from Settings and persisted as a `.active` sidecar file in the profiles directory (`AppState::set_active_agent_profile`).

### MCP Servers (Phase 5)

edytlab supports the Model Context Protocol for extending the agent with external tools. Configured in `~/.edytlab/mcp.json`.

```json
{
  "servers": {
    "my-server": {
      "command": "/usr/local/bin/my-mcp-server",
      "args": ["--config", "/path/to/config.json"],
      "env": { "MY_SECRET": "<keychain:my_secret>" },
      "enabled": true
    }
  }
}
```

Transport types: `stdio` (JSON-RPC over stdin/stdout; `command`/`args`/`env`, as above) and `sse` (HTTP Server-Sent Events; `url` and `headers` instead). The transport is inferred from which fields are present (`McpServerConfig` in `crates/mcp/src/config.rs` is untagged). A `<keychain:slot>` value in `env` is replaced with that keychain secret when the server launches.

The MCP layer starts registered servers at app launch, discovers available tools via `tools/list`, and injects them into the agent's tool list alongside built-in tools. On Ollama the slim selection applies to them like any built-in tool (see [Request size](#request-size-and-small-context-models)): an MCP tool is sent in full when a message names it and otherwise only listed by name, after the built-in tools, and a long MCP tool list ends the names line with "and N more" (the built-in tools are never what it leaves out).

---

## 11. IPC and Event System

### Commands (Request/Response)

All Tauri commands are invoked via `@tauri-apps/api/core:invoke`. They are synchronous from the frontend's perspective (returns a Promise).

```typescript
// invoke("command_name", { arg1, arg2 })
const head = await invoke<NodeId>("get_session_head");
```

Errors propagate as Promise rejections. The `tauri-bridge.ts` layer wraps these into typed async functions.

### Events

The agent turn emits events via Tauri's event system (names in `apps/desktop/src-tauri/src/events.rs`). The frontend subscribes with `listen()`, wrapped by `tauri-bridge.ts`. These are Tauri events, not SSE — SSE is only the wire format between `crates/ai` and the LLM provider.

| Event name | Payload | When emitted |
|-----------|---------|-------------|
| `agent://text-delta` | `{ text: string }` | Each text chunk streamed from the LLM |
| `agent://tool-call` | `{ name: string, id: string }` | Tool execution starts |
| `agent://tool-call-end` | `{ id: string, ok: boolean, not_run: boolean, view?: ToolView }` | Tool execution completes, or the call was announced and will never run (`ok: false`, `not_run: true`: declined, reworded, unanswered, or over the tool budget) |
| `agent://node-created` | `{ node_id: string }` | An agent tool call moved the session's head, to a new node or one already there (`revert_to`); `node_id` is the store's head after the call. Not sent for a call that left the head where it was (`name_node`, a read) |
| `agent://done` | `{}` | Turn complete (no more tool calls) |
| `agent://plan` | `{ steps: object[] }` | A plan, or the held first edit, awaits approval; the turn is suspended (see [Plan first](#plan-first)) |
| `agent://plan-rejected` | none | The user declined the plan or the held edit; the turn ended with no `done` |
| `agent://plan-unavailable` | `{ reason: string, first_edit_held: boolean }` | A plan was asked for and none arrived; `first_edit_held` says whether the first edit will be held for approval or the turn proceeds with no gate |
| `tool-progress` | `ToolProgress` | Progress from a long-running tool (and `select_region`'s match) |
| `marker-changed` | none | Marker/annotation added, renamed, moved or removed |

### Lock Ordering (Deadlock Prevention)

`AppState` holds `Arc<Mutex<Store>>` and `Arc<Mutex<Engine>>` separately. The invariant documented in `CLAUDE.md`:

```rust
// CORRECT — drop store lock before acquiring engine lock
let state = {
    let store = lock_std(&app_state.store, "store")?;
    store.get(id)?
};
// store lock dropped here
let mut engine = lock_std(&app_state.engine, "engine")?;
engine.render_to_wav(&state.state, ...)?;

// WRONG — holding both locks simultaneously causes deadlock
let store = lock_std(&app_state.store, "store")?;
let engine = lock_std(&app_state.engine, "engine")?;  // DEADLOCK RISK
```

The agent's tool dispatch (`tools::Shared::dispatch`) follows the same order:

1. The dispatcher lock is taken alone, to admit the call and clone the tool, and released before any other lock is taken. (The agent used to take dispatcher then store, while `run_track_tool` and `batch_load` take store, engine, then dispatcher: two orders, a deadlock waiting for the right timing.) This is a rule about the agent's dispatch, not about the dispatcher everywhere: `refresh_dispatcher_for_server` holds the dispatcher while it takes the MCP registry's lock, and dispatcher then registry is the order in use, so the reverse must not be added.
2. A tool that runs off the lock (see [Off-lock tools](#off-lock-tools)) takes only the store lock, twice and briefly: to stage a handle, and to commit it. Nothing is locked while it runs.
3. Any other tool takes store, engine, then clipboard, and keeps them for the call.

The registry of live stores (`session::live::Registry`, [#534](https://github.com/laadtushar/edytlab/pull/534), moved into `session` by #566) is a leaf: its lock is taken alone, to look a folder up or register a store, and none of the store, engine, dispatcher, agent or `AppState` locks is taken while it is held, and no store is dropped under it (a folder's identity is worked out, and a store built, outside it). It may itself be taken while a store or engine lock is held (a tool that opens folders of its own, `batch_apply`, does), which is safe because it waits for nothing. In `open_project_with` the order is registry (alone), then the store's lock (held across the re-read, the sweep and the swap), then, with the store lock dropped, `AppState::set_open_project`, which takes the store slot's lock and then the project directory's. The one other path that takes several of these is `release_replaced_project` (a project folder replaced at the same path, #566): the agent's lock (`try_lock_owned`, never waited for), then the registry (`Store::replaced_holder`, alone), then the old store's lock (`try_lock`, never waited for, and held until the window has let go, so no command can stage an edit between the check and the release), then the `store` slot, then `project_dir` (`AppState::release_open_project_if_exclusive`), and last, with every one of those let go, the window-mix hold is released (`AppState::release_window_mix`, as `set_open_project` does), because that lock is taken alone (#607). It is the only path that holds a store's lock across the window's slots. It reverses nothing: the agent's lock is taken before the slot everywhere (`rebuild_agent_into` does too, and a turn holds it across the whole turn), and no path takes a store's lock while holding the slot (`store_handle()` drops the slot before its caller locks the store).

The preview cache's holds (`HELD` in `crates/tools/src/preview_cache.rs`, #575) are behind a lock that is the last one taken: a render takes it after its staging file is renamed into place, with whatever the caller holds (a tool holds the store and engine locks), and keeps it only while it evicts, which lists the cache folder and removes files. Dropping the last hold on an entry takes it too, and evicts under it. The open compare's slot (`AppState::compare`) drops holds while it is locked, so it is locked with nothing else held: `prepare_compare` takes it alone to begin a compare (after holding the new pair, so ending the old one cannot evict a side the two share) and again, after both sides are rendered, to keep their holds, and `end_compare` takes nothing else. The window's mix slot (`AppState::window_mix`) is locked the same way: `render_preview` holds the entry before it looks it up, takes the slot alone once the render has succeeded, and drops the hold it replaced after the slot is unlocked; `set_open_project` takes it alone after the store and project slots are released.

A command that reads the session while an off-lock tool runs gets the head as it was when the tool started. The tool's node appears when it commits, and `agent://node-created` is emitted after that, so a refresh triggered by the event sees it.

Save As (`project::copy_project`) takes the store lock alone, to copy the history and stage the handle that keeps the sweeps off its copy, and lets go of it before the audio is copied. It takes no other lock; opening the copy afterwards follows `open_project_with`'s order.

---

## 12. Security Model

### API Key Storage

- Keys are stored in the **OS-native keychain** (macOS Keychain, Windows Credential Manager, the Secret Service on Linux; with none running, the kernel keyring for the current boot only — see [Keychain Integration](#keychain-integration))
- edytlab never transmits keys to its own servers
- Keys are read at runtime, signed into HTTP requests in-process, and sent directly to provider endpoints
- Keys are never logged or written to disk outside the OS keychain

### Audio Privacy

- Audio processing runs 100% in-process
- ONNX models (Demucs, Whisper) are designed to run locally; neither runs yet (#383–#385), and audio bytes never leave the machine
- The only network traffic is LLM API calls (text tokens only)

### Tauri Permissions

Tauri's security model requires explicit capability declarations. `apps/desktop/src-tauri/capabilities/default.json` grants only `core:default` and the dialog plugin (`dialog:default`, `dialog:allow-open`) for the file pickers. There is no filesystem plugin: reading source audio and writing renders happen in Rust commands.

The asset protocol (`tauri.conf.json` → `app.security.assetProtocol`) starts scoped to `$APPDATA/**`; the app adds each project's folder as it opens (`allow_assets_in_dir` in `commands.rs`), with its `previews/` and `auditions/` caches by name, which is where both a preview and each side of an A/B compare live, and each individual file it hands the timeline (`allow_asset_file`), rather than everything the user can read.

macOS hardened runtime entitlement allows outbound HTTPS to LLM provider endpoints.

### Content Security Policy

The WebView is CSP-restricted. The Tauri shell never fetches arbitrary URLs from the renderer process. All network calls go through Rust (`reqwest`).

---

## 13. Data Flow: Single User Turn

Full end-to-end trace from user input to UI update:

```
[User] types message in Chat
    │
    ▼ React → tauri-bridge.sendMessage(text)
    │
    ▼ invoke("send_message", { text, disabledTools }) → Rust commands.rs
    │
    ▼ acquire Agent lock → agent.turn(text, on_event)
    │
    ▼ Build system prompt:
    │   base instructions
    │   + memory.render()  (global + project markdown)
    │   + matching skill bodies
    │   + profile body (if active)
    │
    ▼ Serialize with LlmProvider::serialize_request()
    │
    ▼ HTTP POST → provider endpoint (SSE)
    │
    ┌─────────────────────────────────────────────────┐
    │  FOR EACH SSE CHUNK:                            │
    │    parse_stream_chunk() → StreamEvents          │
    │    TextDelta → emit agent://text-delta          │
    │    ToolUseStart → emit agent://tool-call        │
    │                → dispatch to ToolDispatcher     │
    │                → tool mutates Store/Engine      │
    │                → Store appends new NodeId       │
    │                → emit agent://node-created      │
    │                  (if the store's head moved)    │
    │                → emit agent://tool-call-end     │
    │    If more tool_calls → loop back               │
    └─────────────────────────────────────────────────┘
    │
    ▼ emit agent://done → React updates UI
    │
    ▼ [User] sees text streamed into Chat,
         tool badges in MessageBubble,
         waveform updates on Timeline,
         new node in GraphView
```

---

## 14. Extension Points

### Adding a New LLM Provider

1. Add a struct implementing `LlmProvider` in `crates/ai/src/provider.rs`
2. Add it to `SUPPORTED_PROVIDER_IDS` and the `provider_from_id()` factory
3. Handle request serialization (`serialize_request`) and stream parsing (`parse_stream_chunk`) — an OpenAI-compatible API can delegate both to `OpenAIProvider`, as Groq, Gemini and Ollama do
4. Give it an arm in `list_models_for_at()` in `crates/ai/src/models.rs`, or picking it shows "unsupported provider id" where the model list belongs
5. Update the `ProviderId` TypeScript union in `tauri-bridge.ts` and the `PROVIDERS` list in `components/Settings.tsx`

No per-provider keychain code is needed: the slots are keyed by provider id (`<id>_api_key`, `<id>_model`, `<id>_base_url`), and the commands in `commands.rs` check ids against `SUPPORTED_PROVIDER_IDS`. A keyless provider overrides `requires_api_key()`. A provider whose models usually run with a small context window (a local one) overrides `tool_set()` to `ToolSet::Slim`; every hosted provider keeps the default, `Full`, and Anthropic must, for its prompt cache (see [Request size and small-context models](#request-size-and-small-context-models)).

### Adding a New Tool

1. Create `crates/tools/src/tool/<name>.rs` implementing the `Tool` trait, and export it from `crates/tools/src/tool/mod.rs`
2. Return the tool descriptor, including the JSON schema for the input, from `schema()`
3. Register it in `ToolDispatcher::default_dispatcher()` in `crates/tools/src/dispatcher.rs`
4. Tests: cover happy path, invalid input, edge cases (empty session, out-of-range times)
5. Regenerate [tools-reference.md](./tools-reference.md) (`UPDATE_TOOLS_REFERENCE=1 cargo test -p tools --test tools_reference_doc`) and add the tool to `website/app/docs/tools/page.tsx` — both are checked by tests
6. Nothing to do for local models: Ollama is sent a core of common tools plus the ones a message names, so a new tool reaches it by being named, and is listed by name in the system prompt until then (a test checks that every permitted tool is sent or listed). If it is an edit a small model should see without being asked, add it to `SLIM_CORE_TOOLS` in `crates/ai/src/tool_selection.rs`; a test holds the core's compacted size within its budget. If people will ask for it in words that are not in its name ("less harsh" for `de_esser`), add a `Cue` there; the three phrasings in `request_size.rs` show how

### Adding a Skill

Drop a `.md` file with YAML frontmatter into `~/.edytlab/skills/`. No recompile needed.

To change a skill that ships with the app, edit it under `apps/desktop/src-tauri/resources/skills/`; nothing else needs updating. Users who have not edited the old copy get the new one at their next launch. Do not add the new text to `apps/desktop/src-tauri/src/pre_manifest_skills.json` (see above); `node scripts/pre-manifest-skill-hashes.mjs --check` verifies that file against the git history (full history needed: `git fetch --unshallow` in a shallow clone).

### Adding an Agent Profile

Drop a `.md` file with YAML frontmatter into `~/.edytlab/agents/`. No recompile needed.

### Adding an MCP Server

Edit `~/.edytlab/mcp.json` via Settings → MCP, or directly. Tools from the server become available to the agent on next restart.

---

*Kept current by the staleness sweep in `CLAUDE.md`; it describes `main`, not a release.*
