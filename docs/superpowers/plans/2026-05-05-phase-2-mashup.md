# Phase 2 — "Mashup" Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Each Module below is a unit of work; per-module 2–5 minute TDD steps are produced at execution time using `executing-plans` against the module's acceptance criteria.

> **Status:** Drafted up-front per user request. Spec §14 cautions that Phase 2 detail is premature before Phase 1 ships — the modules below should be **revisited and re-locked** after Phase 1 GA. Library versions and tool surface are fixed; module sequencing is tentative.

**Goal:** Ship Lighthouse Demo B — drop two MP3s, say *"mashup A's vocals over B's instrumental, key-match, beat-align, give me 3 takes on the drop"*, render and compare three branches.

**Architecture:** Add an ML pipeline crate hosting Demucs (stem separation) and music feature extraction (BPM/key/beats). Add Rubber Band-driven time-stretch and pitch-shift. Promote the session graph from linear to true DAG (fork/diff/compare). Frontend gains a graph view and an A/B compare toggle. No new platform — Mac+Windows from Phase 1.

**Tech Stack additions:** ort 2.0 (already in P1) + Demucs ONNX (htdemucs_ft), `rubberband-sys = "0.3"` (FFI to Rubber Band 3.x C++ library), `aubio-rs = "0.2"` for onset/beat detection (or Python sidecar fallback — see M21 decision), `petgraph = "0.7"` for DAG ops, `react-flow = "11.x"` (graph view).

**Timeline target:** 10 weeks solo (range 9–14).

**Out of scope this phase:** any DSP effect (EQ/comp/etc.), bus routing, mix pipelines, hosted proxy, local LLM, MCP server, Linux build.

---

## Spec coverage map

| Spec § | Requirement | Module(s) |
|---|---|---|
| §4.1 | Mashup walkthrough end-to-end | M22, M23, M28 |
| §5.1 | ML pipeline crate (Demucs, content-hashed cache) | M17 |
| §6 | Branching graph ops: fork, diff, compare, merge | M24 |
| §7 | `analyze_track` (BPM, key, beats, downbeats, sections, RMS, LUFS) | M19 |
| §7 | `separate_stems` | M18 |
| §7 | `time_stretch`, `pitch_shift`, `align_to_beat` | M20 |
| §7 | `apply_diff` (atomic multi-op for AI to fork-and-apply) | M24 |
| §7 | `compare_nodes` | M24 |
| §8 | Mashup-mode system prompt (plan-before-execute) | M27 |

---

## File / crate structure (deltas vs. Phase 1)

```
crates/
├── ml-pipeline/                # NEW (M17) — generic ONNX Runtime hosting + content-hashed cache
│   └── src/{lib,cache,onnx_session}.rs
├── ml-demucs/                  # NEW (M18) — htdemucs wrapper, 4-stem output
├── ml-whisper/                 # (already from P1)
├── audio-analysis/             # NEW (M19) — BPM/key/beats/sections; either Rust (aubio-rs) or Python sidecar
│   └── src/{lib,bpm,key,beats}.rs
├── audio-time/                 # NEW (M20) — time-stretch + pitch-shift via Rubber Band
├── session/                    # MODIFY (M24) — promote DAG ops out of unimplemented!()
│   └── src/{diff,compare,merge}.rs   # newly fleshed out
└── tools/
    └── src/tool/{separate_stems,analyze_track,time_stretch,pitch_shift,align_to_beat,apply_diff,compare_nodes}.rs

apps/desktop/src/components/
├── GraphView.tsx               # NEW (M25) — react-flow DAG view
├── ABCompareBar.tsx            # NEW (M26) — A/B toggle
└── Timeline.tsx                # MODIFY (M26) — multi-track stacked layout

assets/models/
├── htdemucs_ft.onnx            # NEW — fetched at first use
└── (whisper-base.en.onnx already from P1)
```

---

## Modules

### M17 — ML pipeline crate (generic ONNX hosting + content-hashed cache)

**Files:**
- Create: `crates/ml-pipeline/Cargo.toml` (deps: `ort = "2.0"`, `blake3`, `serde`, `tokio`, `tracing`)
- Create: `crates/ml-pipeline/src/{lib,cache,onnx_session,download}.rs`
- Create: `crates/ml-pipeline/tests/cache_smoke.rs`

**Public API:**
```rust
pub struct ModelRegistry { /* model_id -> ort::Session, lazy-loaded */ }
pub struct InferenceCache { /* maps blake3(content) -> result */ }
impl ModelRegistry {
    pub fn load(model_id: &str, path: &Path, ep: ExecProvider) -> Result<Arc<ort::Session>>;
}
impl InferenceCache {
    pub fn get_or_compute<T: Serialize + DeserializeOwned, F: FnOnce() -> Result<T>>(
        &self, key: ContentHash, compute: F) -> Result<T>;
}
pub enum ExecProvider { Cpu, CoreML, Cuda } // Mac default = CoreML; Windows default = Cpu (CUDA opt-in)
```

**Acceptance criteria:**
1. Loading the same model twice returns the same `Arc<Session>` (registry caches).
2. Inference cache: same input bytes → cached result on disk under `<project>/.audiograph/inference-cache/<blake3>.bin`.
3. Cache invalidates on model file change (key includes model hash).
4. CoreML EP loads on Mac without panicking; CPU EP works on both platforms.

**Risk:** Medium. ONNX Runtime EP support varies by platform; on Mac, CoreML setup needs the right `ort` features.

**Estimate:** 1 week.

---

### M18 — Demucs stem separation (`ml-demucs` crate + `separate_stems` tool)

**Files:**
- Create: `crates/ml-demucs/Cargo.toml` (deps: `ml-pipeline`, `audio-decoder`, `ndarray`)
- Create: `crates/ml-demucs/src/lib.rs`
- Create: `crates/tools/src/tool/separate_stems.rs`
- Create: `tests/golden/stems_30s_pop_clip/` — 4 reference stems for a 30 sec public-domain music clip; tolerance-checked rather than byte-compared.

**Tool spec:**
- `separate_stems`: `{path: string, model: "htdemucs_ft" | "htdemucs"}` → `{stems: {vocals, drums, bass, other: path}}`. Cached by content hash of input + model id.

**Acceptance criteria:**
1. On the 30s reference clip, output stems' RMS correlation with ground-truth stems > 0.85 per stem.
2. Same input → byte-identical cached path on second invocation (no re-inference).
3. CoreML on Apple Silicon completes a 3-min track in ≤ 90s on baseline 2023 M2; CPU on a baseline 2022 desktop Windows in ≤ 6 min.
4. OOM fallback: on `out of memory` error, retries with `htdemucs` (smaller, faster) and logs a warning to chat.

**Test design:**
- Reference stems checked into `tests/golden/` (license-clean clip — likely a Free Music Archive track).
- Spectral correlation rather than byte compare.

**Risk:** High. Demucs ONNX export is non-trivial; pre-exported community models exist but quality and licensing vary. Spend the first 2 days on model sourcing before module proper.

**Estimate:** 1.5 weeks.

---

### M19 — Music feature extraction (`audio-analysis` crate + `analyze_track` tool)

**Files:**
- Create: `crates/audio-analysis/Cargo.toml`
- Decision (locked at start of M19): **`aubio-rs` for BPM/onsets/beats** (pure-Rust-callable, FFI to aubio C library). For key detection, prefer Rust/ONNX over a Python sidecar (sidecars complicate signing, notarization, and binary size). Selection ladder, evaluated in order on day 1 of M19:
  1. **Preferred — ONNX key model.** Find or export an ONNX-format key/mode classifier (Essentia ships several Python-trained models; verify whether one is exported to ONNX or convert via `onnx-tf`). Run via `ort` (already a dep from M17). Stays in the Rust/C++ ecosystem.
  2. **Fallback — pure-Rust chroma + Krumhansl-Schmuckler.** Custom: `realfft 3` for spectrum → 12-bin chroma → Krumhansl-Schmuckler correlation → 24-key (major/minor) classification. ~200 LoC; accuracy floor ~70%, the M19 acceptance bar.
  3. **Last resort — Python sidecar.** Only if (1) and (2) both fail. Bundled via `pyoxidizer`. Cost: +60-80 MB binary, signing complexity (each Python `.so` notarized separately on Mac), startup latency.
- Create: `crates/audio-analysis/src/{lib,bpm,key,beats,sections,loudness}.rs` (one of `key_onnx.rs` or `key_chroma.rs` filled depending on ladder outcome)
- Create: `crates/tools/src/tool/analyze_track.rs`

**Tool spec:**
- `analyze_track`: `{path: string}` → `{bpm: f32, key: string ("F minor"), beats: [f32], downbeats: [f32], sections: [{name: string, start_s, end_s}], rms_curve: [f32], lufs_integrated: f32}`.

**Acceptance criteria:**
1. On a corpus of 10 reference tracks (with known BPM), reported BPM is within 1 BPM of ground truth in 9/10. Half-time / double-time detection is accepted.
2. Key detection is correct for 7/10 reference tracks — explicit limitation documented; users can override by tool arg `key_hint`.
3. Beats array is monotonic and roughly periodic (CV < 5%) for 4/4 music.
4. LUFS calculation matches `ffmpeg -af ebur128` within 0.5 LU.

**Test design:** Reference corpus checked in (license-clean clips ≤ 30 sec each, 10 files). Manual ground truth labels.

**Risk:** Medium-high. Music feature extraction in 2026 is still messy in Rust. The selection ladder front-loads risk to day 1 of M19; if ONNX and pure-Rust both fail by EOD-2, fall back to sidecar with the packaging cost absorbed.

**Estimate:** 1.5 weeks.

---

### M20 — Time/pitch (`audio-time` crate + `time_stretch`, `pitch_shift`, `align_to_beat` tools)

**Files:**
- Create: `crates/audio-time/Cargo.toml` (deps: `rubberband-sys`)
- Create: `crates/audio-time/src/{lib,stretch,shift}.rs`
- Create: `crates/tools/src/tool/{time_stretch,pitch_shift,align_to_beat}.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml` to bundle Rubber Band library on both platforms (macOS via vcpkg or homebrew at build time; Windows via vcpkg).

**Tool specs:**
- `time_stretch`: `{track: usize, factor: f32, preserve_formants: bool}` → new node.
- `pitch_shift`: `{track: usize, semitones: f32, preserve_formants: bool}` → new node.
- `align_to_beat`: `{track: usize, beat_grid: [f32]}` → new node; resamples each beat-bound chunk to the target grid.

**Acceptance criteria:**
1. Time-stretch by 0.5× of a 1s 440Hz tone produces a 2s 440Hz tone (peak frequency unchanged); duration tolerance ±10ms.
2. Pitch-shift by +12 semitones of a 440Hz tone produces an 880Hz tone (peak frequency); tolerance ±2Hz.
3. `preserve_formants=true` on a vocal stem keeps formant frequencies (F1, F2) within 5% of source — verified by LPC analysis on a reference vocal clip.
4. Round-trip: stretch by `f` then by `1/f` recovers the source within RMS error ≤ -40 dBFS.

**Test design:** Synthetic tone tests for primary correctness; vocal-formant test for preservation; round-trip RMS as the determinism check.

**Risk:** Medium. Rubber Band is high quality but the FFI layer needs care, especially on Windows where shipping the C++ DLL is non-trivial.

**Estimate:** 1.5 weeks.

---

### M21 — Multi-track session state

**Files:**
- Modify: `crates/session/src/state.rs` — `Track` already in P1 schema; promote `Vec<Track>` to actually hold > 1.
- Modify: `crates/audio-engine/src/{graph,mixer}.rs` — render multi-track mixdown.
- Modify: `crates/tools/src/tool/load.rs` — second `load` appends a track instead of replacing.

**Public API change:**
```rust
// new tool:
// add_track / remove_track / set_track_gain were already in spec §7; add the first three here.
```

**Acceptance criteria:**
1. Load two tracks → render → output is the sample-by-sample sum (with gain) of the two source files.
2. Per-track mute/solo flags supported in `SessionState`; render respects them.
3. Tracks of different sample rates: auto-resampled to project rate at render time using `rubato` (already a dep from P1 M09).

**Test design:** Sum-of-two-tones test (440Hz + 880Hz → spectrum has both peaks); per-track gain test; resample test.

**Risk:** Low-medium. The per-track determinism trap from P1 M06 reappears at multi-track scale.

**Estimate:** 1 week.

---

### M22 — Multi-track audio engine + render

**Files:**
- Modify: `crates/audio-engine/src/render.rs`
- Add streaming render so a 10-track 5-min session doesn't OOM.

**Acceptance criteria:**
1. Stream-renders an 8-track session at constant memory (≤ 200 MB regardless of track count) by reading source files in 1-sec chunks and accumulating into a single output buffer.
2. Cross-platform sample-identical output preserved (CI gate from P1 M06 extended).
3. Render time scales linearly in track count, not super-linearly.

**Risk:** Medium. Streaming + determinism + cross-platform is a tight set.

**Estimate:** 0.5 weeks (much of this is straightforward refactor).

---

### M23 — Mashup-specific tool composition tests

**Files:**
- Create: `tests/e2e/mashup_walkthrough.rs` — drives the full §4.1 spec walkthrough through the CLI binary from P1 M16.
- Create: `tests/fixtures/mashup_a.wav`, `tests/fixtures/mashup_b.wav` — two short, license-clean clips with very different BPM and key (e.g. 120 BPM C major vs 100 BPM A minor).

**Steps tested:**
1. `analyze_track` on both → returns sensible BPM, key.
2. `separate_stems` on both → 4 stems each.
3. `pitch_shift` + `time_stretch` on A's vocal stem to align to B's tempo and key.
4. Synthesize a 3-track session: A's vocal_shifted + B's drums + B's bass.
5. `render_final` → output WAV plays correctly (manual listen) and analyzed BPM/key matches B.

**Acceptance criteria:**
1. The whole sequence runs in CI in ≤ 5 min on macOS runner, ≤ 12 min on Windows runner.
2. Final BPM measured by `analyze_track` on the output is within 0.5 BPM of B's BPM.
3. Manual listen: result is musically coherent (no glitches, no audible alignment errors > 30ms at section boundaries).

**Risk:** This module is the integration sentinel for M17–M22.

**Estimate:** 1 week.

---

### M24 — Branching session graph (DAG ops)

**Files:**
- Modify: `crates/session/src/{lib,diff,compare,merge}.rs` — fill in stubs from P1 M05.
- Create: `crates/tools/src/tool/{fork_node,apply_diff,compare_nodes,revert_to,name_node}.rs`

**Public API now real:**
```rust
impl Store {
    pub fn fork(&mut self, parent: NodeId) -> Result<NodeId>; // creates a new node clone
    pub fn diff(&self, a: NodeId, b: NodeId) -> SessionDiff;
    pub fn merge(&mut self, a: NodeId, b: NodeId) -> Result<NodeId>; // None on conflict (returns Err)
}
pub struct SessionDiff { pub added: Vec<DiffOp>, pub removed: Vec<DiffOp>, pub modified: Vec<(DiffOp, DiffOp)> }
```

**`apply_diff` tool** — atomic multi-op so the AI can author multi-branch experiments in a single tool call. Args: `{from_node: NodeId, branches: [{ops: [DiffOp], label: string}]}` → returns `{branches: [NodeId]}`.

**Acceptance criteria:**
1. `fork(x)` creates a child of `x` with state byte-identical to `x`.
2. `diff(a, b)` is symmetric: `diff(a,b)` reversed equals `diff(b,a)`.
3. Property test: arbitrary fork/apply/revert sequences leave the store in a valid state (all parent pointers resolve).
4. `merge` returns `Err(MergeConflict)` when both branches modified the same effect on the same track.
5. `apply_diff` with 3 branch specs produces 3 new nodes parented to `from_node`.

**Test design:** `proptest` for graph invariants; snapshot tests for diff output shapes.

**Risk:** Low-medium. Pure data structure work; well-trodden CRDT-adjacent territory.

**Estimate:** 1 week.

---

### M25 — Frontend graph view

**Files:**
- Create: `apps/desktop/src/components/GraphView.tsx` (uses `react-flow`)
- Modify: `apps/desktop/src/App.tsx` — add a tab/toggle: "Timeline" vs. "Graph".
- Modify: `apps/desktop/src-tauri/src/commands.rs` — expose `get_graph()` returning all nodes + parent links.

**UI behaviors:**
- Nodes show: short label (or AI's first-3-words summary), tool that produced them, time.
- Edges show parent → child.
- Click node → canvas loads that node's preview.
- Right-click node → "Set as head", "Compare with…", "Rename", "Delete".
- Branches color-coded by lineage.

**Acceptance criteria:**
1. A session of 8 nodes with 2 branches renders correctly; manual layout is readable.
2. Clicking a node updates head; canvas re-renders within 1 sec.
3. Performance: 200-node graph renders in < 500ms.

**Risk:** Medium. UX-heavy; the graph view is what makes the differentiator visible — budget polish time.

**Estimate:** 1 week.

---

### M26 — A/B compare bar + multi-track timeline

**Files:**
- Create: `apps/desktop/src/components/ABCompareBar.tsx`
- Modify: `apps/desktop/src/components/Timeline.tsx` — vertically stack tracks; show track names; click-to-mute.
- Modify: `apps/desktop/src-tauri/src/commands.rs` — `prepare_compare(a: NodeId, b: NodeId)` pre-renders both.

**UI behaviors:**
- A/B bar: large toggle "A | B" plus "Switch on ↑" arrow showing the playhead position; clicking toggles instantly without restart.
- Pre-render both sides on `prepare_compare` so the toggle is gapless.
- Mark the "A" side as the parent and "B" side as the candidate by default; "Accept B" promotes B to head.

**Acceptance criteria:**
1. A/B switch latency < 50ms (no re-render lag).
2. Sample-accurate alignment: switching mid-playback doesn't cause a click or glitch.
3. "Accept B" updates head and the graph view both update.

**Risk:** Medium. Gapless A/B requires both renders pre-loaded; memory usage doubles per compare.

**Estimate:** 1 week.

---

### M27 — Mashup-mode system prompt + plan-before-execute UX

**Files:**
- Create: `crates/ai/prompts/mashup_mode.md`
- Modify: `crates/ai/src/loop.rs` — mode detection (Sonnet 4.6 classifier, or Haiku for cheap mode) routes the system prompt.
- Add a "Plan" message type: agent emits a special structured `plan` block (tool-use schema) before executing; frontend renders an "Approve plan" UI.

**Behaviors:**
- Mode detection on every user turn: classifier outputs `mashup` | `mix` | `voice` | `general`.
- Mashup mode: agent ALWAYS emits a `plan` first, listing intended tool calls + reasoning. User clicks "Run plan" to execute. Plans can be edited inline (drop a step, change a parameter).
- After plan executes, agent presents 3 forks if appropriate; user picks one to make head.

**Acceptance criteria:**
1. Mode classifier accuracy on 20 hand-labeled prompts ≥ 18/20.
2. Plan UI never auto-runs in mashup mode (verified by event sequence test).
3. Plan-edit happy path works on Mac and Windows.

**Risk:** Medium. The plan-before-execute UX is novel; UX iteration likely needed.

**Estimate:** 1 week.

---

### M28 — Lighthouse Demo B end-to-end test + screen recording

**Files:**
- Modify: `tests/e2e/mashup_walkthrough.rs` from M23 to add UI E2E via Playwright-Tauri.
- Create: `assets/demos/phase2-mashup.mp4` — 90-second screen recording of the full demo.

**Demo script (matches §4.1):**
1. Drop `mashup_a.mp3` and `mashup_b.mp3`.
2. Type the spec's mashup prompt.
3. Approve plan (3 forks for the drop).
4. A/B compare each, pick v2.
5. Refine ("vocal louder in the drop") → fork.
6. Export.

**Acceptance criteria:**
1. Full sequence completes in < 4 minutes wall clock on both Mac and Windows.
2. Output renders are musically coherent (manual listen gate).
3. 90-second screen recording produced and saved.

**Risk:** Integration risk; budget 1 week for surprises.

**Estimate:** 1 week.

---

## Phase 2 schedule (10 weeks)

| Wk | Modules in flight |
|---|---|
| 1 | M17, M21 (parallel — different parts of stack) |
| 2 | M17 finish, M18 start, M22 |
| 3 | M18 finish, M19 start |
| 4 | M19 finish, M20 |
| 5 | M20 finish, M23 |
| 6 | M23 finish, M24 |
| 7 | M25 |
| 8 | M26 |
| 9 | M27 |
| 10 | M28 |

## Risks & mitigations (Phase 2)

| Risk | Likelihood | Mitigation |
|---|---|---|
| Demucs ONNX export quality lags PyTorch reference | Medium | M18 budgets 2 days for model sourcing first. If gap is large, ship `htdemucs` (not `_ft`) as default and offer "high quality (slow)" mode. |
| Key detection accuracy gap | Medium | M19 ladder (ONNX → Rust → sidecar) front-loads the choice to day 1. If all three options miss the 70% accuracy bar, ship without key detection in v1 and document — pitch-shifting still works via user-supplied `key_hint`. |
| Rubber Band C++ FFI Windows build flakiness | Medium | M20 uses `vcpkg` consistently; CI builds from source on Windows to avoid binary mismatches. |
| Graph view UX feels confusing to casual users | Medium | M25 ships with Timeline as default view; Graph is opt-in toggle (matches spec §13 mitigation). |
| Schedule slip from Phase 1 cascades | High (solo dev) | Phase 2 modules are mostly additive; if Phase 1 ships at week 11 instead of 9, Phase 2 starts at week 11 with no rework. |

## Open questions deferred to execution

- **Demucs default model:** `htdemucs_ft` (best, ~5x slower) vs. `htdemucs` (fast). Tentative: auto-select by file length (≤ 4 min → `_ft`, otherwise `htdemucs`).
- **Music key detection accuracy:** if sidecar accuracy < 70%, ship without key detection in v1 and document.
- **Graph view density:** at how many nodes does the layout become unreadable? Plan to auto-collapse subtrees beyond ~30 nodes.
- **A/B compare memory:** double-render is expensive for long sessions. Consider streaming both renders rather than pre-loading entirely.

## Self-review checklist run

- [x] Spec coverage map: every Phase-2-relevant spec section has a module.
- [x] No "TBD/TODO" placeholders.
- [x] Type names consistent: `NodeId`, `SessionState`, `SessionDiff`, `Tool` align with Phase 1.
- [x] Mashup walkthrough (§4.1) maps to a single integration module (M28).
- [x] Branching graph operations land before frontend graph view (M24 → M25).
- [x] Demucs and Whisper share the ONNX runtime via M17 — no duplicate ORT setup.

## Execution handoff

Plan saved to `docs/superpowers/plans/2026-05-05-phase-2-mashup.md`. Phase 1 must ship before this plan is executed; on Phase 1 GA, **re-review this plan against learnings** (likely 1-day refresh) before starting M17.
