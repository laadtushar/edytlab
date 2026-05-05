# Handover — Conversational Audio Editor

**For:** Starting a fresh Claude Code session in a new repository
**Date:** 2026-05-05
**Author:** Tushar (with Claude)

---

## How to use this doc

Open a new Claude Code session in the new repo and paste sections 1–4 as the kickoff prompt. The full design spec lives at `docs/specs/2026-05-05-conversational-audio-editor-design.md`.

---

## 1. Product one-pager

**What:** A desktop app (Tauri + Rust) where producers, podcasters, and DJs do real audio editing by chatting with Claude. Drop two MP3s, say *"mashup A's vocals over B's instrumental, key-match, beat-align, give me 3 takes on the drop"*, get a rendered file. Drop stems, refine through chat with audible A/B at every turn.

**Why it doesn't exist yet:** Pro DAWs (Logic, Pro Tools, RipX) require expert knowledge. AI tools (Descript Underlord, Adobe Podcast, Moises) are shallow — transcript shuffling, stem isolation, preset chains. Nobody offers conversational, multi-track, cross-song production at professional DSP quality. The agent layer is missing, not the technology.

**Lighthouse demos for v1:**
- **B. Mashup any two songs** — viral demo, TikTok-shaped.
- **C. Conversational mix engineer** — drop stems, multi-turn refinement with A/B at every turn. The depth/moat.

**Podcast/voice editing** ships as a supporting capability (Phase 1 *is* this), not the lighthouse.

## 2. Locked decisions (from brainstorm)

| # | Decision | Choice |
|---|---|---|
| 1 | Product structure | New repo, separate from Treacle |
| 2 | v1 scope | Music production primary; podcast as supporting |
| 3 | Form factor | Tauri (Rust) desktop. **v1 = Mac + Windows in parallel; Linux deferred to post-v1.** |
| 4 | Audio engine | Pure Rust DSP graph (`cpal`, `symphonia`, `dasp`/`fundsp`, `rubato`); ML via ONNX Runtime / `candle` sidecars |
| 5 | AI inference | Hybrid: BYO Anthropic key OR hosted subscription OR local LLM (Ollama). Switchable at runtime. |
| 6 | Session model | **Branchable mix graph** — every state is a DAG node. Fork/merge/A-B compare. Differentiator vs every competitor. |
| 7 | Distribution | Desktop app primary; engine *also* exposed as remote MCP for Claude Desktop/Code power users |
| 8 | Team | **Solo dev.** Phasing assumes one full-time engineer. |

## 3. What's next

Both blocking questions are resolved:
- **Platform priority:** Mac + Windows ship in parallel for v1 (the whole point of choosing Tauri). Linux deferred.
- **Team:** Solo. Phasing assumes one full-time engineer.

Next steps:

1. Produce a Phase 1 implementation plan from §9 of the design spec.
2. Stand up dual-platform CI (Mac + Windows) with signing pipelines as task #1 in Phase 1 — everything downstream assumes both platforms green.
3. **Don't write code** until the Phase 1 plan is approved.

Open questions remaining in §12 of the spec that can be punted to the plan: brand name, OSS vs proprietary, pricing tier details, stem-separation default model, librosa-rs vs Python sidecar, MCP auth model, telemetry vendor, distribution channel, Windows ML accel default.

## 4. Build phasing (recap)

Three milestones, each independently shippable.

- **Phase 1 — "Edit a single track"** (~9 weeks). Tauri shell, waveform canvas, basic chat, 8 tools (load, transcribe, cut_range, trim, gain, normalize, render_preview, render_final), linear session graph, BYO Claude key, **Mac + Windows from day one** (dual signing pipelines, WebView2 packaging, WASAPI on Windows). **This is podcast cleanup core in disguise.**
- **Phase 2 — "Mashup"** (~10 weeks). Demucs ONNX, BPM/key/beat analysis, time-stretch + pitch-shift (Rubber Band), multi-track session, branchable graph + A/B compare. Lighthouse B ships here.
- **Phase 3 — "Conversational mix engineer"** (~10 weeks). Pure-Rust EQ, compressor, reverb, limiter, de-esser, saturation; bus routing; mix pipelines; hosted subscription path; local LLM via Ollama; localhost MCP server. Lighthouse C ships here. (Linux deferred to post-v1.)

**Total v1 target: ~6.5-7 months solo dev (likely 8-9 realistic).**

## 5. Full design spec

The complete spec lives at:

```
docs/specs/2026-05-05-conversational-audio-editor-design.md
```

The spec covers:

1. Problem
2. Goals / Non-goals
3. Key decisions (locked in brainstorm)
4. Lighthouse user scenarios (mashup walkthrough + conversational mix walkthrough)
5. Architecture (component diagram, decomposition table, data-flow trace)
6. Session graph data model (Rust types + operations)
7. Tool surface (~30 tools, organized by category)
8. AI agent design (single-agent, three modes, prompt-caching strategy, local LLM caveats)
9. Build phases (3 phases, ~6 months)
10. Error handling & failure modes
11. Testing strategy (golden WAV diffs, property tests, listening tests gated)
12. Open questions (10)
13. Risks (8 ranked)
14. What's next

## 6. Suggested first prompt for new repo session

```
I'm starting a new product: a desktop app where producers chat with Claude
to do real audio editing — mashups, mixing, podcast cleanup. The full design
spec is in docs/specs/2026-05-05-conversational-audio-editor-design.md.

Read the spec end to end. Then:
1. Confirm you understand the locked decisions in §3.
2. Ask me to resolve the two blocking open questions
   (platform priority, solo/team) before any planning.
3. Once those are answered, invoke the writing-plans skill on Phase 1
   ("Edit a single track") from §9.

Do not write any code until I approve the Phase 1 plan.
```

## 7. Things explicitly NOT decided yet

- Product / brand name
- Open source vs proprietary split
- Pricing
- Stem separation model defaults (htdemucs vs htdemucs_ft)
- Music feature extraction lib (librosa Python sidecar vs pure Rust)
- MCP localhost auth model
- Telemetry vendor (privacy-first leans self-hosted)
- Distribution channel (direct vs Mac App Store vs Setapp vs Microsoft Store)
- Windows ML acceleration default (CPU-only, CUDA opt-in, or DirectML/WinML)

These belong in the Phase 1 / Phase 2 / Phase 3 implementation plans, not the design spec.
