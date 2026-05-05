# Phase 3 — "Conversational Mix Engineer" Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Each Module below is a unit of work; per-module 2–5 minute TDD steps are produced at execution time using `executing-plans` against the module's acceptance criteria.

> **Status:** Drafted up-front per user request. Spec §14 cautions Phase 3 detail is premature before Phase 1 (and now Phase 2) ships — re-lock this plan after Phase 2 GA. The DSP module list and AI surface are stable; sequencing and library choices may shift.

**Goal:** Ship Lighthouse Demo C — drop a folder of stems, say *"rough mix this, modern indie pop, vocal upfront"*, agent renders a plan, refine through chat with A/B at every turn, ship a final mix. This is the depth/moat demo.

**Architecture:** Add a pure-Rust DSP effects crate (parametric EQ, compressor, reverb, limiter, de-esser, filter, saturation) plugged into the existing `audio-engine` graph. Add bus routing and master chain. Add three composite mix-pipeline tools. Add the hosted-subscription AI path (Stripe + a thin proxy backend) and the local LLM path (Ollama/OpenAI-compatible). Expose the engine as a localhost MCP server for Claude Desktop/Code power users.

**Tech Stack additions:** `fundsp = "0.20"` (used as a reference, but most effects custom-implemented for control); `biquad = "0.5"` (parametric EQ filters); `rubato 0.16` (already from P1); `axum = "0.8"` (proxy backend + MCP server); `stripe-rust = "0.41"`; `async-openai = "0.30"` (OpenAI-compatible client for Ollama); `mcp-server-rs` (or hand-roll the MCP transport).

**Timeline target:** 10 weeks solo (range 10–14).

**Out of scope this phase:** Linux build, ASIO support on Windows, plugin hosting (VST3/CLAP), DAW round-trip, real-time DJ mode.

---

## Spec coverage map

| Spec § | Requirement | Module(s) |
|---|---|---|
| §4.2 | Conversational mix engineer walkthrough | M37, M40 |
| §7 | `apply_eq` | M29 |
| §7 | `apply_compressor` | M30 |
| §7 | `apply_reverb` | M31 |
| §7 | `apply_limiter`, `apply_de_esser`, `apply_filter`, `apply_saturation` | M32 |
| §7 | `set_track_gain`, `set_track_pan`, `add_send`, `set_bus_routing` | M33 |
| §7 | `mix_for_streaming`, `master_for_genre`, `cleanup_voice` | M34 |
| §3 | Hosted subscription AI path | M35 |
| §3 | Local LLM via Ollama | M36 |
| §3 | Engine as remote MCP for Claude Desktop/Code | M38 |
| §8 | Mix-engineer-mode system prompt | M37 |

---

## File / crate structure (deltas vs. Phase 2)

```
crates/
├── dsp-effects/                # NEW (M29–M32) — parametric EQ, comp, reverb, limiter, etc.
│   └── src/{lib,eq,compressor,reverb,limiter,deesser,filter,saturation,common}.rs
├── audio-engine/
│   └── src/{bus,master_chain}.rs   # NEW (M33) — routing
├── tools/
│   └── src/tool/
│       ├── apply_eq.rs, apply_compressor.rs, apply_reverb.rs, apply_limiter.rs, ...
│       ├── set_bus_routing.rs, add_send.rs, set_track_pan.rs
│       └── mix_for_streaming.rs, master_for_genre.rs, cleanup_voice.rs   # M34
└── ai-providers/
    ├── src/{lib,anthropic,proxy,ollama}.rs    # MODIFY (M35, M36) — split P1 ai/anthropic into providers
    └── (replaces direct anthropic in P1 crates/ai)

apps/
├── proxy-backend/              # NEW (M35) — axum service for hosted subscription
│   └── src/{main,auth,billing,proxy_anthropic}.rs
└── mcp-server/                 # NEW (M38) — embedded localhost MCP, also standalone binary
    └── src/{main,transport,bridge_to_dispatcher}.rs
```

---

## Modules

### M29 — Parametric EQ (`dsp-effects::eq`)

**Files:**
- Create: `crates/dsp-effects/Cargo.toml` (deps: `biquad = "0.5"`, `dasp = "0.11"`)
- Create: `crates/dsp-effects/src/{lib,eq,common}.rs`
- Create: `crates/tools/src/tool/apply_eq.rs`
- Create: `tests/golden/eq_low_shelf_+3db_200hz.wav`

**Public API:**
```rust
pub struct EqBand { pub kind: EqBandKind, pub frequency: f32, pub gain_db: f32, pub q: f32 }
pub enum EqBandKind { LowShelf, HighShelf, Bell, LowPass, HighPass, Notch }
pub fn apply_eq(input: &[f32], sample_rate: u32, bands: &[EqBand]) -> Vec<f32>;
```

**Tool spec:** `apply_eq`: `{track: usize, bands: [EqBand]}` → new node.

**Acceptance criteria:**
1. Pink noise through a +3dB bell at 1 kHz Q=1 → measured spectrum has +3dB peak at 1 kHz ± 0.2dB (FFT analysis).
2. Low-shelf +3dB at 200Hz on a known fixture matches the golden WAV byte-for-byte.
3. EQ is bypass-clean: 0 dB on all bands → output equals input bit-exactly.
4. Cross-platform deterministic.

**Test design:** FFT-based magnitude response check at 50 frequency points; golden WAV diff.

**Risk:** Low-medium. Biquad EQ is well-understood; the trap is q-factor convention inconsistency between sources.

**Estimate:** 0.5 weeks.

---

### M30 — Compressor (`dsp-effects::compressor`)

**Files:**
- Create: `crates/dsp-effects/src/compressor.rs`
- Create: `crates/tools/src/tool/apply_compressor.rs`

**Public API:**
```rust
pub struct CompressorParams {
    pub threshold_db: f32, pub ratio: f32,
    pub attack_ms: f32, pub release_ms: f32,
    pub knee_db: f32, pub makeup_gain_db: f32,
    pub sidechain: Option<Sidechain>, // for parallel-comp later
}
pub fn apply_compressor(input: &[f32], sample_rate: u32, params: &CompressorParams) -> Vec<f32>;
```

**Acceptance criteria:**
1. Steady tone at -6 dBFS through threshold=-12, ratio=4:1 → output at -12 + (-6-(-12))/4 = -10.5 dBFS ± 0.2.
2. Attack/release: a step from -∞ to 0 dBFS reaches the steady-state gain reduction within ~3 × attack_ms.
3. Soft-knee variant: at threshold ± knee/2, gain reduction is the analytical soft-knee curve within 0.5 dB.
4. Sidechain input correctly drives gain reduction independent of audio signal.

**Test design:** Steady-state tone tests for ratio; step-response tests for attack/release.

**Risk:** Medium. Compressor envelopes are subtle; "feels right" is non-trivial. Listening test required.

**Estimate:** 1 week.

---

### M31 — Reverb (`dsp-effects::reverb`)

**Files:**
- Create: `crates/dsp-effects/src/reverb.rs`
- Create: `crates/tools/src/tool/apply_reverb.rs`

**Implementation choice:** Algorithmic feedback delay network (FDN). Hand-rolled rather than via `fundsp` to keep parameters explicit. Stretch goal: convolution reverb with checked-in IR files.

**Public API:**
```rust
pub struct ReverbParams { pub size: f32 /* 0–1 */, pub decay_s: f32, pub damping: f32 /* 0–1 */, pub mix: f32 /* 0–1 wet */, pub pre_delay_ms: f32 }
pub fn apply_reverb(input: &[f32], sample_rate: u32, params: &ReverbParams) -> Vec<f32>;
```

**Acceptance criteria:**
1. Mix=0 → output equals input.
2. Mix=1 → measured RT60 within 10% of `decay_s` (impulse response analysis).
3. Damping increases high-frequency loss in tail (frequency-dependent decay verified on broadband impulse).
4. Cross-platform deterministic.

**Test design:** Impulse response capture; RT60 analysis; spectral analysis of late tail.

**Risk:** High aesthetically; medium technically. Algorithmic reverb that *sounds good* is harder than one that's mathematically correct. Plan a listening session with reference reverbs (Valhalla, FabFilter) to set the bar.

**Estimate:** 1.5 weeks.

---

### M32 — Limiter, de-esser, filter, saturation

**Files:**
- Create: `crates/dsp-effects/src/{limiter,deesser,filter,saturation}.rs`
- Create: `crates/tools/src/tool/{apply_limiter,apply_de_esser,apply_filter,apply_saturation}.rs`

**Specs:**
- **Limiter:** look-ahead (5-15ms), brick-wall ceiling. Param: `ceiling_db, release_ms, lookahead_ms`. Goal: zero clipping at ceiling.
- **De-esser:** sidechain compressor on a band-passed copy (4–10 kHz). Param: `threshold_db, ratio, frequency_hz, q`.
- **Filter:** simple low-pass / high-pass / band-pass biquad. Already partially in M29; expose as standalone tool for explicit use.
- **Saturation:** soft-clip + tape-style harmonic distortion. Param: `drive_db, character ("warm" | "bright" | "tape"), mix`.

**Acceptance criteria (collective):**
1. Limiter: input swept from -20 to +6 dBFS → output never exceeds ceiling (verified with peak detection over 1M samples).
2. De-esser: sibilant fixture (clean female vocal) → measured 4–10 kHz energy reduced by ≥ 6 dB; rest of spectrum within ±1 dB.
3. Saturation drive=0 → bypass-clean.
4. All effects deterministic across Mac and Windows.

**Risk:** Medium. Saturation models vary widely; pick simple and well-defined over fancy.

**Estimate:** 1.5 weeks (collective).

---

### M33 — Bus routing & master chain

**Files:**
- Modify: `crates/audio-engine/src/{graph,mixer,bus,master_chain}.rs`
- Modify: `crates/session/src/state.rs` — `BusGraph` struct already in P1 schema; promote to actually used.
- Create: `crates/tools/src/tool/{set_track_pan,add_send,set_bus_routing,set_master_chain}.rs`

**Behaviors:**
- Sends: a track can route a percentage to a bus (e.g., "snare → 30% → reverb_bus → master").
- Bus chain: each bus has its own effects chain.
- Master chain: terminal effects applied to the final mixdown.
- Pan: equal-power pan law.

**Acceptance criteria:**
1. Track pan = 100% L → output stereo file: left channel matches input × 1.0, right channel = 0.
2. Send to a reverb bus: tone fed through a 0.5-send to a wet-only reverb returns the dry tone + reverb tail (verified by spectral analysis).
3. Master limiter applied: total output ceiling at -1 dBTP regardless of channel sums.
4. Determinism preserved: send order is deterministic.

**Test design:** Equal-power pan correctness; send vs. insert behavioral test; render-determinism test on a 4-track session with sends.

**Risk:** Medium. Bus routing is the place where graph correctness can subtly break (cycles, double-counting). Add a graph-validation pass.

**Estimate:** 1.5 weeks.

---

### M34 — Mix pipelines (composite tools)

**Files:**
- Create: `crates/tools/src/tool/{mix_for_streaming,master_for_genre,cleanup_voice}.rs`
- These compose lower-level effect tools using the agent's own tool-calling — but as a *composite* tool, the call chain is fixed and deterministic.

**Tool specs:**
- `mix_for_streaming`: `{node_id, target_lufs: f32 (default -14)}` → applies HPF on bass tracks, light bus comp, master limiter, ends at integrated LUFS == target ± 0.5.
- `master_for_genre`: `{node_id, genre: "indie_pop" | "hip_hop" | "edm" | "podcast"}` → genre-specific master chain.
- `cleanup_voice`: `{node_id, track: usize}` → HPF @ 80 Hz, de-ess, gate, leveler, normalize.

**Acceptance criteria:**
1. `mix_for_streaming` on a reference rough mix produces output measured at exactly the target LUFS ± 0.5 LU (use `ebur128` validator).
2. `cleanup_voice` on a noisy podcast clip subjectively (and by SNR measurement) cleans it; 8/10 listening-test panel agrees it's better than source.
3. Each pipeline is a single tool call in the agent's view but produces N session nodes (one per intermediate state) so the user can step backwards.

**Risk:** Medium. Composite tools are easy to over-engineer; keep them simple and document defaults.

**Estimate:** 1 week.

---

### M35 — Hosted subscription AI path (proxy backend + Stripe + auth)

**Files:**
- Create: `apps/proxy-backend/Cargo.toml` (deps: `axum`, `tower`, `sqlx` (sqlite for v1), `stripe-rust`, `jsonwebtoken`, `serde`)
- Create: `apps/proxy-backend/src/{main,auth,billing,proxy_anthropic,db}.rs`
- Create: `apps/proxy-backend/migrations/001_initial.sql` — users, subscriptions, usage tables.
- Create: `crates/ai-providers/src/proxy.rs` — client side; sends to our proxy URL with our JWT, gets streamed Anthropic responses back.
- Modify: `apps/desktop/src/components/Settings.tsx` — add "Account" tab with sign-in / subscribe.

**Architecture:**
- Backend = single small `axum` service deployed to Fly.io or similar. SQLite for v1; postgres later.
- Auth: email + magic link (no passwords). JWT issued on click, stored in OS keychain on desktop.
- Billing: Stripe Checkout for subscription; webhook updates subscription status in DB.
- Proxy: forwards streaming `messages` requests to Anthropic API with our key, accounts for usage, blocks if subscription expired or quota exceeded.
- Quota: per-month hard cap on tokens; soft warning at 80%.

**Acceptance criteria:**
1. End-to-end: sign up via desktop → receive magic-link email → click → desktop logged in. Test on Mac + Windows.
2. Subscribe via Stripe Checkout test mode → backend marks active → proxy starts forwarding requests.
3. Cancelled subscription → proxy returns 402 within 24 hours; desktop falls back gracefully ("Subscription paused, switch to BYO key?").
4. Streaming response from proxy is byte-equivalent to streaming from Anthropic direct (modulo our injected request-id header).

**Risk:** **High** — this is the only module that requires external infra and ongoing operations. Solo dev has limited bandwidth for backend ops. Mitigation: keep backend as small and stateless as possible; pick a managed sqlite (or skip persistence beyond Stripe's source of truth).

**Estimate:** 2 weeks. The longest single module of Phase 3.

---

### M36 — Local LLM path (Ollama / OpenAI-compatible)

**Files:**
- Create: `crates/ai-providers/src/ollama.rs`
- Modify: `crates/ai/src/loop.rs` — provider trait abstraction so the loop is provider-agnostic.
- Modify: `apps/desktop/src/components/Settings.tsx` — Provider tab: BYO Anthropic | Hosted | Local Ollama.

**Tool surface for local mode:** simplified subset (~12 tools): load, transcribe, cut_range, trim, gain, normalize, separate_stems, analyze_track, time_stretch, pitch_shift, render_preview, render_final. Effects and bus routing are NOT exposed in local mode (tool-call reliability limits).

**Acceptance criteria:**
1. With `ollama serve` running locally and `qwen2.5-coder:32b` model pulled, a turn `"normalize to -1 dBFS"` correctly invokes `normalize` (smoke test).
2. Provider switching is hot — no app restart needed.
3. Documentation: explicit "what works in local mode vs. hosted mode" table in user guide.
4. UX: when local mode fails to tool-call, the agent surfaces a clean error: "Local model couldn't pick a tool — try hosted mode for this prompt?".

**Risk:** Medium-high. Local LLM tool-calling reliability is volatile across model versions and sizes. Document the gap; don't promise parity.

**Estimate:** 1 week.

---

### M37 — Mix-engineer-mode system prompt

**Files:**
- Create: `crates/ai/prompts/mix_engineer_mode.md`
- Modify: `crates/ai/src/loop.rs` — mode classifier already exists from P2 M27; add `mix` route.

**Behaviors (per spec §8):**
- Tighter loops: emit small (1–3 tool) batches rather than full plans up front.
- Always offer to **fork** rather than overwrite when the user requests a change.
- A/B preview is the default after every meaningful change.
- Reasoning trace: agent explains *why* it picked a setting (e.g. "snare reduction by ratio 4:1 → 2.5:1 because you said 'too crunchy', which is usually parallel comp pushing snare snap").

**Acceptance criteria:**
1. On the §4.2 walkthrough prompts, the agent forks rather than overwrites on every refinement turn.
2. A/B compare is offered on every node-creating tool-use.
3. Reasoning trace appears before each fork.
4. Mode classifier accuracy on 30 hand-labeled mix prompts ≥ 27/30.

**Estimate:** 1 week.

---

### M38 — Localhost MCP server (`apps/mcp-server`)

**Files:**
- Create: `apps/mcp-server/Cargo.toml` (deps: `axum` for SSE, `tower`, `tools`, `session`, `audio-engine`)
- Create: `apps/mcp-server/src/{main,transport,bridge_to_dispatcher}.rs`
- Decision: also embed it as a feature in `apps/desktop` so the desktop app exposes the same surface to localhost when enabled in Settings. Standalone binary is for headless / CI / Claude Desktop integration where the GUI isn't running.

**MCP surface:** Same dispatcher as desktop. Auth: bearer token written to a per-user file at `~/.config/edytlab/mcp-token` (mode 0600 on POSIX), or per-launch if user prefers ephemeral. CORS off — localhost only.

**Acceptance criteria:**
1. `curl -H "Authorization: Bearer $TOKEN" http://localhost:NNNN/mcp/v1/tools` lists tools matching the desktop's catalog.
2. Claude Desktop config snippet (`~/Library/Application Support/Claude/claude_desktop_config.json`) example shipped in docs.
3. Tool invocations through MCP create the same session nodes as through the desktop chat (verified by inspecting the project store after a sequence of MCP calls).
4. Bearer token rotation supported (regenerate from Settings).

**Risk:** Medium. MCP spec is evolving; pick a stable revision and document the version we target.

**Estimate:** 1 week.

---

### M39 — Polish: telemetry (opt-in), crash reports, autoupdate

**Files:**
- Create: `crates/telemetry/Cargo.toml` (deps: chosen vendor — see below)
- Modify: `apps/desktop/src/components/Settings.tsx` — opt-in toggle for telemetry
- Modify: signing pipelines to publish update manifests for Tauri's auto-updater.

**Decisions to lock during M39:**
- Telemetry vendor: leaning self-hosted Plausible or PostHog (privacy-first), with explicit user opt-in.
- Crash reports: Sentry or self-hosted GlitchTip.
- Autoupdate: Tauri's built-in updater pointing at S3-served manifest.

**Acceptance criteria:**
1. Telemetry off by default; toggling on sends a confirming event; toggling off ceases all transmission within 30 sec.
2. Crash reproduces (forced panic) → report appears in Sentry/GlitchTip dashboard.
3. Autoupdate from `0.x.y-1` to `0.x.y` succeeds on Mac and Windows; old install replaced atomically.

**Risk:** Low-medium. Vendor choice (#7 in spec open questions) blocks final shipping but not module work; can pick last.

**Estimate:** 1 week.

---

### M40 — Lighthouse Demo C end-to-end + screen recording

**Files:**
- Create: `tests/e2e/mix_engineer_walkthrough.rs`
- Create: `tests/fixtures/band_recording_12_stems/` — license-clean multi-stem recording (~12 stems × 90 sec).
- Create: `assets/demos/phase3-mix-engineer.mp4` — 2-min screen recording.

**Demo script (matches §4.2):**
1. Drop folder of 12 stems.
2. Type spec's prompt: "Rough mix this. Modern indie pop. Vocal upfront."
3. Approve plan.
4. Refine "drums too crunchy" → fork two alternatives.
5. Refine "halfway between those + warm up the vocal" → synthesize a third branch.
6. Accept final → export.

**Acceptance criteria:**
1. Sequence completes in ≤ 6 minutes wall clock on Mac, ≤ 15 min on Windows.
2. Final mix subjectively passes a 5-listener panel ("acceptable rough mix" 4/5).
3. 2-minute screen recording produced.
4. Same demo runs through localhost MCP (M38) — same node sequence created in the project store.

**Risk:** This is the v1 launch gate. Full integration of all phases. Budget 2 weeks for surprises.

**Estimate:** 2 weeks.

---

## Phase 3 schedule (10 weeks)

| Wk | Modules in flight |
|---|---|
| 1 | M29, M30 start |
| 2 | M30 finish, M31 |
| 3 | M31 finish, M32 |
| 4 | M33 |
| 5 | M34, M35 start |
| 6 | M35 finish |
| 7 | M36, M37 |
| 8 | M38, M39 |
| 9 | M40 (start) |
| 10 | M40 finish, launch prep |

Critical path: M35 (proxy backend) is the single longest module and the only one with non-code dependencies (Stripe, hosting). Start it in week 5 in parallel with M34 to keep slack.

## Risks & mitigations (Phase 3)

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| DSP effects fail aesthetic listening tests vs. commercial plugins | Medium | High | Per-effect listening sessions with reference plugins. If reverb (M31) fails, swap algorithmic for convolution + 5 checked-in IRs as escape hatch. |
| Backend operations burden on solo dev (M35) | High | Medium | Keep backend stateless and minimal. Stripe is source of truth. SQLite + Litestream for cheap durability. Document an explicit pause-the-service runbook for vacations. |
| Local LLM tool-calling regresses with new Ollama versions | Medium | Low | Pin to a known-good model + Ollama version in our docs; user choice to upgrade. |
| MCP spec moves under us | Medium | Low | Pin a version; bump deliberately. |
| Composite mix pipelines (M34) are perceived as "too automated/canned" | Medium | Medium | Surface intermediate session nodes so users can dial back specific steps. |
| DSP cross-platform determinism breaks at scale | Medium | High | Extend P1 cross-platform render gate to cover all 7 effects. |

## Open questions deferred to execution

- **Telemetry vendor + crash reporter:** lock by start of M39.
- **Default genre presets in `master_for_genre`:** start with `indie_pop`, `hip_hop`, `edm`, `podcast`; add others based on early-user feedback.
- **Pricing for hosted tier:** $15-25/mo range from spec. Lock 2 weeks before public launch.
- **Localhost MCP token model:** persistent file vs. per-launch. Default to persistent file, mode 0600.
- **Convolution reverb vs. algorithmic:** committed to algorithmic for v1; convolution may be added Phase 3.5.
- **Effect chain order:** EQ → Comp → Reverb → Limiter is conventional; expose as user-reorderable per track.

## Self-review checklist run

- [x] Spec coverage: every Phase-3 spec section maps to a module.
- [x] Composite tools (M34) come after primitive effects (M29–M33) — order is right.
- [x] Local LLM (M36) has a clear documented surface gap from hosted (~12 tools subset).
- [x] MCP server (M38) reuses the dispatcher — no duplication of tool implementations.
- [x] No "TBD/TODO" placeholders in module bodies.
- [x] Type and tool names consistent with Phase 1 + 2.
- [x] Critical path identified (M35 backend); started early in schedule.

## Execution handoff

Plan saved to `docs/superpowers/plans/2026-05-05-phase-3-mix-engineer.md`. Phase 2 must ship before this plan is executed; on Phase 2 GA, **re-review this plan** (likely 1-day refresh) before starting M29.

After Phase 3 ships, v1 is feature-complete. Post-v1 backlog (per spec §9): Linux build, ASIO support, note-level editing, plugin hosting, DAW round-trip, real-time DJ mode, mobile companion. Prioritize after v1 user feedback.
