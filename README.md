# edytlab

**Talk to Claude. Get pro-grade audio edits.**

A Tauri 2 desktop audio editor where producers, podcasters, and DJs do real audio editing by chatting with an AI agent. Drop two MP3s, ask for a mashup, get a rendered WAV. Drop stems, refine the mix through conversation with audible A/B at every turn.

Local-first, multi-provider, pure-Rust DSP. Mac and Windows in v1.

## Website and docs

- Website: <https://edytlab.app> (placeholder — DNS not yet provisioned; see `/website` for the marketing site source)
- Design spec: [`docs/specs/2026-05-05-conversational-audio-editor-design.md`](docs/specs/2026-05-05-conversational-audio-editor-design.md)
- Documentation index: [`docs/README.md`](docs/README.md)

## Key features

- **Conversational multi-track production.** Say *"mashup A's vocals over B's drums, key-match, give me 3 takes on the drop"* and the agent plans, executes, and renders. Branches per take, A/B in the canvas.
- **Pure-Rust DSP graph.** Decode (`symphonia`), routing/effects (`fundsp` / `dasp`), resampling (`rubato`), I/O (`cpal`). DSP quality is non-negotiable — no Python in the hot path.
- **Local-first.** Audio never leaves your machine unless you export. The agent talks to a remote LLM; the audio engine runs entirely in-process.
- **Multi-provider LLMs out of the box.** Anthropic, OpenRouter, and OpenAI — each with its own key in the OS keychain, switchable from the Settings panel without reinstall. Adding a fourth is a single `LlmProvider` impl.
- **Branchable session graph.** Every state is a node in a DAG. Fork, name, compare, revert — A/B is first-class, not an undo stack. *(Linear timeline shipping in Phase 1; full DAG view lands in Phase 2.)*
- **ML primitives where they matter.** Demucs (stem separation) and Whisper (transcription) integrated as ONNX-driven tools the agent can call. Rubber Band for time-stretch / pitch-shift in Phase 2.

## Quick start

Prerequisites:

- Rust toolchain pinned by `rust-toolchain.toml` (currently 1.88, installs automatically via `rustup`)
- Node 20+ and `pnpm` 9.15+
- Platform Tauri prerequisites: see <https://tauri.app/start/prerequisites/> (Xcode CLT on macOS, MSVC build tools + WebView2 on Windows)

```bash
git clone https://github.com/laadtushar/edytlab.git
cd edytlab
pnpm install
pnpm tauri:dev          # equivalent to: pnpm --filter @edytlab/desktop tauri dev
```

The first build is slow — `cargo` compiles the full Rust workspace plus Tauri. Subsequent runs are incremental.

## AI provider setup

edytlab supports three providers out of the box. Each provider has its own API key slot in the OS keychain, and you can switch the active provider from the gear icon in the chat header without restarting.

| Provider | Get a key |
|---|---|
| Anthropic | <https://console.anthropic.com/settings/keys> |
| OpenRouter | <https://openrouter.ai/keys> |
| OpenAI | <https://platform.openai.com/api-keys> |

Keys are stored via the [`keyring`](https://crates.io/crates/keyring) crate — Keychain on macOS, Credential Manager on Windows. The keychain entry is namespaced per provider (`anthropic_api_key`, `openrouter_api_key`, `openai_api_key`); the active provider is mirrored in `active_provider`. A legacy unsuffixed `anthropic_api_key` slot is read for back-compat and migrated on first run.

The model picker is a combo (free-form input + curated suggestions from the live catalogue) so a brand-new model id works the moment you know it.

## Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│  Tauri shell (React 19 + Vite + Tailwind)                        │
│   Canvas (waveform)  ·  Chat panel  ·  Graph view  ·  Settings   │
└──────────────────────────────────────────────────────────────────┘
                       │ tauri::command + SSE events
┌──────────────────────────────────────────────────────────────────┐
│  Rust core (cargo workspace)                                     │
│   crates/ai          — LlmProvider trait, agent loop, keychain   │
│   crates/tools       — ~20 deterministic tool impls + dispatcher │
│   crates/session     — DAG of session states, fork/diff/compare  │
│   crates/audio-*     — decode, engine, I/O, time-domain ops      │
│   crates/ml-*        — Demucs, Whisper, ONNX pipeline            │
└──────────────────────────────────────────────────────────────────┘
                       │
┌──────────────────────────────────────────────────────────────────┐
│  LLM providers (pluggable at runtime)                            │
│   Anthropic  ·  OpenRouter  ·  OpenAI                            │
└──────────────────────────────────────────────────────────────────┘
```

Single user turn: chat message → `crates/ai` agent loop → tool calls dispatched in `crates/tools` → DSP / ML in the audio engine → SSE events stream back to the canvas. The `LlmProvider` trait in `crates/ai/src/provider.rs` owns auth, endpoint path, request serialization, and stream parsing — that's the extension point for new providers.

For the long version, read [`docs/specs/2026-05-05-conversational-audio-editor-design.md`](docs/specs/2026-05-05-conversational-audio-editor-design.md) §5.

## Tech stack

- **Shell:** Tauri 2, Rust workspace (edition 2021, toolchain 1.88), `cargo` profile-release with `lto = true`
- **Frontend:** React 19, Vite 7, Tailwind 4, `@xyflow/react` for the graph view, `wavesurfer.js` for waveforms
- **Audio:** `cpal`, `symphonia`, `dasp`, `fundsp`, `rubato`
- **ML:** ONNX Runtime via `ort` for Demucs / Whisper
- **LLM:** `reqwest` + `eventsource-stream` for SSE; `keyring` for OS-keychain credential storage
- **Test:** `cargo test` (unit + integration), `vitest` (frontend), `wiremock` for HTTP fakes
- **Tooling:** pnpm 9 workspace, conventional commits

## Project structure

```
apps/
  desktop/            Tauri shell — React frontend (src/) + Rust bridge (src-tauri/)
  cli/                Headless CLI for batch audio operations and smoke tests
crates/
  ai/                 LLM provider abstraction, agent loop, keychain, prompt cache
  tools/              ~20 audio-editing tools (load, cut, gain, transcribe, render, …)
  session/            Session-graph data model, DAG storage, fork/diff/compare
  audio-decoder/      File decode (symphonia)
  audio-engine/       DSP graph + render
  audio-io/           cpal playback / capture
  audio-time/         Time-stretch and pitch-shift primitives
  audio-analysis/     BPM, key, beat-grid, transients
  ml-demucs/          Stem separation
  ml-whisper/         Transcription
  ml-pipeline/        Shared ONNX runner + model cache
docs/                 Specs, handover, packaging notes
.github/workflows/    CI, auto-release, signed mac/win release pipelines
tests/                Cross-crate integration tests
```

## Development

The full acceptance gate (mirrors `CLAUDE.md`) — run before opening a PR:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm --filter @edytlab/desktop test
pnpm --filter @edytlab/desktop exec tsc --noEmit
```

Common one-shots:

```bash
pnpm tauri:dev                                       # run the desktop app in dev mode
pnpm --filter @edytlab/desktop test:watch            # vitest in watch mode
cargo test -p ai                                     # test a single crate
cargo run -p cli -- <args>                           # exercise the headless CLI
```

The Tauri bundle build is intentionally **not** in CI (too slow); release workflows cover that path.

## Releases

- **Auto dev releases.** `auto-release.yml` listens to `ci.yml`'s `workflow_run` on `main`. On green, it tags `v<version>-dev.<run_number>` and dispatches `release-dev.yml`, which builds **unsigned** mac + win bundles and attaches them to a draft GitHub Release. The workflow does not auto-publish.
- **Signed releases.** `release-mac.yml` (Apple notarization) and `release-win.yml` (Authenticode + DigiCert timestamp) are **manual** `workflow_dispatch` and gated on signing secrets being provisioned. See [`docs/packaging-windows.md`](docs/packaging-windows.md) for the Windows side, including the SmartScreen reputation note.
- Bundle targets are pinned in `tauri.conf.json` to `["app", "dmg", "msi", "nsis", "deb", "appimage"]` — don't revert to `"all"`.

App version is canonical in `apps/desktop/src-tauri/tauri.conf.json`; `package.json` files mirror it.

## Contributing

Working-style rules, branch naming, conventional-commit prefixes, and acceptance gates are codified in [`CLAUDE.md`](CLAUDE.md). One concern per PR; open as draft, squash-merge once CI is green. No formal CONTRIBUTING wall — read `CLAUDE.md` and ship.

## Roadmap

v1 (in progress) ships in three phases — single-track edit, mashup, conversational mix engineer. Spec §9 has the breakdown.

Out-of-scope for v1, on the post-v1 roadmap:

- **v2 — backlog.** Real-time DJ performance mode (live decks, controllers, beat-jump). Note-level / RipX-style harmonic editing. VST3 / CLAP plugin hosting. DAW round-trip (OMF / AAF / Logic project export).
- **Deferred.** Linux build (deferred from v1; signing/distribution pipeline cost). ASIO support on Windows. Mobile companion. Multi-user collaboration. Cloud project sync.
- **Out.** Music *generation* (Suno / Udio territory) — edytlab edits and mixes existing audio.

## Configuration

- **API keys:** stored in the OS keychain via the `keyring` crate (per-provider slot). Never committed; never logged.
- **Active provider + model:** mirrored in `localStorage` under `edytlab.model.<provider>` and `active_provider` in the keychain.
- **Rust toolchain:** pinned via `rust-toolchain.toml`. CI uses the same pin.
- **Tauri config:** `apps/desktop/src-tauri/tauri.conf.json` — bundle targets, identifier, signing entitlements, WebView2 install mode.

## License

TODO — `LICENSE` not yet committed. License choice is one of the open questions in the design spec (§12); will land before public distribution.
