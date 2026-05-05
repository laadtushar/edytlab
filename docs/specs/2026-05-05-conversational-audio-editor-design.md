# Conversational Audio Editor — Design

**Status:** Draft
**Date:** 2026-05-05
**Author:** Tushar (with Claude)
**Scope:** New product, separate repo (TBD name)
**Related:** Brainstorm conversation 2026-05-05. This product is independent of Treacle and will live in a new repository.

---

## 1. Problem

Audio editing today forces a binary choice. Pro tools (Logic, Pro Tools, Ableton, RipX) give producers full control but require expert knowledge and 100+ hours to learn. AI tools (Descript Underlord, Adobe Podcast, Moises) are easy but shallow — they shuffle transcripts, isolate stems, or apply preset chains; they don't reason about the music.

Nobody offers **conversational, multi-track, cross-song production** at professional DSP quality. A producer who wants to say "take A's vocal, put it over B's drumloop in F minor, sidechain the synths, give me 3 versions of the drop" must do it by hand in a DAW, even though every individual operation is automatable.

The gap is not technology — Demucs, Whisper, Rubber Band, librosa all exist and are excellent. The gap is the *agent layer* that plans, executes, evaluates, and iterates over those primitives in a session the user can actually trust and steer.

## 2. Goals / Non-goals

**Goals**

- Two lighthouse demos shippable in v1: (a) "mashup any two songs" and (b) "conversational mix engineer" (drop stems, refine through chat with audible A/B at every turn).
- DSP quality is non-negotiable. Output is indistinguishable in a blind test from a competent producer doing the same operation by hand.
- The session graph (every state is a node, every edit is a branch) is a first-class user-facing concept, not an undo stack.
- Local-first: the audio engine runs entirely on the user's machine. No file ever leaves unless the user explicitly exports.
- AI inference is pluggable: BYO Anthropic key, hosted subscription, or local LLM via Ollama. Switchable without reinstall.
- One unified codebase covers podcast/voice editing as a supporting capability (transcription, filler removal, cleanup pipelines).

**Non-goals (v1)**

- Real-time DJ performance mode (live decks, controller integration, beat-jump). Deferred to v2 desktop companion.
- Note-level audio editing (RipX-style harmonic decomposition). Architecturally enabled but not shipped in v1.
- VST3/CLAP plugin hosting. Pure Rust DSP only — adding plugin hosting is a v2 decision.
- Music generation (Suno/Udio territory). We edit and mix existing audio.
- Mobile (iOS/Android). Desktop only.
- Collaboration / multi-user sessions.
- Cloud project sync. Sessions live on disk; export to file for sharing.
- DAW round-trip (OMF/AAF export, Logic project format). Track in v2 backlog.

## 3. Key decisions (locked in brainstorm)

| Decision | Choice | Rationale |
|---|---|---|
| Product structure | New repo, separate from Treacle | Different domain, different team trajectory; no shared infra makes sense |
| v1 scope | Music production primary; podcast as supporting | Lighthouse demos are music-focused; podcast cleanup is a thin layer over the same engine |
| Form factor | Tauri (Rust) desktop app. **v1 ships Mac + Windows in parallel; Linux deferred to post-v1.** | Cross-platform is the point of choosing Tauri; Linux audience too small to justify the third signing/distribution path in v1 |
| Team | **Solo dev.** Phasing assumes one full-time engineer. | Locked. Hiring would re-phase. |
| Audio engine | Pure Rust DSP graph (`cpal`, `symphonia`, `dasp`/`fundsp`, `rubato`); ML via ONNX Runtime / `candle` sidecars | Highest quality ceiling, no Python in hot path |
| AI inference | Hybrid: BYO Anthropic key OR hosted subscription (proxy backend) OR local LLM (Ollama) | Power-user flexibility; enables free tier; no lock-in |
| Session model | **Branchable mix graph** (every state = DAG node, fork/merge/A-B compare) | Differentiator vs every competitor; needed for conversational mixing UX |
| Public surface | Desktop app primary; engine *also* exposed as remote MCP for Claude Desktop/Code power users | Two distribution channels, one engine; MCP composes with the broader Claude tool ecosystem |

## 4. Lighthouse user scenarios

### 4.1 Mashup (lighthouse demo B)

```
User drops song_A.mp3 + song_B.mp3 onto the canvas.
User: "Mashup — A's vocals over B's instrumental, key-match, beat-align,
       give me 3 takes on the drop."
```

Agent plan (Claude generates and shows before executing):

1. `analyze_track` on both → A: 124 BPM, A minor; B: 128 BPM, F minor.
2. `separate_stems` on both → 4 stems each (vocals, drums, bass, other).
3. Decide alignment: stretch A's vocal stem to 128 BPM via `time_stretch` (Rubber Band, formant-preserving). Pitch-shift A's vocal +8 semitones (A → F).
4. `mix_session` create: A_vocal_shifted + B_drums + B_bass + B_other.
5. Generate 3 branches off the same parent for the drop:
   - branch `drop_v1`: 4-bar build at 1:30, full drop at 1:34
   - branch `drop_v2`: same build, drop comes in on the 2-and (offbeat surprise)
   - branch `drop_v3`: filtered drop (LP sweep open over 2 bars)
6. `render_preview` on all three; user listens via canvas A/B button.

User picks v2, asks "vocal a bit louder in the drop only". Agent forks `drop_v2 → drop_v2a` with automation on the vocal track between 1:34 and 1:38. Renders. User exports.

### 4.2 Conversational mix engineer (lighthouse demo C)

```
User drops a folder of 12 stems from a band recording.
User: "Rough mix this. Modern indie pop. Vocal upfront."
```

Agent presents a render plan: levels (kick -6dB, snare -8dB, vocal -3dB...), high-pass on bass, 1.5kHz presence boost on lead vocal, parallel compression on drums, bus compression, master limiting to -14 LUFS. User approves. Renders.

User: "Drums are too crunchy."
Agent: "I'm hearing the parallel comp pushing the snare snap. Branch with comp ratio 4:1 → 2.5:1, or branch with no parallel comp at all?"
User: "Both."
Agent forks both, renders, user A/Bs against the original mix.

User: "Halfway between those two, and warm up the vocal."
Agent: synthesizes a third branch (3:1 ratio), adds a low-shelf +1.5dB at 200Hz on the vocal, explains the reasoning trace. User accepts.

The session graph now has 5 nodes (original → rough mix → 2 alternatives → final). User can revisit any.

## 5. Architecture

```
┌───────────────────────────────────────────────────────────────────┐
│  Tauri shell (TypeScript/React)                                    │
│  ┌─────────────────┐  ┌────────────────┐  ┌──────────────────┐    │
│  │ Canvas          │  │ Chat panel     │  │ Version graph    │    │
│  │ (waveform,      │  │ (Claude UI)    │  │ (DAG view)       │    │
│  │  timeline,      │  │                │  │                  │    │
│  │  playhead)      │  │                │  │                  │    │
│  └─────────────────┘  └────────────────┘  └──────────────────┘    │
└───────────────────────────────────────────────────────────────────┘
                              │ tauri::command
┌───────────────────────────────────────────────────────────────────┐
│  Rust core                                                         │
│  ┌────────────────┐  ┌────────────────┐  ┌────────────────────┐   │
│  │ Session graph  │  │ Audio engine   │  │ ML pipeline        │   │
│  │ (DAG store,    │  │ (DSP graph,    │  │ (Demucs ONNX,      │   │
│  │  diff/merge,   │  │  cpal I/O,     │  │  Whisper ONNX,     │   │
│  │  serialize)    │  │  rubato,       │  │  pyannote ONNX,    │   │
│  │                │  │  fundsp)       │  │  librosa-rs)       │   │
│  └────────────────┘  └────────────────┘  └────────────────────┘   │
│  ┌─────────────────────────────────────────────────────────────┐  │
│  │ Tool dispatcher (~30 tools, deterministic, no LLM access)   │  │
│  └─────────────────────────────────────────────────────────────┘  │
└───────────────────────────────────────────────────────────────────┘
                              │
┌───────────────────────────────────────────────────────────────────┐
│  AI layer (pluggable, swappable at runtime)                        │
│  ┌──────────────┐  ┌──────────────────┐  ┌──────────────────┐     │
│  │ BYO Claude   │  │ Hosted proxy     │  │ Local LLM        │     │
│  │ (direct API) │  │ (our backend +   │  │ (Ollama, Qwen,   │     │
│  │              │  │  subscription)   │  │  Llama)          │     │
│  └──────────────┘  └──────────────────┘  └──────────────────┘     │
└───────────────────────────────────────────────────────────────────┘
                              │
┌───────────────────────────────────────────────────────────────────┐
│  External MCP surface (optional, for Claude Desktop/Code users)    │
│  Same tool dispatcher exposed over HTTP/SSE on localhost           │
└───────────────────────────────────────────────────────────────────┘
```

### 5.1 Component decomposition

Each row owns one concern, no cross-coupling:

| Component | Responsibility | Dependencies |
|---|---|---|
| **Session graph** | Persist DAG of session states; serialize/deserialize; compute diff between nodes; produce comparison playlists for A/B | Pure Rust, no audio |
| **Audio engine** | Decode files; build DSP graphs from session state; stream output to `cpal`; render to disk | `symphonia`, `cpal`, `rubato`, `fundsp`, `dasp` |
| **ML pipeline** | Load ONNX models; run Demucs/Whisper/pyannote; cache results per-file (content hash) | `ort` (ONNX Runtime), or `candle` if pure-Rust GPU works |
| **Tool dispatcher** | Receive tool calls (from Claude or MCP); validate args; invoke engine + ML; return structured results | All of the above |
| **AI layer** | Prompt Claude/local LLM; manage tool-call loop; route to BYO/proxy/local | `reqwest`, OpenAI-compatible client for Ollama |
| **Tauri shell** | Canvas (waveform via WebAudio for preview, native render for final), chat UI, graph view | TS/React/whatever frontend stack chosen |
| **MCP server** | Expose tool dispatcher over local HTTP/SSE; auth via shared-secret token | Same dispatcher, different transport |

### 5.2 Data flow (single user turn)

1. User types in chat: "vocal warmer".
2. Frontend sends turn to AI layer with current session-graph node ID.
3. AI layer hydrates context: session summary (track names, key, BPM, current effects), reasoning trace from previous turns, available tools.
4. Claude responds with a plan: *"branch from current, low-shelf +1.5dB at 200Hz on vocal track, render preview"*.
5. AI layer surfaces plan to user (via streaming chat) and emits tool calls to the dispatcher.
6. Dispatcher: creates new graph node forked from current, mutates session state in the new node, calls audio engine to render preview to a temp WAV.
7. Frontend gets new node ID + preview path → loads in canvas, enables A/B button against parent.
8. User listens, accepts (graph node confirmed) or rejects (node soft-deleted).

## 6. Session graph data model

The differentiator. Every session state is an immutable node in a DAG.

```rust
pub struct SessionNode {
    id: NodeId,                       // content-hashed
    parent: Option<NodeId>,           // null only for root
    created_at: DateTime<Utc>,
    label: Option<String>,            // user or AI assigned ("vocal warmer", "drop v2")
    reasoning: Option<String>,        // AI's explanation for why this node exists
    state: SessionState,              // full snapshot
}

pub struct SessionState {
    tracks: Vec<Track>,               // each track: clips, effects, automation
    bus_routing: BusGraph,            // sends, returns, parallel chains
    master_chain: Vec<EffectInstance>,
    tempo_map: TempoMap,
    key_map: Option<KeyMap>,          // per-section key info, for music
    transcript: Option<Transcript>,   // for podcast/voice
    sample_rate: u32,
    length_samples: u64,
}
```

**Operations on the graph:**

- `fork(node) -> node` — clone a node as a child. O(1) via copy-on-write.
- `diff(a, b) -> SessionDiff` — what changed between two nodes (effect added, gain changed, clip moved). Used for AI reasoning ("in branch X you boosted 200Hz...") and for the UI's "what's different" view.
- `compare(a, b) -> ComparisonPlaylist` — render both, expose to A/B switch in canvas.
- `merge(a, b) -> Option<node>` — only valid for non-conflicting diffs (e.g. branch X changed bass, branch Y changed vocal → merge ok). Conflicts surface to user.
- `name(node, label)`, `pin(node)`, `delete(node)` — housekeeping.

**Storage:** JSON-serialized nodes on disk under `<project>/.audiograph/<node_id>.json`. Audio renders cached separately, content-addressed by `(node_id, render_settings_hash)`.

**Why this works for AI:** Claude can reason over the graph as structured data ("which branch does the user prefer based on their feedback?") and can author multi-branch experiments in a single tool call (`fork_and_apply([{op: ..., label: "v1"}, {op: ..., label: "v2"}])`).

## 7. Tool surface (~30 tools, dispatcher API)

The agent calls these. Each is deterministic, side-effect-bounded, and cheap to test.

**Analysis (read-only):**
- `analyze_track(path)` → bpm, key, beats[], downbeats[], sections[], rms_curve, lufs
- `transcribe(path)` → words[] with timestamps + speaker labels
- `detect_silences(path, threshold_db)` → ranges
- `find_transients(path)` → onset times

**Decomposition:**
- `separate_stems(path, model)` → 4-6 stem files (cached by content hash)

**Editing primitives:**
- `cut_range(track, start, end)`, `trim`, `split`, `crossfade`, `gain_automation`
- `time_stretch(clip, factor, preserve_formants)` (Rubber Band)
- `pitch_shift(clip, semitones, preserve_formants)`
- `align_to_beat(clip, beat_grid)`
- `quantize(clip, grid, strength)`

**Effects (each as a graph node):**
- `apply_eq(track, bands[])`
- `apply_compressor(track, threshold, ratio, attack, release, makeup)`
- `apply_reverb(track, ir | algorithm, mix)`
- `apply_limiter`, `apply_de_esser`, `apply_filter`, `apply_saturation`

**Routing:**
- `set_track_gain`, `set_track_pan`, `add_send`, `set_bus_routing`

**Mixing pipelines (composite, AI-tunable presets):**
- `mix_for_streaming(target_lufs)` — full chain to LUFS target
- `master_for_genre(genre)`
- `cleanup_voice` (HPF, de-ess, gate, normalize, leveler)

**Session graph:**
- `fork_node`, `apply_diff(diffs[])` (atomic multi-op), `compare_nodes(a, b)`, `revert_to(node)`, `name_node`

**Render:**
- `render_preview(node, range)` → wav (low-latency)
- `render_final(node, format, options)` → mp3/wav/flac/ogg

**Library:**
- `list_files(folder)`, `import_file(path)`, `export(node, path)`

## 8. AI agent design

**Single-agent, tool-calling, streaming.** No multi-agent in v1 (debugging cost too high; one agent + good prompts wins).

**System prompt** establishes the agent as a producer/engineer with three operating modes (auto-selected from user request):

1. *Mashup mode* — explicit plan-before-execute. Always show the user the plan before any non-trivial render.
2. *Mix engineer mode* — tighter loops, smaller tool batches, frequent A/B previews. Always offer to fork rather than overwrite.
3. *Voice/podcast mode* — transcript-first, batch operations OK without prior approval (low-stakes ops like filler removal).

**Context provided per turn:**

- Current node ID + summary (tracks, durations, key, bpm, recent effects)
- Last N turns of conversation
- Recent reasoning trace (why prior decisions were made)
- Tool catalog (filtered by current mode)

**Caching:** Anthropic prompt caching on the system prompt + tool catalog (these are stable). Per-turn payload kept small.

**Token budget per turn:** target <8k input tokens, <2k output. Claude Sonnet 4.6 default; Haiku 4.5 for cheap classification (mode detection, quick yes/no).

**Local LLM mode:** Qwen-2.5-Coder-32B or Llama 3.3 70B via Ollama. Tool-calling support is shakier; we ship a simpler tool surface in this mode (subset of ~12 tools), and accept lower planning quality.

## 9. Build phases

Three milestones, each shippable.

### Phase 1 — "Edit a single track" (~9 weeks)

Goal: foundation works end-to-end on the simplest case, on both target platforms.

- Tauri shell, canvas with waveform, basic chat
- Audio engine: load WAV/MP3, play, render. Core Audio on Mac; WASAPI on Windows (ASIO deferred to post-v1).
- Session graph: linear (no branching yet) — but data model in place
- 8 tools: load, transcribe, cut_range, trim, gain, normalize, render_preview, render_final
- BYO Claude key only (no proxy, no local LLM)
- **Both platforms: macOS + Windows.** Code signing + notarization (Mac) and Authenticode signing + SmartScreen reputation seeding (Windows) wired into CI from day one. WebView2 install/runtime handled.

Demo: "remove silence at the start, normalize, export." Verified on a Mac (Apple Silicon) and a Windows 11 box.

**Why +3 weeks vs the original macOS-only estimate:** dual-platform CI, two signing pipelines, WebView2 packaging, testing audio I/O on WASAPI. None individually hard; collectively non-trivial for a solo dev.

### Phase 2 — "Mashup" (~10 weeks)

- Add Demucs ONNX integration → `separate_stems`
- Add `analyze_track` (BPM/key/beats via librosa-rs or aubio bindings)
- Add `time_stretch`, `pitch_shift`, `align_to_beat` (Rubber Band)
- Multi-track session model
- Branchable session graph + A/B compare in canvas
- Canvas: graph view added
- (Windows already shipped in Phase 1.)

Demo: lighthouse B (mashup any two songs).

### Phase 3 — "Conversational mix engineer" (~10 weeks)

- Effects: EQ, compressor, reverb, limiter, de-esser, saturation (all pure Rust DSP)
- Bus routing, sends, master chain
- Mix pipeline tools (`mix_for_streaming`, `master_for_genre`)
- Hosted-subscription AI path (proxy backend + auth + billing — minimal)
- Local LLM path via Ollama
- MCP server exposed on localhost
- (Linux deferred to post-v1.)

Demo: lighthouse C (drop stems, conversational refinement).

**Total v1 target: ~6.5-7 months from project start (solo).**

Phases 4+ (post-v1, prioritization TBD): Linux build, ASIO support on Windows, note-level editing, plugin hosting, DAW round-trip, real-time DJ mode, mobile companion.

## 10. Error handling & failure modes

| Failure | Detection | Behavior |
|---|---|---|
| File decode fails | `symphonia` returns error | Surface in chat: "Couldn't decode foo.mp3 — corrupt or unsupported codec." Don't crash. |
| Stem separation OOM | Process exit code | Fall back to lower-quality model (`htdemucs_ft` → `htdemucs`); retry. If still fails, tell user to free RAM. |
| Render fails mid-way | Engine returns partial | Mark the target node as `render_failed`, keep partial cached for inspection, surface error. Graph state remains valid (state was saved before render started). |
| AI returns invalid tool call | Schema validation in dispatcher | Reject, return error to AI loop, agent retries (max 3). |
| User runs out of disk for cache | Pre-render disk check | Block render with actionable message; offer cache cleanup. |
| Claude API down (BYO mode) | HTTP error | Surface; offer to switch to local LLM if available. |

**Crash safety:** session graph is durable on disk after every node creation. App crash mid-render leaves the graph valid; the in-flight render simply isn't materialized. Restart resumes cleanly.

## 11. Testing strategy

- **Unit tests** for every DSP block: golden WAV in, deterministic WAV out, byte-compare with checked-in reference. Catches regressions in pure-Rust effects which are the highest-risk code.
- **Integration tests** for the dispatcher: scripted tool sequences (load → cut → render) verify end-to-end output against reference renders.
- **Property tests** (`proptest`) for the session graph: any sequence of fork/apply/revert leaves the graph valid; serialize/deserialize roundtrips are identity.
- **Snapshot tests** for AI prompts: lock down the system prompt and per-mode templates; intentional changes require updating the snapshot.
- **Listening tests** (manual, gated): a small reference set of mix tasks rendered nightly and listened to by the team. Cannot be automated; non-negotiable for shipping.
- **No "vibes only" PRs.** A PR that changes a DSP node must include a golden WAV diff and a one-line listening note.

## 12. Open questions (to resolve in planning, not blocking spec)

**Resolved 2026-05-05:**
- ~~Platform priority~~ → Mac + Windows ship in parallel for v1; Linux deferred. (Cross-platform is the reason for choosing Tauri.)
- ~~Solo or team~~ → Solo. Phasing assumes one full-time engineer.

**Still open:**

1. **Brand / product name.** Working title only.
2. **Open source vs proprietary.** A lean OSS core (engine + tools) with a proprietary AI proxy + premium features (advanced mixing pipelines, cloud sync) is the obvious split. Confirm before phase 3.
3. **Pricing.** Free tier (BYO key, all features); paid sub ($15-25/mo) for hosted AI proxy + premium mix presets + priority models. Confirm before phase 3.
4. **Stem separation quality vs latency.** `htdemucs_ft` is highest quality but slowest. Default to `_ft` and offer "fast" toggle? Or auto-select by file length?
5. **librosa-rs vs aubio bindings vs custom.** Music feature extraction has no clean Rust answer. May need to ship one Python sidecar in v1 (`librosa`) and migrate to pure Rust over time. Acceptable compromise?
6. **MCP auth model.** Localhost-only with bearer token? Or unix socket? Concrete in phase 3 plan.
7. **Telemetry.** Crash reports yes (Sentry-equivalent); usage telemetry only with explicit opt-in. PostHog or self-hosted? Privacy-first product → leans self-hosted.
8. **Distribution.** Direct download? Mac App Store? Setapp? Microsoft Store? Each has signing/sandboxing implications for the audio engine.
9. **Windows ML acceleration default.** CPU-only at launch (works everywhere, slow), CUDA opt-in for NVIDIA users, or DirectML/WinML for broad GPU coverage? CUDA-only is simplest to ship; DirectML is in maintenance mode per Microsoft.

## 13. Risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Pure Rust DSP quality lags commercial plugins | Medium | High | Reference-track listening tests gate every effect. If a node fails the bar, swap implementation (e.g. swap homemade compressor for `compressor_rs` crate or wrap a C library). |
| Stem separation too slow on CPU | High | Medium | First class GPU support (Metal on Mac, CUDA on Windows/Linux) via ONNX Runtime. Offer cloud rendering as paid escape hatch. |
| Branchable graph UX overwhelms casual users | Medium | High | Default to "linear timeline" view; graph view opt-in. Pro users discover it; casuals never see it. |
| Local LLMs can't reliably tool-call | High | Medium | Ship simplified tool subset for local mode; accept lower quality. Document the gap. |
| Scope creep into note-level editing | High | Critical | Hard-coded in Non-goals. PRs touching note-level decomposition rejected pre-v1. |
| Music feature extraction (no good Rust lib) blocks phase 2 | Medium | High | Accept Python sidecar (`librosa`) as v1 compromise; track replacement. |
| 6-month timeline slips because solo dev | Medium | Medium | Phasing is independently shippable — phase 1 alone is a useful product (single-track AI editor). Worst case: ship phase 1, gather feedback, decide. |
| Anthropic pricing changes break BYO economics | Low | Medium | Hybrid AI strategy already hedges this. Local LLM path always available. |

## 14. What's next

Both blocking questions are resolved (Mac + Windows parallel; solo dev). Next:

1. Produce an implementation plan for Phase 1 (the Phase 1 section of §9 is the seed).
2. Stand up CI with Mac + Windows runners and signing pipelines as the very first task in Phase 1 — every subsequent task assumes both platforms green.

The Phase 2 and Phase 3 plans should be drafted only after Phase 1 is in flight; deciding their detail now is premature.
