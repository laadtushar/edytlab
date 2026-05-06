# Phase 1 — "Edit a Single Track" Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Each Module below is a unit of work; per-module 2–5 minute TDD steps are produced at execution time using `executing-plans` against the module's acceptance criteria.

**Goal:** Ship a Tauri desktop app on Mac and Windows that loads a WAV/MP3, talks to Claude through chat, and runs 8 deterministic editing tools (load, transcribe, cut_range, trim, gain, normalize, render_preview, render_final) end-to-end. Demo: *"remove silence at the start, normalize, export."*

**Architecture:** Cargo workspace; thin Tauri shell calls a Rust core split into 6 crates (audio-io, audio-decoder, session, tools, ai, mcp-stub). Frontend is React + TypeScript + Vite. AI is BYO Anthropic key only — no proxy, no local LLM. Session graph is linear in this phase but uses the DAG data model from spec §6 so Phase 2 only adds operations, not migrations.

**Tech Stack:** Rust 1.83+, Tauri 2.x, cpal 0.15, symphonia 0.5, hound 3.5 (WAV writes), rubato 0.16 (resampling at engine→I/O boundary and Whisper pre-processing), reqwest 0.12, serde 1, tokio 1, anyhow 1, thiserror 2, ort 2.0 (ONNX Runtime, Whisper-base only this phase), React 18, TypeScript 5.6, Vite 6, wavesurfer.js 7 (waveform display).

**Timeline target:** 9 weeks solo (range 8–12). The +3 weeks vs. a Mac-only build come from Module 02 (dual-platform CI + signing) and Module 16 (Windows-specific WASAPI/WebView2 polish).

**Out of scope this phase:** branching graph operations (fork/diff/compare), Demucs/stem separation, time-stretch, pitch-shift, BPM/key analysis, multi-track sessions, effects (EQ/comp/etc.), bus routing, MCP server, hosted proxy, local LLM, Linux build, ASIO support.

---

## Spec coverage map

Each spec section that should land in Phase 1 → module(s):

| Spec § | Requirement | Module(s) |
|---|---|---|
| §3 | Tauri shell, Mac+Windows, BYO Claude key | M01, M02, M14, M15 |
| §5.1 | Session graph crate (data model only) | M05 |
| §5.1 | Audio engine (decode, play, render) | M03, M04, M06 |
| §5.1 | Tool dispatcher | M07 |
| §5.1 | AI layer (BYO Claude) | M10 |
| §5.1 | Tauri shell with canvas + chat | M11, M12, M13 |
| §6 | Session DAG data types (linear ops only) | M05 |
| §7 | 8 tools: load, transcribe, cut_range, trim, gain, normalize, render_preview, render_final | M08, M09 |
| §8 | Single-agent tool-calling loop, voice/podcast mode | M10 |
| §10 | Crash safety: session durable on every node | M05 |
| §11 | Golden WAV diff tests for DSP | M06, M08 |

---

## File / crate structure

Locked at start of phase. Splitting is by responsibility, not technical layer.

```
edytlab/
├── Cargo.toml                          # workspace root
├── package.json                        # frontend root (pnpm workspace)
├── pnpm-workspace.yaml
├── crates/
│   ├── audio-io/                       # M03 — cpal wrapper, output device mgmt
│   │   ├── src/lib.rs
│   │   └── src/{coreaudio,wasapi}.rs   # platform-specific glue
│   ├── audio-decoder/                  # M04 — symphonia wrapper
│   │   └── src/lib.rs
│   ├── audio-engine/                   # M06 — DSP graph, render-to-disk
│   │   └── src/{lib,graph,render}.rs
│   ├── session/                        # M05 — DAG data model + JSON store
│   │   └── src/{lib,node,state,store}.rs
│   ├── tools/                          # M07, M08, M09 — dispatcher + 8 tools
│   │   └── src/{lib,dispatcher,tool/*.rs}
│   ├── ai/                             # M10 — Anthropic client + tool loop
│   │   └── src/{lib,anthropic,loop}.rs
│   └── ml-whisper/                     # M09 — ONNX Whisper-base wrapper
│       └── src/lib.rs
├── apps/
│   └── desktop/                        # M01 — Tauri app
│       ├── src-tauri/                  # Rust side of Tauri (commands, setup)
│       │   ├── Cargo.toml
│       │   └── src/{main,commands,events}.rs
│       ├── src/                        # React/TS frontend
│       │   ├── App.tsx
│       │   ├── components/{Chat,Canvas,Settings}.tsx
│       │   ├── hooks/useSession.ts
│       │   └── lib/tauri-bridge.ts
│       ├── index.html
│       ├── vite.config.ts
│       └── tsconfig.json
├── tests/
│   ├── golden/                         # checked-in reference WAVs (M06, M08)
│   └── e2e/                            # cross-platform smoke test (M16)
├── .github/workflows/
│   ├── ci.yml                          # M02 — build + test on mac+win
│   ├── release-mac.yml                 # M14 — sign + notarize + upload .dmg
│   └── release-win.yml                 # M15 — sign + .msi + WebView2
└── docs/
    ├── specs/2026-05-05-conversational-audio-editor-design.md  # (already there)
    ├── HANDOVER.md                                              # (already there)
    └── superpowers/plans/                                       # this plan
```

Why this layout:
- Rust core is a workspace of small focused crates so each can be built/tested in isolation; the audio engine never depends on Tauri.
- Tauri shell lives under `apps/desktop` to leave room for `apps/mcp-server` in Phase 3 without restructuring.
- Tests directory is a peer of crates (not under each crate) for golden WAVs that several crates share.

---

## Modules

Each module has: **Files**, **Acceptance criteria** (the gate to call it done), **Test design** (what golden inputs and properties), **Risk**, **Estimate**. Steps are produced by `executing-plans` per module.

---

### M01 — Repo scaffolding & Tauri shell hello-world

**Files:**
- Create: `Cargo.toml` (workspace), `apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/src-tauri/src/main.rs`, `apps/desktop/package.json`, `apps/desktop/vite.config.ts`, `apps/desktop/index.html`, `apps/desktop/src/App.tsx`, `apps/desktop/src/main.tsx`, `pnpm-workspace.yaml`, `package.json`, `.gitignore`, `rust-toolchain.toml` (pin to 1.83), `.cargo/config.toml`
- Create: `apps/desktop/src-tauri/tauri.conf.json` with bundle identifier `app.edytlab.desktop`, window 1280×800, dev URL `http://localhost:1420`

**Acceptance criteria:**
1. `pnpm install && pnpm tauri dev` opens a window on Mac with the text "edytlab" rendered by React.
2. `cargo build --workspace` succeeds with one workspace member (`apps/desktop/src-tauri`) — even before other crates exist.
3. `cargo fmt --check && cargo clippy --workspace -- -D warnings` passes.
4. Same `pnpm tauri dev` works on Windows 11.

**Test design:** None at this layer beyond `cargo check`. Treat M01 as a smoke gate.

**Risk:** Low. Tauri 2.x scaffolding is well-trodden. The only platform variance is WebView2 which is auto-installed on Windows 11 22H2+.

**Estimate:** 0.5 weeks.

---

### M02 — Dual-platform CI with signing pipelines

**Files:**
- Create: `.github/workflows/ci.yml` (matrix: macos-14 [Apple Silicon], windows-latest)
- Create: `.github/workflows/release-mac.yml` (Developer ID + notarytool)
- Create: `.github/workflows/release-win.yml` (Authenticode + signtool)
- Create: `scripts/sign-mac.sh`, `scripts/sign-windows.ps1`
- Modify: `apps/desktop/src-tauri/tauri.conf.json` to declare bundle targets (.app + .dmg on Mac; .msi on Windows)

**Acceptance criteria:**
1. CI runs on every push: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test --workspace`, `pnpm install --frozen-lockfile`, `pnpm tauri build` on both runners. End-to-end < 25 min wall clock per platform.
2. Manually-triggered release workflow on Mac produces a notarized `.dmg` whose Gatekeeper status is `accepted source=Notarized Developer ID` (verify via `spctl -a -t exec -vv path/to/app`).
3. Manually-triggered release workflow on Windows produces a signed `.msi` whose `signtool verify /pa` returns `Successfully verified`.
4. Apple Developer ID, app-specific password, and Windows code-signing cert are stored as encrypted GitHub Action secrets — not committed.

**Test design:** A throwaway `--version` flag is added to the Tauri binary so each release artifact can be exec'd in a one-line CI job and exit-coded for sanity.

**Risk:** Medium-high — first Apple notarization typically fails 1–2 times on entitlements; Windows SmartScreen reputation builds over weeks regardless of EV signing. Buffer accordingly. Track Apple notarization service status; outages happen.

**Estimate:** 1.5 weeks. Highest-risk single module of Phase 1.

---

### M03 — Audio I/O abstraction (`audio-io` crate)

**Files:**
- Create: `crates/audio-io/Cargo.toml` (deps: `cpal = "0.15"`, `rubato = "0.16"`, `thiserror`, `tracing`)
- Create: `crates/audio-io/src/lib.rs` — public traits `OutputStream`, `OutputDevice`
- Create: `crates/audio-io/src/coreaudio.rs`, `crates/audio-io/src/wasapi.rs` — `#[cfg(target_os = ...)]` modules
- Create: `crates/audio-io/tests/playback.rs`

**Public API:**
```rust
pub trait OutputStream: Send {
    fn play(&mut self) -> Result<()>;
    fn pause(&mut self) -> Result<()>;
    fn write_samples(&mut self, samples: &[f32]) -> Result<()>; // interleaved stereo
}
pub fn default_output(sample_rate: u32, channels: u16) -> Result<Box<dyn OutputStream>>;
```

**Acceptance criteria:**
1. Plays a 1-second 440 Hz sine through default output on Mac (Core Audio) and Windows (WASAPI shared mode).
2. WASAPI exclusive mode is **not** required this phase — shared mode latency floor (10–20 ms) is acceptable for non-realtime preview playback. Document this in module README.
3. Underrun behavior: if `write_samples` is starved, output silence — never crash the audio thread. Property test verifies.
4. Sample rate mismatch (e.g. play a 44.1 kHz file on a 48 kHz-locked device) is handled by **transparent resampling using `rubato 0.16`** at the engine→I/O boundary. The user-facing API does not surface mismatched-rate errors; resampling is logged at debug level. (`rubato` is shared with M09's Whisper pre-processing; pulling it in here, not later.)

**Test design:**
- Integration test starts a stream, writes a known buffer, polls `frames_played()` counter (provided by the trait — counts per-channel frames actually written to the device, including silence frames written on underrun), asserts within 5% of expected after 1s sleep. (Renamed from `samples_played` during M03 review for audio-terminology accuracy: a "sample" is one scalar; a "frame" is one sample per channel.)
- Manual listening: smoke test on each platform — sine should not click, distort, or pitch-shift.

**Risk:** Medium. WASAPI's shared-mode buffer sizes vary by Windows version; cpal abstracts most of it but format-negotiation surprises happen.

**Estimate:** 1 week.

---

### M04 — Audio decoder (`audio-decoder` crate)

**Files:**
- Create: `crates/audio-decoder/Cargo.toml` (deps: `symphonia = { version = "0.5", features = ["mp3", "wav", "flac"] }`)
- Create: `crates/audio-decoder/src/lib.rs`
- Create: `crates/audio-decoder/tests/decode.rs`
- Create: `tests/golden/sine_440hz_1s.wav`, `tests/golden/sine_440hz_1s.mp3` (generated by build script in `crates/audio-decoder/build.rs` using `hound` + a stub MP3 encoder, or checked in)

**Public API:**
```rust
pub struct DecodedAudio { pub samples: Vec<f32>, pub sample_rate: u32, pub channels: u16 }
pub fn decode_file(path: &Path) -> Result<DecodedAudio>;
```

**Acceptance criteria:**
1. Decodes the golden WAV; `samples.len() == sample_rate * channels * 1`.
2. Decodes the golden MP3; resulting peak frequency (FFT) is 440 Hz ± 1 Hz.
3. Stereo files yield interleaved samples (L, R, L, R, ...).
4. Corrupt input → `Err(DecodeError::Corrupt)` — never panics. Property test feeds random bytes and asserts no panic.

**Test design:** Golden-WAV byte comparison for WAV roundtrip. For MP3, use peak-frequency assertion rather than byte compare (MP3 is lossy).

**Risk:** Low. `symphonia` is mature.

**Estimate:** 0.5 weeks.

---

### M05 — Session crate (DAG data model + JSON store)

**Files:**
- Create: `crates/session/Cargo.toml` (deps: `serde`, `serde_json`, `chrono`, `blake3`, `thiserror`)
- Create: `crates/session/src/lib.rs`, `node.rs`, `state.rs`, `store.rs`, `diff.rs` (stub for Phase 2)
- Create: `crates/session/tests/{roundtrip,linear_ops}.rs`
- Create: `crates/session/proptest-regressions/` (will accumulate)

**Public API:**
```rust
pub struct NodeId(pub [u8; 32]); // blake3 hash of state
pub struct SessionNode { pub id: NodeId, pub parent: Option<NodeId>, pub created_at: DateTime<Utc>, pub label: Option<String>, pub reasoning: Option<String>, pub state: SessionState }
pub struct SessionState { pub tracks: Vec<Track>, pub bus_routing: BusGraph, pub master_chain: Vec<EffectInstance>, pub tempo_map: TempoMap, pub key_map: Option<KeyMap>, pub transcript: Option<Transcript>, pub sample_rate: u32, pub length_samples: u64 }
pub struct Store { /* ... */ }
impl Store {
    pub fn open(project_dir: &Path) -> Result<Self>;
    pub fn append(&mut self, node: SessionNode) -> Result<NodeId>;
    pub fn get(&self, id: NodeId) -> Result<SessionNode>;
    pub fn head(&self) -> NodeId;
    pub fn set_head(&mut self, id: NodeId) -> Result<()>;
}
```

Phase 1 only uses the linear subset: `append` always parents to `head` and updates `head`. Branching ops (`fork`, `diff`, `compare`, `merge`) are stubs with `unimplemented!()` bodies and `#[allow(dead_code)]` — Phase 2 fills them in. **All data fields are present from day one** so on-disk format is forward-compatible.

**Acceptance criteria:**
1. `serde_json::to_string(&node)` followed by `from_str` roundtrips byte-equal — locked by snapshot test.
2. `Store::append` is durable: file `<project>/.audiograph/nodes/<hex_node_id[0..2]>/<hex_node_id>.json` exists (sharded by first 2 hex chars — same scheme git uses for `objects/`; keeps directory entries bounded to ~256 children at the top level even after thousands of edits) and `head` file updated atomically (write-temp + rename).
3. Property test (`proptest`): any sequence of `append` calls leaves the store readable; `head` always points to the most recent append.
4. Crash safety: kill -9 mid-append leaves either (old head + no new node file) or (old head + new node file fully written) — never (new head + missing node file). Verified by killing a child process at random points.

**Test design:**
- Snapshot test: a fixture `SessionNode` serializes to a checked-in JSON file. Changes require explicit snapshot update.
- Property test for store invariants.
- Crash test using `assert_cmd` to spawn-and-kill a child binary.

**Risk:** Medium. The "atomic append + crash safety" property is easy to get wrong on Windows where rename semantics are weaker than POSIX. May need `tempfile::persist` + fsync.

**Estimate:** 1 week.

---

### M06 — Audio engine (`audio-engine` crate)

**Files:**
- Create: `crates/audio-engine/Cargo.toml` (deps: `audio-decoder`, `audio-io`, `session`, `hound = "3.5"`, `dasp = "0.11"`, `rayon`)
- Create: `crates/audio-engine/src/{lib,graph,render,mixer}.rs`
- Create: `crates/audio-engine/tests/render_golden.rs`
- Create: `tests/golden/render_unity_pass_through.wav` (decode → render with no changes; should be sample-identical to source after WAV→WAV)

**Public API:**
```rust
pub fn render_state_to_wav(state: &SessionState, out: &Path, range: Option<TimeRange>) -> Result<RenderReport>;
pub fn play_state(state: &SessionState, output: &mut dyn OutputStream, range: Option<TimeRange>) -> Result<PlayHandle>;
```

**Acceptance criteria:**
1. **Unity pass-through** (the most important DSP correctness test): a `SessionState` with one track containing one clip and no effects renders to a WAV that is sample-identical to the source WAV. Byte-compare against golden. *If this is wrong, every higher-level test is meaningless.*
2. Single track, single gain change of +6.02 dB (= ×2.0): output samples are exactly 2× source samples (within float epsilon).
3. Render is deterministic: same state → byte-identical output across 100 runs and across Mac/Windows.
4. Render of a 10-minute single-track WAV (load + gain + normalize + render) completes in **< 30 seconds (≈ 20× real-time) on a 2020 MacBook Air baseline**. Linear ops on uncompressed audio should be I/O-bound, not CPU-bound; if we can't hit 20× we have a vectorization or memory-copy problem worth fixing now. Multi-track sessions in Phase 2 will be re-baselined.

**Test design:**
- Golden WAV diff for unity pass-through (the non-negotiable).
- Property test: render(decode(file)) == file modulo floating-point epsilon at the int16 quantization boundary.
- Cross-platform determinism test runs same fixture on Mac and Windows in CI and byte-compares.

**Risk:** Medium-high. Determinism across platforms requires care: avoid `f32` accumulation order surprises, avoid `rayon` for the master mix (parallel reduction is non-deterministic). **Pin the mix order** explicitly.

**Estimate:** 1 week.

---

### M07 — Tool dispatcher (`tools` crate skeleton)

**Files:**
- Create: `crates/tools/Cargo.toml` (deps: `session`, `audio-engine`, `serde`, `serde_json`, `schemars = "0.8"`)
- Create: `crates/tools/src/{lib,dispatcher,schema}.rs`
- Create: `crates/tools/src/tool/mod.rs` (will hold individual tool modules in M08, M09)

**Public API:**
```rust
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn schema(&self) -> serde_json::Value; // Anthropic tool-use schema
    fn invoke(&self, args: serde_json::Value, ctx: &mut ToolContext) -> Result<ToolResult>;
}
pub struct ToolDispatcher { tools: HashMap<String, Box<dyn Tool>> }
impl ToolDispatcher {
    pub fn register(&mut self, tool: Box<dyn Tool>);
    pub fn invoke(&self, name: &str, args: serde_json::Value, ctx: &mut ToolContext) -> Result<ToolResult>;
}
pub struct ToolContext<'a> { pub store: &'a mut session::Store, pub engine: &'a mut audio_engine::Engine }
```

**Acceptance criteria:**
1. Registering and invoking a no-op test tool returns `ToolResult::Ok(value)`.
2. Invoking an unregistered tool returns `Err(DispatchError::Unknown)` with the requested name in the message.
3. Schema export: `dispatcher.tool_schemas()` returns a JSON array shaped exactly like the Anthropic API's `tools` parameter — locked by snapshot test against a real Anthropic schema example.
4. Schema validation rejects malformed args before invoking the tool.

**Test design:** Pure unit tests; no audio yet at this layer. Snapshot the schema shape against a hand-written reference.

**Risk:** Low.

**Estimate:** 0.5 weeks.

---

### M08 — 6 deterministic tools (load, cut_range, trim, gain, normalize, render_*)

**Files:**
- Create: `crates/tools/src/tool/{load,cut_range,trim,gain,normalize,render_preview,render_final}.rs`
- Create: `crates/tools/tests/tools_e2e.rs`
- Create: `tests/golden/normalize_-1dbfs.wav` (a non-trivial source: voice clip with peak -6 dBFS → normalized to -1 dBFS)

**Tool specs (all return `ToolResult` with new `NodeId` and a textual summary for the model):**

| Tool | Args | Behavior |
|---|---|---|
| `load` | `{path: string}` | Decode file into a new track on a new session node; return `node_id`, sample_rate, length_samples, channels. |
| `cut_range` | `{track: usize, start_sample: u64, end_sample: u64}` | Remove samples in the range, shift remainder left, append node. |
| `trim` | `{track: usize, start_sample: u64, end_sample: u64}` | Keep only the range, append node. |
| `gain` | `{track: usize, db: f32}` | Apply a constant gain to a track, append node. |
| `normalize` | `{track: usize, target_dbfs: f32}` | Scan peak, scale so peak == target_dbfs. |
| `render_preview` | `{node_id: NodeId, range?: [u64,u64]}` | Render to a temp WAV, return path; do **not** create a new node. |
| `render_final` | `{node_id: NodeId, format: "wav"\|"mp3"\|"flac", out_path: string}` | Render to user-chosen path. |

**Acceptance criteria:**
1. `gain(0, +6.02)` on a unity track followed by `render_final` yields samples 2× the source (within float epsilon).
2. `normalize(0, -1.0)` on the voice fixture matches the golden WAV byte-for-byte.
3. `cut_range` followed by `render_final` produces a file whose duration is exactly `original_duration - (end_sample - start_sample) / sample_rate`.
4. Every tool that mutates state creates exactly one new session node, parented to the prior head.
5. Argument validation: out-of-range samples, unknown track index → typed errors surfaced to the model with actionable text (`"track index 3 out of range; session has 1 track"`).

**Test design:**
- Per-tool integration test using a tiny in-memory store.
- Cross-tool sequence test: `load → cut_range → normalize → render_final` matches a golden output for a known fixture.
- Property test on `gain`: compose two gains; result == single gain whose dB sum is exact.

**Risk:** Low individually; medium aggregate (6 tools × correctness × determinism × cross-platform).

**Estimate:** 1.5 weeks.

---

### M09 — Whisper transcription (`ml-whisper` crate + `transcribe` tool)

**Files:**
- Create: `crates/ml-whisper/Cargo.toml` (deps: `ort = "2.0"`, `ndarray`, `audio-decoder`)
- Create: `crates/ml-whisper/src/lib.rs`
- Create: `crates/tools/src/tool/transcribe.rs`
- Create: `crates/ml-whisper/tests/transcribe_smoke.rs`
- Create: `assets/models/whisper-base.en.onnx` — **not committed**; downloaded by `scripts/fetch-models.sh` and `cargo test` skips with a warning if missing.
- Create: `tests/golden/known_speech_clip.wav` (1–2 sec of clearly-spoken English, e.g. "the quick brown fox jumps over the lazy dog"), and `tests/golden/known_speech_transcript.txt`.

**Public API:**
```rust
pub struct WhisperModel { /* ort::Session */ }
impl WhisperModel { pub fn load(model_path: &Path) -> Result<Self>; pub fn transcribe(&self, audio_16khz_mono: &[f32]) -> Result<Vec<Word>>; }
pub struct Word { pub text: String, pub start_s: f32, pub end_s: f32, pub confidence: f32 }
```

**Tool spec:**
- `transcribe`: `{path: string}` → `{words: [Word]}`. Internally resamples to 16 kHz mono using `rubato` (workspace-level dep already added in M03).

**Acceptance criteria:**
1. On the known fixture, output transcript matches the reference within Levenshtein distance ≤ 2 (allowing for known Whisper quirks).
2. Word timestamps are monotonic non-decreasing; `start_s < end_s` for every word.
3. Model is loaded once and reused across calls (do not re-load per-tool-invocation — verified by a benchmark assert: 10 calls < 3× the time of 1 call).
4. Missing-model fallback: tool returns a structured error to the model that includes the install command, instead of panicking.

**Test design:**
- Smoke test gated on `WHISPER_MODEL` env var being set. CI sets it.
- Output is intentionally fuzzy (Levenshtein) — Whisper is not byte-deterministic across ONNX Runtime versions.

**Risk:** Medium. ONNX Runtime + CoreML EP on Apple Silicon has historically had quirks (operators falling back to CPU). Test on M1, M2, and Intel Mac if any developer has access. On Windows, default to CPU EP this phase — adding CUDA is out of scope.

**Estimate:** 1 week.

---

### M10 — AI layer (`ai` crate; Anthropic client + tool loop)

**Files:**
- Create: `crates/ai/Cargo.toml` (deps: `reqwest = { version = "0.12", features = ["json", "rustls-tls"] }`, `tokio`, `serde`, `eventsource-stream` for SSE streaming, `tools`)
- Create: `crates/ai/src/{lib,anthropic,prompt,loop}.rs`
- Create: `crates/ai/tests/loop_integration.rs`
- Create: `crates/ai/prompts/system.md` (versioned, snapshot-tested)
- Create: `crates/ai/prompts/voice_mode.md`

**Public API:**
```rust
pub struct AnthropicConfig { pub api_key: String, pub model: String /* "claude-sonnet-4-6" */ }
pub struct Agent { /* holds dispatcher, config, conversation history */ }
impl Agent {
    pub fn new(cfg: AnthropicConfig, dispatcher: Arc<Mutex<ToolDispatcher>>, store: Arc<Mutex<Store>>) -> Self;
    pub async fn turn(&mut self, user_message: String, on_event: impl FnMut(AgentEvent)) -> Result<TurnResult>;
}
pub enum AgentEvent { TextDelta(String), ToolCallStart{name:String, id:String}, ToolCallEnd{id:String, ok:bool}, NodeCreated(NodeId), Done }
```

**Tool-call loop:**
1. Build request: system prompt (cached) + tool schemas (cached) + conversation history.
2. Stream Anthropic `messages` endpoint with `tool_choice: auto`.
3. On `tool_use` block, invoke dispatcher synchronously, append `tool_result`, continue.
4. Hard cap: 10 tool calls per turn; on excess, return error to user.

**Acceptance criteria:**
1. With a recorded fixture (mocked HTTP), turn `"normalize this to -1 dBFS"` against a session containing one loaded WAV correctly invokes `normalize` once and emits `NodeCreated` once.
2. System prompt is request-cached using Anthropic's `cache_control: ephemeral` block — verified by inspecting outgoing request body in the mock.
3. Streaming text deltas are emitted in order to `on_event`.
4. Tool-call schema validation is enforced before invoke; malformed args from the model surface a `tool_result` with `is_error: true` and the model gets one retry before the loop bails.
5. API key is loaded from OS keychain (Tauri `keyring` crate) — never written to disk in plaintext, never logged.

**Test design:**
- HTTP mocked via `wiremock` with recorded Anthropic responses.
- Snapshot test on the system prompt and the request payload shape.
- Live integration test gated on `ANTHROPIC_API_KEY` env var: round-trips one turn against the real API. Runs on local dev only, not CI.

**Risk:** Medium. Streaming + tool-use + caching on Anthropic has subtle edge cases (cache invalidation, partial tool-use blocks).

**Estimate:** 1.5 weeks.

---

### M11 — Tauri commands wiring core to frontend

**Files:**
- Create: `apps/desktop/src-tauri/src/{commands,events,state}.rs`
- Modify: `apps/desktop/src-tauri/src/main.rs` to register commands
- Create: `apps/desktop/src/lib/tauri-bridge.ts`

**Commands (Rust → frontend):**
```rust
#[tauri::command] async fn open_project(path: String) -> Result<ProjectInfo, String>;
#[tauri::command] async fn send_message(text: String) -> Result<(), String>; // streams via events
#[tauri::command] async fn set_api_key(key: String) -> Result<(), String>;
#[tauri::command] async fn get_session_head() -> Result<NodeId, String>;
#[tauri::command] async fn get_node(id: NodeId) -> Result<SessionNode, String>;
#[tauri::command] async fn render_preview(node: NodeId) -> Result<String, String>; // returns path
```

**Events (Rust → frontend):**
- `agent://text-delta` `{text: string}`
- `agent://tool-call` `{name: string, id: string}`
- `agent://node-created` `{node_id: string}`
- `agent://done`

**Acceptance criteria:**
1. Frontend can call `open_project`, get a `ProjectInfo`, and TypeScript types match Rust types (via `ts-rs` or hand-aligned with snapshot test).
2. `send_message` triggers a stream of events that arrive in order in the frontend.
3. `set_api_key` writes to keychain; on relaunch, the key persists.

**Test design:**
- Tauri integration test using `tauri::test::mock_app`.
- Frontend test using Vitest stubs the Tauri bridge.

**Risk:** Low-medium. Type-sync between Rust and TS is a recurring footgun; resolve by generating from `schemars`.

**Estimate:** 0.5 weeks.

---

### M12 — Frontend chat panel + waveform canvas

**Files:**
- Create: `apps/desktop/src/components/{Chat,Canvas,MessageBubble,ToolBadge}.tsx`
- Create: `apps/desktop/src/hooks/{useSession,useAgentStream}.ts`
- Create: `apps/desktop/src/styles.css` (Tailwind v4 with `@import "tailwindcss"`)
- Modify: `apps/desktop/src/App.tsx` to lay out Chat (right 30%) + Canvas (left 70%)

**UI behaviors:**
- Chat: streams text deltas char-by-char; tool calls appear as compact badges ("running normalize…") that resolve to "✓ normalized -1 dBFS" or "✗ error"; final new-node summary shown as a divider row.
- Canvas: wavesurfer.js renders the loaded track; playhead scrubs in time; play/pause keyboard shortcut (space).
- Empty state: "Drop an audio file or use the file menu" → file drop loads via the `load` tool.
- Render-preview button next to chat: plays the latest node's render through the audio engine.

**Acceptance criteria:**
1. Drag-drop a WAV → file appears as track in canvas within 2 sec; chat shows agent's "loaded foo.wav, 3:42, 44.1 kHz stereo" message.
2. Type "normalize to -1 dBFS" → chat streams response; tool badge appears and resolves; new-node divider rendered; canvas re-rendered preview; playback works.
3. Render error → user-friendly message ("Could not normalize: source is silent") in chat, no crash.

**Test design:**
- Vitest + React Testing Library for component logic.
- Playwright for one happy-path E2E driving Tauri (covered in M16).

**Risk:** Medium — UI is the most likely area for "looks right but feels wrong." Manual UX iteration is needed; budget extra polish time at end of phase.

**Estimate:** 1 week.

---

### M13 — Settings & API key UI

**Files:**
- Create: `apps/desktop/src/components/Settings.tsx`
- Modify: `apps/desktop/src/App.tsx` to add a settings menu item.

**Behaviors:**
- First-launch: blocking modal asking for Anthropic API key, with a "How to get a key" link.
- Settings panel: change model (claude-sonnet-4-6 default; claude-haiku-4-5 for cheap mode), change key, clear key.
- Key validity: a "Test" button issues a 1-token Anthropic request and shows green/red.

**Acceptance criteria:**
1. App started with no key → modal blocks chat until key is set.
2. Test button against a bad key shows `"401 invalid x-api-key"` not an unhandled error.
3. Cleared key returns app to first-launch state without restart.

**Test design:** Unit tests for the Settings component; manual test for the first-launch flow on each platform.

**Risk:** Low.

**Estimate:** 0.5 weeks.

---

### M14 — Mac packaging (Developer ID, notarization, .dmg)

**Files:**
- Modify: `apps/desktop/src-tauri/tauri.conf.json` — set bundle identifier, signing identity, entitlements
- Create: `apps/desktop/src-tauri/entitlements.plist` — `com.apple.security.app-sandbox` not set this phase (we read arbitrary files); only enable: `com.apple.security.network.client` (Anthropic), `com.apple.security.cs.allow-unsigned-executable-memory` (ONNX Runtime JIT).
- Modify: `.github/workflows/release-mac.yml` — call `xcrun notarytool submit` with secrets `APPLE_ID`, `APPLE_APP_SPECIFIC_PASSWORD`, `APPLE_TEAM_ID`.

**Acceptance criteria:**
1. `tauri build --bundles dmg` produces a notarized `.dmg`.
2. `spctl -a -t exec -vv` reports `accepted source=Notarized Developer ID`.
3. Stapled (`stapler validate`) so offline launch works.
4. Audio I/O and microphone permission prompts (if needed) appear on first use, not on launch.

**Risk:** Medium. ONNX Runtime's JIT entitlement requirement is a known gotcha; first notarization typically rejects without it.

**Estimate:** 0.5 weeks (assuming Apple Developer Program enrollment is already complete; if not, allow extra wall-clock time for Apple's verification).

---

### M15 — Windows packaging (Authenticode, .msi, WebView2)

**Files:**
- Modify: `apps/desktop/src-tauri/tauri.conf.json` — set Windows section, WebView2 install mode `embedBootstrapper`.
- Modify: `.github/workflows/release-win.yml` — `signtool sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 /f cert.pfx /p $env:CERT_PASS bundle.msi`.
- Add: `assets/MicrosoftEdgeWebView2RuntimeInstallerX64.exe` (~1.7 MB bootstrapper) to bundle, OR use `downloadBootstrapper` mode (smaller installer, requires net).

**Decision:** Default to `embedBootstrapper` so install works offline. Accepts +1.7 MB on the .msi.

**Acceptance criteria:**
1. `.msi` installs cleanly on a fresh Windows 11 22H2 VM.
2. `signtool verify /pa /v` returns success.
3. App launches without WebView2 already installed (bootstrapper handles it).
4. SmartScreen warning is the only friction on first-install — documented in user guide that this clears as downloads accumulate.

**Risk:** Medium. SmartScreen reputation is non-deterministic and outside our control. Plan to ship beta to private testers for ~2 weeks before public to seed reputation.

**Estimate:** 0.5 weeks.

---

### M16 — End-to-end smoke test (the demo)

**Files:**
- Create: `tests/e2e/podcast_cleanup.rs` — Rust binary using `assert_cmd` to drive a headless build of the app (or, if Tauri-headless isn't viable, a CLI binary `apps/cli/` that wires the same core stack).
- Decision: ship a small `cli` binary alongside the desktop app. It exercises the same core for E2E without browser.
- Create: `apps/cli/Cargo.toml`, `apps/cli/src/main.rs` — accepts `--message` and `--input-file`, prints JSON events. Pure infrastructure for testing; not user-facing.

**Demo script (manual, pre-release):**
1. Open app on Mac; drop `tests/fixtures/raw_podcast_intro.wav` (15s, 2s of room tone before speech starts).
2. Type: "Remove the silence at the start, then normalize to -1 dBFS."
3. Wait ≤ 30s. Expect: chat narrates; two tool badges (`cut_range`, `normalize`); preview button plays the result; speech starts at t=0; peak level is -1 dBFS ± 0.1.
4. Click "Export…", choose `.mp3`, save.
5. Decode the exported file with `ffprobe`; assert duration < original by ≥ 1.8 sec; assert peak ≈ -1 dBFS.
6. Repeat steps 1–5 on Windows 11.

**Acceptance criteria:**
1. Pass on both Mac and Windows. Phase 1 ships when this is green.
2. Recorded screen capture (~2 min, no edits) is saved to `assets/demos/phase1-podcast-cleanup.mp4` for the README.

**Risk:** This is the integration risk surfacing — will reveal anything missed in M01–M15. Budget 1 week for surprises.

**Estimate:** 1 week.

---

## Phase 1 schedule (9 weeks)

| Wk | Modules in flight |
|---|---|
| 1 | M01 (scaffold), M02 start (CI base) |
| 2 | M02 finish (signing on both), M03 start |
| 3 | M03, M04 |
| 4 | M05 |
| 5 | M06, M07 |
| 6 | M08 |
| 7 | M09, M10 start |
| 8 | M10 finish, M11, M12 start |
| 9 | M12 finish, M13, M14, M15, M16 |

The schedule is sequenced so the highest-risk modules (M02 signing, M06 DSP determinism, M10 streaming + caching) finish before M16 integration. M14/M15 packaging are quick if M02 was done well.

## Risks & mitigations (Phase 1)

| Risk | Likelihood | Mitigation |
|---|---|---|
| Apple notarization rejection on entitlements | High first attempt | M02 budgets 2 attempts; entitlement file specified in M14 lists known requirements (JIT for ONNX). |
| WASAPI shared-mode underruns under load | Medium | Property test in M03 with starvation injection; if encountered, fall back to a larger ring buffer. |
| ONNX Runtime CoreML EP operator fallbacks slow Whisper | Medium | M09 benchmarks; if too slow, fall back to CPU EP — still usable for podcast lengths. |
| Cross-platform render determinism gap | Medium-high | M06 explicitly pins mix order, no rayon for the mix; CI cross-platform compare gate. |
| Anthropic streaming + tool-use edge cases | Medium | M10 has full HTTP fixtures and snapshot tests on request payload. |
| Solo dev underestimates packaging | High | 1 week budget already in M02; another 1 week between M14/M15/M16 buffer. |

## Open questions deferred to execution

- **API key UX**: keychain only, or also a "session-only" mode that holds the key in memory for paranoid users? Decide during M13.
- **Model default**: Sonnet 4.6 vs. Haiku 4.5 default. Tentative: Sonnet 4.6 default with a "Cheap mode" toggle to Haiku 4.5 in Settings. Confirm during M13.
- **Render format defaults**: WAV (lossless) default, MP3 (320kbps) and FLAC as alternates. No Ogg in Phase 1.
- **Project file format**: a `.edytlab` directory (the `.audiograph/` from spec) opened by the app? Or a single zipped file? Tentative: directory in Phase 1 (matches spec); zip-export in Phase 2.

## Self-review checklist run

- [x] Spec coverage map (above) maps every Phase-1-relevant spec section to a module.
- [x] No "TBD/TODO/implement later" placeholders in module bodies.
- [x] Type names consistent across modules: `NodeId`, `SessionState`, `Tool`, `ToolDispatcher` referenced identically in M05, M07, M08, M10, M11.
- [x] Every module names exact file paths and exact dependency versions.
- [x] DSP correctness gate (unity pass-through, M06) lands before any tool that mutates audio (M08).
- [x] Whisper model is treated as a runtime-fetched asset (M09) — not committed.

## Execution handoff

Plan saved to `docs/superpowers/plans/2026-05-05-phase-1-edit-single-track.md`. Two execution options:

1. **Subagent-Driven (recommended)** — fresh subagent per module, code review between modules, fast iteration. Best for modules M03–M10 where the surface area is well-bounded.
2. **Inline Execution** — execute modules in this session using `executing-plans`, batch with checkpoints. Best for M01, M02, M14–M16 where decisions emerge from running CI.

A hybrid is reasonable: M01 and M02 inline (lots of reactive tweaking); M03–M13 subagent-driven (well-scoped); M14–M16 inline.
