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
│  commands.rs (~86 commands)  ·  state.rs (AppState)  ·  events.rs          │
└───────────┬──────────┬────────────────────┬──────────────┬──────────────────┘
            │          │                    │              │
     ┌──────▼──┐  ┌────▼────┐  ┌──────────▼───┐  ┌──────▼──────┐
     │  crates/│  │ crates/ │  │    crates/   │  │   crates/   │
     │  ai     │  │ tools   │  │    session   │  │   audio-*   │
     │  agent  │  │ 93 tools│  │    DAG store │  │   ml-*      │
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
    ├── ml-pipeline/              # Shared ONNX runtime gate, model downloader (no callers yet) and inference cache (runtime not shipped, #383)
    ├── ml-whisper/               # Transcription via ONNX Whisper-base (decoder is a stub, #384)
    ├── recorder/                 # Microphone capture to WAV (cpal)
    ├── session/                  # DAG data model, node store, fork/diff/compare
    ├── skills/                   # User skill library with trigger evaluation
    └── tools/                    # 93 deterministic audio-editing tools
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
│   │   │   ├── Ruler
│   │   │   └── MarkerLayer
│   │   ├── TranscriptPane
│   │   └── GraphView (xyflow DAG visualization)
│   └── RightPane
│       └── Chat
│           ├── MessageBubble[]
│           │   └── ToolBadge[]
│           ├── ThinkingIndicator
│           ├── EmptyState (no audio loaded)
│           └── ChatInput + CapabilitiesMenu
├── ABCompareBar (when compareMode active)
├── ShortcutsOverlay (? key)
└── ErrorBanner (API key / render errors)
```

The tree is conceptual: `PlaybackControls`, `MainContent`, `LeftPane`/`RightPane` and `ChatInput` are regions of `App.tsx`, `AppHeader.tsx` and `Chat.tsx`, not components of their own.

### State Management

All session state lives in Rust (`AppState`). The frontend is intentionally thin — it:
1. Sends commands via `tauri-bridge.ts`
2. Receives Tauri events (`agent://text-delta`, `agent://tool-call`, `agent://node-created`, `agent://done`, …)
3. Derives local UI state from the command responses

Local React state: `head`, `audioPath`, `rendering`, `leftView`, `compareMode`, `markers`, `tracks`, `showShortcuts`, `selection`. Most of it is not persisted; where you were — head, zoom, selection and playhead — is saved to `<project>/.audiograph/view.json` and restored on open (`src/lib/viewState.ts`, `src-tauri/src/project.rs`).

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
| A turn whose calls only read the session | Never held. |

With no answer within five minutes nothing runs, each held call is answered "not
run" (`ToolCallNotRun`), and the turn ends with `PlanTimeout`.

"Would change the session" is `Tool::mutates()`, which defaults to `true`: a new
tool, and every MCP tool, is held unless it is deliberately marked read-only.
`crates/tools/tests/read_only_tools.rs` pins the read-only list by name, so
widening it is a reviewed change. `ToolDispatcher::would_mutate` answers for a
specific call and shares its pre-dispatch checks with `invoke`, so a call that
would be refused (turned off, unknown, invalid arguments) is not held.

The shared approval path (arming the gate, the five-minute timeout, the card's
steps, the bookkeeping for a step that does not run) is
`crates/ai/src/approval.rs`.

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
| `anthropic` | `https://api.anthropic.com` | `x-api-key` header | `claude-sonnet-4-6` | Native Anthropic format |
| `openrouter` | `https://openrouter.ai/api` | `Authorization: Bearer` | `claude-sonnet-4-6` | Anthropic-compatible API; prepends `"anthropic/"` to unqualified model ids |
| `openai` | `https://api.openai.com` | `Authorization: Bearer` | `gpt-4o-mini` | Full translation: Anthropic shape → chat-completions → back |
| `groq` | `https://api.groq.com/openai` | `Authorization: Bearer` | `llama-3.3-70b-versatile` | Chat-completions; reuses `OpenAIProvider`'s translation |
| `gemini` | `https://generativelanguage.googleapis.com/v1beta/openai` | `Authorization: Bearer` | `gemini-2.0-flash` | Gemini's OpenAI-compatible endpoint; reuses the same translation |
| `ollama` | `http://localhost:11434/v1` | none (`requires_api_key() == false`) | `llama3.2` | Local daemon, OpenAI-compatible; reuses the same translation |

Every provider's base URL can be overridden per provider from Settings (`<provider>_base_url` in the keychain).

### Reasoning Effort

Anthropic's Messages API takes `output_config: {"effort": "low" | "medium" | "high" | "xhigh" | "max"}`; Settings exposes it for Anthropic only (`<provider>_effort` in the keychain, `get_effort_for` / `set_effort_for`). `LlmConfig::effective_effort()` drops it for any provider whose `supports_effort()` is false, so OpenRouter and the chat-completions providers never receive the field. Three consequences are handled in `agent_loop.rs`:

- The classifier request never carries it: its cheap model answers `effort` with a 400, and `classify_mode` swallows errors. It is sent with `thinking: {"type": "disabled"}` (Anthropic wire format only; the Haiku models accept it) and `max_tokens` 256, because a model that thinks first would otherwise spend the cap before its one word and the mode would silently fall back to `general`.
- A set effort raises `max_tokens` (`Effort::min_max_tokens`), because thinking counts against it; the one-shot reply reader takes the first `text` block, not `content[0]`. With no effort set the caps are `DEFAULT_MAX_TOKENS` (a turn's step) and `PLAN_MAX_TOKENS` (the plan request), both 8192: Anthropic's 5.x models think by default whether or not an effort is sent.
- `thinking` / `redacted_thinking` blocks are kept in the assistant history and replayed first, signature intact, for as long as the turn that produced them lasts; block and delta types the client does not model are skipped. The next turn drops them (below).

### Thinking blocks and the request prefix

On Anthropic's 5.x models (Fable 5.1, Opus 5.5, Sonnet 5.5, Haiku 5.5) a `thinking` block's signature is bound to the request that produced it: the top-level `system`, the `tools` set, and every message before it. Replayed under a different prefix it is a 400 `invalid_request_error` ("Invalid `signature` in `thinking` block. The block is bound to a different conversation"). The API enforces this by default for accounts created on or after 2026-08-31, and for any request that sets `thinking.block_binding.prefix_mismatch_behavior`.

edytlab changes the prefix between user turns: `system` is rebuilt every turn (the classifier's mode, matched skills, memory, the agent profile, and the session context with the head node id and the selection), and `tools` follows that turn's whitelist (the per-turn disabled-tools list). So `run_turn` calls `strip_thinking` once at the start of every turn, removing every `thinking` and `redacted_thinking` block from the history before the new user message is added. The docs list removing all of them as valid; the model only loses that reasoning. Text, `tool_use` and `tool_result` blocks, and their pairing, are untouched.

Within a turn nothing changes: `system` and `tools` are built once before the request loop and sent unchanged on every step, so the thinking a step produced is replayed, signature intact, with the tool results that follow it. That is the one case where replaying is valid, and it holds only while that stays true: **a change that rebuilds `system` or `tools` between a turn's steps must strip there too.** `tests/prior_turn_thinking.rs` pins both halves with a mock provider (a changed `system`, a changed tool set, and a fixed prefix inside a turn), and `tests/live_thinking_binding.rs` is the `#[ignore]`d check against the real API with the strict check switched on (`ANTHROPIC_E2E_KEY`).

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
pub fn load_api_key(provider_id: &str) -> Option<String>
pub fn save_api_key(provider_id: &str, key: &str) -> Result<(), keyring::Error>
pub fn delete_api_key(provider_id: &str) -> Result<(), keyring::Error>
pub fn load_active_provider() -> Option<String>
pub fn save_active_provider(provider_id: &str) -> Result<(), keyring::Error>
// plus load_/save_/delete_ for `base_url`, `model` and `effort`, keyed the same way
```

Keychain slots (accounts):
- `<provider>_api_key` — one per hosted provider (`anthropic_api_key`, `openrouter_api_key`, …)
- `active_provider` (stores provider id string)
- `<provider>_model` (per-provider model choice)
- `<provider>_base_url` (per-provider endpoint override)
- `<provider>_effort` (per-provider reasoning effort; absent means the model's default)

Builds from before multi-provider support stored the Anthropic key as `anthropic_api_key`, which is already the new name for that slot, so it is read as-is — there is no migration step.

On Linux the `keyring` crate is built with only its `linux-native` feature, which is the kernel's in-memory `keyutils` store: entries do not survive a reboot.

### Model Catalogue

`crates/ai/src/models.rs` returns a static curated list for Anthropic and fetches the live list for the other five (OpenRouter's `GET /api/v1/models`; OpenAI, Groq, Gemini and Ollama from their OpenAI-style models endpoint under the configured base URL), with a 10-minute TTL cache. The combo picker in Settings surfaces these alongside free-form input so new model ids work immediately.

### Constants

```rust
pub const DEFAULT_MODEL: &str = "claude-sonnet-4-6";
pub const CLASSIFIER_MODEL: &str = "claude-haiku-4-5-20251001";
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
    pub fn default_dispatcher() -> Self     // registers all 93 built-in tools
    pub fn register(&mut self, tool: Box<dyn Tool>)
    pub fn unregister_prefix(&mut self, prefix: &str) -> usize  // MCP `<server>__` tools
    pub fn tool_schemas(&self) -> Value     // sent to the LLM
    pub fn would_mutate(&self, name: &str, args: &Value, allowed: Option<&HashSet<String>>) -> bool
    pub fn invoke(&self, name: &str, args: Value, ctx: &mut ToolContext) -> Result<ToolResult>
}
```

### The Tools

There are 93, registered in `ToolDispatcher::default_dispatcher()` with their implementations under `crates/tools/src/tool/`. The full list, with every parameter, is [tools-reference.md](./tools-reference.md) — generated from the registry, so it cannot drift from what the agent can call.

Two of them are stubs: `separate_stems` (Demucs, [#385](https://github.com/laadtushar/edytlab/issues/385)) and `transcribe` (Whisper, [#384](https://github.com/laadtushar/edytlab/issues/384)) return an error in this build. `cut_words`, `remove_fillers`, `duck_under_speech` and the phrase/speech modes of `select_region` need a transcript, which only `transcribe` produces, so they cannot run yet either.

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
}
```

### DAG Operations

```
head → N3 → N2 → N1 → root

Fork:
head → N3 ──────────────── (existing branch)
             └→ N4 → N5   (forked branch — new head)

Revert:
  set_head(N2)  →  head now points to N2, N3/N4/N5 still exist (no deletion)

A/B Compare:
  prepare_compare(A, B) renders both to temp WAVs
  accept_b(B) moves head to B
```

### Undo and redo

A node's id is the hash of its state alone, so an edit that returns to an earlier state (mute, then unmute) lands on a node that already exists, and that node keeps the parent it had the first time it was reached (`Store::append` skips the write; pinned by `a_state_reached_again_keeps_its_first_parent`). The graph does not record the route the user took, so undo cannot follow `parent` (#398).

The frontend keeps the path instead: `apps/desktop/src/lib/headTrail.ts`, held by `useSession`, is one reducer for the head, the undo trail and the redo list. Every head move in the frontend goes through it (`setHeadLocal` for a head that moved on, `resetHead` for a different project); a new head clears redo. Undo goes to the last head on the trail and asks the node for its `parent` only once the trail is spent. The History graph draws the path over the parent edges.

The History graph's "Set as head" is one of those moves. `GraphView` hands the choice to App (`onSetHead`), which calls `set_head_to` and then records the step with `setHeadLocal`, so undo returns from the jump (#453). `set_head_to` emits no event of its own, so the views of session state do not wait to be told: the track list is refreshed by the move itself, and the label lane (`useMarkers`), the transcript and the sync-lock toggle re-read when `head` changes.

The path lives for the session. After a restart the trail is empty and undo steps back through each node's `parent`, which is right unless the history revisited a state.

### Storage Format

Nodes are stored as content-addressed JSON files under `<project-dir>/.audiograph/` (`session::STORE_DIR`; layout documented in `crates/session/src/store.rs`):
```
<project-dir>/
  project.json     # project metadata (name, notes, export tags), beside the store
  .audiograph/
    nodes/
      <hex[0..2]>/<blake3-hex>.json   # sharded by the first two hex chars
    head           # plain text: current NodeId
    view.json      # where you were: head, zoom, selection, playhead
    derived/       # audio written by destructive edits, named by content hash
    clipboard/     # copy_region blobs
    previews/      # cached preview renders, keyed by node id
```

Annotations (markers, regions, labels) are part of `SessionState`, so they live inside each node's JSON rather than in a directory of their own.

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
}
```

There is no native playback path. All playback happens in the webview — WaveSurfer and `<audio>` elements reading rendered files over the asset protocol, including the A/B crossfade — so the engine never opens an output device. The only native audio I/O in the workspace is microphone capture in `crates/recorder` (cpal input). A cpal output crate (`audio-io`) and a `play_state` entry point existed until [#388](https://github.com/laadtushar/edytlab/issues/388) removed them: nothing called them, and `play_state` pushed a whole mix into a ~250 ms ring buffer that dropped whatever did not fit.

### What the Render Processes

Every track and every clip, with per-clip volume automation, per-track effect chains, sends into buses with their own chains, and the master chain (`crates/audio-engine/src/render.rs`, `effect_chain.rs`). `tempo_map` is read by tools — `select_region`'s beat ranges, for one — not by the renderer.

### Fast Path

A whole-session render that reduces to one track playing one source untouched is written by copying the source file's bytes instead of decoding and mixing. The conditions (one unsoloed track with one clip covering the whole source from frame 0, unity gain and pan, no track effects or sends, no active master-chain effect, no volume envelope or stretch/pitch/beat-grid metadata, source rate equal to session rate, no render range) live in `single_track_unity` (`graph.rs`) and `is_unity_passthrough` (`render.rs`). Everything else goes through the mixer.

---

## 8. ML Pipeline

### ONNX Runtime Setup

```rust
// crates/ml-pipeline/src/lib.rs
// Uses ort 2.0.0-rc.12 with load-dynamic feature
// Execution providers: CoreML (macOS) → CUDA (if available) → CPU
// Model files: loaded from disk, cached by blake3 hash
```

The runtime is loaded dynamically from `ORT_DYLIB_PATH` (or a `libonnxruntime` next to the binary). This avoids linking ONNX into the binary (reduces binary size; allows model updates without recompilation).

`ml_pipeline::runtime::ensure()` does the loading, and every code path that builds an `ort` session (`ModelRegistry::load`, `WhisperModel::load`, `DemucsModel::load`) calls it first. It looks at `ORT_DYLIB_PATH` (a relative path is tried against the executable's directory, then the working directory), then the library file name next to the executable. There is no bare-name or system search, because a system copy may be the wrong version. `ort` 2.0.0-rc.12 cannot report a library it fails to load: building the error re-enters its own initialisation and the thread blocks forever (through `ort::init_from` too). So `ensure()` opens the library itself and makes the checks `ort` would (the `OrtGetApiBase` entry point, version 1.24 or newer, the C API version), and hands it to `ort::init_from` only once they pass. A missing library is `Error::MissingRuntime { searched }` and one that fails the checks is `Error::RuntimeLoad`, where a model load used to hang while the agent held the dispatcher, store and engine locks. The tools turn either into a tool error that still says the feature is not implemented.

`ml_pipeline::fetched_model_path` downloads, verifies and caches a pinned artifact (URL, size and SHA-256 per file): it streams to a `.part` file, resumes with `Range`, refuses a truncated or wrong-hash file and fetches it again once, can be cancelled, and runs its HTTP request on its own thread so it is safe to call from async code. It has no callers and no model is pinned, so nothing is downloaded today. Nothing ships the runtime library or sets `ORT_DYLIB_PATH` yet, so no model can load in a user's install. Shipping the runtime and the download UI is the rest of [#383](https://github.com/laadtushar/edytlab/issues/383).

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

Model variants available (`SUPPORTED_MODEL_IDS`):
- `htdemucs_ft` (default) — fine-tuned, best quality
- `htdemucs` — the OOM fallback

`htdemucs_6s`, with guitar and piano stems, is **not** supported: the
tool rejects any id outside the two above. This section used to list it
(#233).

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

The agent sees memory on every turn but has no tool to write it: none of the 93 registered tools touches memory. You edit it in Settings → Memory (the `read_memory` / `write_memory` commands), which is how BPM, speaker names or style preferences carry across turns and sessions.

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

---

## 10. Agent Profiles and MCP Servers

### Agent Profiles

Override model, tool whitelist, and system-prompt addendum per session. Stored under `~/.edytlab/agents/`.

```yaml
---
description: "Podcast production — fast, focused"
model.provider: anthropic
model.id: claude-haiku-4-5-20251001
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

The MCP layer starts registered servers at app launch, discovers available tools via `tools/list`, and injects them into the agent's tool list alongside built-in tools.

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
| `agent://node-created` | `{ node_id: string }` | DAG node appended after tool |
| `agent://done` | `{}` | Turn complete (no more tool calls) |
| `agent://plan` | `{ steps: object[] }` | A plan, or the held first edit, awaits approval; the turn is suspended (see [Plan first](#plan-first)) |
| `agent://plan-rejected` | none | The user declined the plan or the held edit; the turn ended with no `done` |
| `agent://plan-unavailable` | `{ reason: string, first_edit_held: boolean }` | A plan was asked for and none arrived; `first_edit_held` says whether the first edit will be held for approval or the turn proceeds with no gate |
| `tool-progress` | `ToolProgress` | Progress from a long-running tool (and `select_region`'s match) |
| `marker-changed` | none | Marker/annotation added or removed |

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

---

## 12. Security Model

### API Key Storage

- Keys are stored in the **OS-native keychain** (macOS Keychain, Windows Credential Manager; on Linux the kernel `keyutils` store, which does not persist across reboots — see [Keychain Integration](#keychain-integration))
- edytlab never transmits keys to its own servers
- Keys are read at runtime, signed into HTTP requests in-process, and sent directly to provider endpoints
- Keys are never logged or written to disk outside the OS keychain

### Audio Privacy

- Audio processing runs 100% in-process
- ONNX models (Demucs, Whisper) are designed to run locally; neither runs yet (#383–#385), and audio bytes never leave the machine
- The only network traffic is LLM API calls (text tokens only)

### Tauri Permissions

Tauri's security model requires explicit capability declarations. `apps/desktop/src-tauri/capabilities/default.json` grants only `core:default` and the dialog plugin (`dialog:default`, `dialog:allow-open`) for the file pickers. There is no filesystem plugin: reading source audio and writing renders happen in Rust commands.

The asset protocol (`tauri.conf.json` → `app.security.assetProtocol`) starts scoped to `$APPDATA/**`; the app adds each project's folder as it opens (`allow_assets_in_dir` in `commands.rs`) and each individual file it hands the timeline or a compare render (`allow_asset_file`), rather than everything the user can read.

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

No per-provider keychain code is needed: the slots are keyed by provider id (`<id>_api_key`, `<id>_model`, `<id>_base_url`), and the commands in `commands.rs` check ids against `SUPPORTED_PROVIDER_IDS`. A keyless provider overrides `requires_api_key()`.

### Adding a New Tool

1. Create `crates/tools/src/tool/<name>.rs` implementing the `Tool` trait, and export it from `crates/tools/src/tool/mod.rs`
2. Return the tool descriptor, including the JSON schema for the input, from `schema()`
3. Register it in `ToolDispatcher::default_dispatcher()` in `crates/tools/src/dispatcher.rs`
4. Tests: cover happy path, invalid input, edge cases (empty session, out-of-range times)
5. Regenerate [tools-reference.md](./tools-reference.md) (`UPDATE_TOOLS_REFERENCE=1 cargo test -p tools --test tools_reference_doc`) and add the tool to `website/app/docs/tools/page.tsx` — both are checked by tests

### Adding a Skill

Drop a `.md` file with YAML frontmatter into `~/.edytlab/skills/`. No recompile needed.

### Adding an Agent Profile

Drop a `.md` file with YAML frontmatter into `~/.edytlab/agents/`. No recompile needed.

### Adding an MCP Server

Edit `~/.edytlab/mcp.json` via Settings → MCP, or directly. Tools from the server become available to the agent on next restart.

---

*Last updated: 2026-05-17. Reflects edytlab v0.1.0-dev.*
