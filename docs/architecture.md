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
                                  │  tauri::command (IPC) + Tauri Events (SSE)
┌─────────────────────────────────▼───────────────────────────────────────────┐
│  APPLICATION LAYER  (apps/desktop/src-tauri — Rust)                        │
│                                                                             │
│  commands.rs (~50 commands)  ·  lib.rs (AppState, event plumbing)          │
└───────────┬──────────┬────────────────────┬──────────────┬──────────────────┘
            │          │                    │              │
     ┌──────▼──┐  ┌────▼────┐  ┌──────────▼───┐  ┌──────▼──────┐
     │  crates/│  │ crates/ │  │    crates/   │  │   crates/   │
     │  ai     │  │ tools   │  │    session   │  │   audio-*   │
     │  agent  │  │ ~28 ops │  │    DAG store │  │   ml-*      │
     └──────┬──┘  └────┬────┘  └──────────┬───┘  └──────┬──────┘
            │          │                  │              │
     ┌──────▼──────────▼──────────────────▼──────────────▼──────┐
     │  SHARED STATE  (Arc<Mutex<_>> in AppState)                │
     │  Store · Engine · Agent · Clipboard · PlanNotify          │
     └───────────────────────────────────────────────────────────┘
                               │
     ┌─────────────────────────▼──────────────────────────┐
     │  EXTERNAL SERVICES (network only for LLM tokens)   │
     │  Anthropic API · OpenRouter API · OpenAI API        │
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
│   └── cli/                      # Headless batch CLI (smoke tests + scripting)
└── crates/
    ├── ai/                       # LLM abstraction, agent loop, keychain
    ├── agent_profiles/           # Per-session model + tool-whitelist profiles
    ├── audio-analysis/           # BPM, key, beat-grid, transient detection
    ├── audio-decoder/            # symphonia-based file decode (MP3 WAV FLAC)
    ├── audio-engine/             # DSP graph render + cpal playback
    ├── audio-io/                 # cpal device enumeration + capture
    ├── audio-time/               # Pitch-shift / time-stretch primitives (Phase 2)
    ├── mcp/                      # MCP server lifecycle + JSON-RPC dispatch
    ├── memory/                   # Global/project markdown memory fragments
    ├── ml-demucs/                # Stem separation via ONNX Demucs
    ├── ml-pipeline/              # Shared ONNX runtime + model cache
    ├── ml-whisper/               # Transcription via ONNX Whisper large-v3
    ├── session/                  # DAG data model, node store, fork/diff/compare
    ├── skills/                   # User skill library with trigger evaluation
    └── tools/                    # ~93 deterministic audio-editing tools
```

### Dependency Graph (simplified)

```
apps/desktop/src-tauri
    ├── crates/ai
    │   ├── crates/session
    │   └── crates/tools
    │       ├── crates/audio-engine
    │       │   ├── crates/audio-decoder
    │       │   └── crates/audio-io
    │       ├── crates/ml-demucs
    │       │   └── crates/ml-pipeline
    │       └── crates/ml-whisper
    │           └── crates/ml-pipeline
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
| Build tool | Vite 7 + Turbopack |
| Styling | Tailwind CSS 4 |
| Waveform | WaveSurfer.js 7 |
| Graph view | @xyflow/react 12 |
| Animations | Framer Motion 11 |

### Component Tree

```
App.tsx
├── AppHeader
│   ├── Logo + project path
│   ├── PlaybackControls (transport)
│   └── SettingsGear → Settings modal
├── MainContent (split pane)
│   ├── LeftPane
│   │   ├── Timeline (wavesurfer, tracks, markers, playhead)
│   │   │   ├── Ruler
│   │   │   └── MarkerLayer
│   │   └── GraphView (xyflow DAG visualization)
│   │       └── Canvas (node thumbnails)
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

### State Management

All session state lives in Rust (`AppState`). The frontend is intentionally thin — it:
1. Sends commands via `tauri-bridge.ts`
2. Receives SSE events (text delta, tool call, node created, done)
3. Derives local UI state from the command responses

Local React state (not persisted): `head`, `audioPath`, `rendering`, `leftView`, `compareMode`, `markers`, `tracks`, `showShortcuts`, `selection`.

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
}

pub enum AgentEvent {
    TextDelta(String),
    ToolCallStart { name: String, id: String },
    ToolCallEnd   { id: String, ok: bool },
    NodeCreated(NodeId),
    Done,
    Plan { steps: Vec<serde_json::Value> },
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
    │  Tool call loop (max 10/turn)  │
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
// crates/ai/src/keychain.rs
pub fn set_key(service: &str, key: &str) -> Result<()>
pub fn get_key(service: &str) -> Result<Option<String>>
pub fn delete_key(service: &str) -> Result<()>
```

Keychain slots:
- `anthropic_api_key`
- `openrouter_api_key`
- `openai_api_key`
- `active_provider` (stores provider id string)
- `active_model_<provider>` (per-provider model override)

Legacy `anthropic_api_key` (without provider prefix) is read on first run and migrated.

### Model Catalogue

`crates/ai/src/models.rs` fetches available models from each provider's `/v1/models` endpoint with a 10-minute TTL cache. The combo picker in Settings surfaces these alongside free-form input so new model ids work immediately.

### Constants

```rust
pub const DEFAULT_MODEL: &str = "claude-sonnet-4-6";
pub const CLASSIFIER_MODEL: &str = "claude-haiku-4-5-20251001";
pub const MAX_TOOL_CALLS_PER_TURN: usize = 10;
```

---

## 5. Tool Dispatch System

### Tool Trait

```rust
// crates/tools/src/lib.rs

pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn input_schema(&self) -> Value;  // Anthropic-shaped JSON Schema
    fn call(
        &self,
        input: Value,
        ctx: &mut ToolContext,
    ) -> Result<ToolResult, ToolError>;
}

pub struct ToolContext<'a> {
    pub store:     &'a mut Store,
    pub engine:    &'a mut Engine,
    pub clipboard: &'a mut Option<Vec<f32>>,
}

pub struct ToolDispatcher {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolDispatcher {
    pub fn new() -> Self  // Registers all 93 built-in tools
    pub fn dispatch(&mut self, name: &str, input: Value, ctx: &mut ToolContext) -> ToolResult
    pub fn tool_definitions(&self) -> Vec<Value>  // Sent to LLM on every turn
    pub fn filter(&self, whitelist: &[String]) -> Self  // For agent profiles
}
```

### All 28 Tools

| Tool | File | What it does |
|------|------|-------------|
| `load` | `load.rs` | Decode audio file + create session node |
| `cut_range` | `cut_range.rs` | Remove a time range from a track |
| `copy_region` | `copy_region.rs` | Copy region to clipboard |
| `paste_region` | `paste_region.rs` | Paste clipboard at position |
| `fade` | `fade.rs` | Apply fade-in / fade-out envelope |
| `gain` | `gain.rs` | Apply static dB gain to region |
| `set_track_gain` | `set_track_gain.rs` | Set per-track gain level |
| `normalize` | `normalize.rs` | Normalize to LUFS or peak target |
| `reverse` | `reverse.rs` | Reverse audio region |
| `trim` | `trim.rs` | Trim silence from start/end |
| `insert_silence` | `insert_silence.rs` | Insert silence at position |
| `time_stretch` | `time_stretch.rs` | Adjust duration without pitch change |
| `pitch_shift` | `pitch_shift.rs` | Shift pitch without duration change |
| `add_track` | `add_track.rs` | Add a new empty track to the session |
| `remove_track` | `remove_track.rs` | Remove a track by id |
| `separate_stems` | `separate_stems.rs` | Run Demucs; output 4 stem tracks |
| `transcribe` | `transcribe.rs` | Run Whisper; store word-level transcript |
| `analyze_track` | `analyze_track.rs` | BPM, key, loudness analysis |
| `align_to_beat` | `align_to_beat.rs` | Align track start to nearest beat |
| `apply_diff` | `apply_diff.rs` | Apply a computed session diff |
| `compare_nodes` | `compare_nodes.rs` | Generate diff between two DAG nodes |
| `fork_node` | `fork_node.rs` | Fork current DAG node → new branch |
| `revert_to` | `revert_to.rs` | Jump to earlier node in the DAG |
| `name_node` | `name_node.rs` | Set a human label on a node |
| `label` | `label.rs` | Add annotation/marker to the timeline |
| `render_final` | `render_final.rs` | Offline render full session to WAV |
| `render_preview` | `render_preview.rs` | Render preview (temp file) for playback |
| `util` | `util.rs` | Shared helpers (range validation, etc.) |

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
}

// crates/session/src/state.rs
pub struct SessionState {
    pub tracks:        Vec<Track>,
    pub bus_routing:   BusGraph,
    pub master_chain:  Vec<EffectInstance>,  // Forward-compat, Phase 2
    pub tempo_map:     TempoMap,
    pub key_map:       Option<KeyMap>,
    pub transcript:    Option<Transcript>,
    pub sample_rate:   u32,
    pub length_samples: u64,
}

pub struct Track {
    pub id:       TrackId,
    pub name:     String,
    pub clips:    Vec<Clip>,
    pub gain_db:  f32,
    pub muted:    bool,
}

pub struct Clip {
    pub source_path:  String,   // Absolute path — never modified
    pub start_sec:    f64,
    pub duration_sec: f64,
}
```

### Store Operations

```rust
// crates/session/src/store.rs
impl Store {
    pub fn open(path: &Path) -> Result<Store>        // Creates if absent
    pub fn head(&self) -> Option<NodeId>             // Current node pointer
    pub fn set_head(&mut self, id: NodeId) -> Result<()>
    pub fn get(&self, id: NodeId) -> Result<SessionNode>
    pub fn append(                                   // Append new node → child of head
        &mut self,
        parent: Option<NodeId>,
        label: Option<String>,
        state: SessionState,
    ) -> Result<NodeId>
    pub fn list_nodes(&self) -> Result<Vec<SessionNode>>
    pub fn annotations_for(&self, node: NodeId) -> Result<Vec<Annotation>>
    pub fn diff(a: &SessionState, b: &SessionState) -> SessionDiff
    pub fn fork(base: &SessionState, branch: &SessionState) -> Result<SessionState>
    pub fn merge(a: &SessionState, b: &SessionState) -> Result<SessionState>
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

### Storage Format

Nodes are stored as content-addressed JSON files under `<project-dir>/.edytlab/nodes/`:
```
.edytlab/
  nodes/
    <blake3-hash-1>.json
    <blake3-hash-2>.json
    ...
  head           # plain text: current NodeId
  annotations/   # per-node annotation files
```

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

pub fn play_state<'a>(
    state: &SessionState,
    output: &'a mut dyn OutputStream,
    range: Option<TimeRange>,
) -> Result<PlayHandle<'a>>
```

### Phase 1 Scope

Phase 1 implements single-track, single-clip playback and render with optional gain. The following fields exist in `SessionState` for forward compatibility but are **not processed** in Phase 1:
- `bus_routing`
- `master_chain`
- `tempo_map`

Multi-track render: each track decoded and mixed in the render pipeline.

### Fast Path

Single-track sessions with a single clip skip the intermediate temp-file step and stream decoded audio directly to the output WAV. Multi-track sessions render through the full mixer.

---

## 8. ML Pipeline

### ONNX Runtime Setup

```rust
// crates/ml-pipeline/src/lib.rs
// Uses ort 2.0.0-rc.12 with load-dynamic feature
// Execution providers: CoreML (macOS) → CUDA (if available) → CPU
// Model files: loaded from disk, cached by blake3 hash
```

Runtime is loaded dynamically from `ORT_DYLIB_PATH` at startup. This avoids linking ONNX into the binary (reduces binary size; allows model updates without recompilation).

### Whisper (Transcription)

```
Input: WAV (any sample rate)
    │
    ▼  resample to 16 kHz mono (rubato)
    │
    ▼  log-mel spectrogram (80 bins, 30-second window)
    │
    ▼  Whisper encoder + decoder (ONNX large-v3)
    │
    ▼  word-level timestamps via DTW alignment
    │
Output: Vec<WordTimestamp> stored in SessionState.transcript
```

Runs entirely on-device. A 60-minute file transcribes in ~4–8 minutes on a modern laptop (CPU-only). Apple Neural Engine (CoreML) and CUDA acceleration reduce this significantly.

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

Model variants available:
- `htdemucs` (default) — best quality/speed ratio
- `htdemucs_6s` — 6 stems (adds guitar + piano), ~2× slower

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

The agent can read/write memory directly using tools — letting it persist BPM, speaker names, style preferences, or any session context across turns.

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

Active profile is selected from Settings and persisted in the keychain.

### MCP Servers (Phase 5)

edytlab supports the Model Context Protocol for extending the agent with external tools. Configured in `~/.edytlab/mcp.json`.

```json
{
  "servers": {
    "my-server": {
      "transport": "stdio",
      "command": "/usr/local/bin/my-mcp-server",
      "args": ["--config", "/path/to/config.json"],
      "env": { "MY_SECRET": "${MY_SECRET_FROM_KEYCHAIN}" },
      "enabled": true
    }
  }
}
```

Transport types: `stdio` (JSON-RPC over stdin/stdout), `sse` (HTTP Server-Sent Events).

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

### Events (Streaming SSE)

The agent turn emits events via Tauri's event system. The frontend subscribes with `listen()`.

| Event name | Payload | When emitted |
|-----------|---------|-------------|
| `agent:text-delta` | `{ text: string }` | Each SSE text chunk from the LLM |
| `agent:tool-call` | `{ name: string, id: string }` | Tool execution starts |
| `agent:tool-call-end` | `{ id: string, ok: boolean }` | Tool execution completes |
| `agent:node-created` | `{ node_id: string }` | DAG node appended after tool |
| `agent:done` | `{}` | Turn complete (no more tool calls) |
| `agent:plan` | `{ steps: object[] }` | Multi-step plan emitted (mashup mode) |
| `marker:changed` | `{}` | Marker/annotation added or removed |

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

- Keys are stored in the **OS-native keychain** (macOS Keychain, Windows Credential Manager)
- edytlab never transmits keys to its own servers
- Keys are read at runtime, signed into HTTP requests in-process, and sent directly to provider endpoints
- Keys are never logged or written to disk outside the OS keychain

### Audio Privacy

- Audio processing runs 100% in-process
- ONNX models (Demucs, Whisper) run locally; audio bytes never leave the machine
- The only network traffic is LLM API calls (text tokens only)

### Tauri Permissions

Tauri's security model requires explicit capability declarations. `apps/desktop/src-tauri/tauri.conf.json` declares only the capabilities the app needs:
- File system: read (source audio), write (renders to user-specified paths)
- Dialog: open/save file pickers
- Protocol-asset: loads bundled web assets

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
    ▼ invoke("send_message", { text }) → Rust commands.rs
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
    │    TextDelta → emit agent:text-delta to WebView │
    │    ToolUseStart → emit agent:tool-call          │
    │                → dispatch to ToolDispatcher     │
    │                → tool mutates Store/Engine      │
    │                → Store appends new NodeId       │
    │                → emit agent:node-created        │
    │                → emit agent:tool-call-end       │
    │    If more tool_calls → loop back               │
    └─────────────────────────────────────────────────┘
    │
    ▼ emit agent:done → React updates UI
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
2. Add it to `SUPPORTED_PROVIDER_IDS` and the `from_id()` factory
3. Handle request serialization (`serialize_request`) and stream parsing (`parse_stream_chunk`)
4. Add a keychain slot in `commands.rs` (`set_api_key_for` / `has_api_key_for`)
5. Update the `ProviderId` TypeScript union in `tauri-bridge.ts`

### Adding a New Tool

1. Create `crates/tools/src/tool/<name>.rs` implementing `Tool` trait
2. Add the JSON schema for the input (`input_schema()`)
3. Register in `ToolDispatcher::new()` in `crates/tools/src/lib.rs`
4. Tests: cover happy path, invalid input, edge cases (empty session, out-of-range times)

### Adding a Skill

Drop a `.md` file with YAML frontmatter into `~/.edytlab/skills/`. No recompile needed.

### Adding an Agent Profile

Drop a `.md` file with YAML frontmatter into `~/.edytlab/agents/`. No recompile needed.

### Adding an MCP Server

Edit `~/.edytlab/mcp.json` via Settings → MCP Servers, or directly. Tools from the server become available to the agent on next restart.

---

*Last updated: 2026-05-17. Reflects edytlab v0.1.0-dev.*
