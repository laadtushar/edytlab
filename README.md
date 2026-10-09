# edytlab

**Talk to Claude. Get pro-grade audio edits.**

A Tauri 2 desktop audio editor where producers, podcasters, and DJs do real audio editing by chatting with an AI agent. Drop two MP3s, ask for a mashup, get a rendered WAV. Drop stems, refine the mix through conversation with audible A/B at every turn.

Local-first, multi-provider, pure-Rust DSP. Builds for macOS (universal), Windows and Linux.

## Website and docs

- Website: <https://edytlab.com> (source in `/website`)
- Design spec: [`docs/specs/2026-05-05-conversational-audio-editor-design.md`](docs/specs/2026-05-05-conversational-audio-editor-design.md)
- Documentation index: [`docs/README.md`](docs/README.md)

## Key features

- **Conversational multi-track production.** Say *"mashup A's vocals over B's drums, key-match, give me 3 takes on the drop"* and the agent plans, executes, and renders. Branches per take, A/B in the canvas.
- **Pure-Rust DSP.** Decode (`symphonia`), resampling (`rubato`), I/O (`cpal`), and effects written in-house in `crates/audio-dsp` — a crate with deliberately zero dependencies. DSP quality is non-negotiable and there is no Python in the hot path.
- **Local-first.** Audio never leaves your machine unless you export. The agent talks to the LLM you pick — a hosted one, or a local model through Ollama; the audio engine runs entirely in-process.
- **Multi-provider LLMs out of the box.** Anthropic, OpenRouter, OpenAI, Groq, Gemini and Ollama — each hosted provider with its own key in the OS keychain (Ollama needs none), switchable from the Settings panel without reinstall. Adding another is a single `LlmProvider` impl.
- **Branchable session graph.** Every state is a node in a DAG. Fork, name, compare, revert — A/B is first-class, not an undo stack. The Graph view draws the DAG next to the timeline.
- **ML tools, not shipped yet.** Demucs (stem separation) and Whisper (transcription) are wired in as tools the agent can call, but their model inference is not shipped: both return an error today ([#385](https://github.com/laadtushar/edytlab/issues/385), [#384](https://github.com/laadtushar/edytlab/issues/384)), and the runtime and model delivery they need is [#383](https://github.com/laadtushar/edytlab/issues/383). The transcript-driven edits (`cut_words`, `remove_fillers`, `duck_under_speech`) are built but need a transcript, so they wait on #384 too.
- **Time and pitch without a C dependency.** Time-stretch, pitch-shift, formant preservation and beat-grid warping run on a phase vocoder written here, on `realfft`. Rubber Band was the original plan and was dropped: it needs a different native package on each of the three CI targets, and a native dependency is the kind of thing that breaks every build at once. See the module docs in `crates/audio-time/src/vocoder.rs` for the trade that buys and what it costs.

## Demo videos

Screen recordings of the desktop app with Claude as the agent, each one a DJ job that ends in an exported file. They also play on the website: <https://edytlab.com/#demos>.

- [**Beatmatch and blend two tracks**](website/public/demos/dj-beatmatched-transition.mp4) (2:58, shown at 1.6× speed): the DJ asks for both tempos; the incoming track is time-stretched to match, started 8 bars before the outgoing one ends and crossfaded, with a low-pass filter on the outgoing track; the mix is compressed, limited, brought to −14 LUFS and exported as a WAV.
- [**Extend an intro for mixing**](website/public/demos/dj-extended-club-intro.mp4) (2:26, with sound; waits for Claude are sped up, every playback is in real time): the DJ asks for Solar Flare's tempo and bar length; the drums-only first 8 bars are repeated so the intro runs 16 bars; a high-pass filter goes on the new intro, the first 4 bars fade in, and the track is exported as a WAV. You hear the original intro, then the new one, then the drop at 0:31.

The list lives in [`website/lib/demos.ts`](website/lib/demos.ts); its test fails if a listed video or poster is missing from `website/public/demos/`.

## Quick start

Prerequisites:

- Rust toolchain pinned by `rust-toolchain.toml` (currently 1.88, installs automatically via `rustup`)
- Node 20+ and `pnpm` 9.15+
- Platform Tauri prerequisites: see <https://tauri.app/start/prerequisites/> (Xcode CLT on macOS, MSVC build tools + WebView2 on Windows, the WebKitGTK / GTK / ALSA dev packages on Linux — the exact `apt` list is in [`docs/development-guide.md`](docs/development-guide.md#linux-ubuntu-2204))

```bash
git clone https://github.com/laadtushar/edytlab.git
cd edytlab
pnpm install
pnpm tauri:dev          # equivalent to: pnpm --filter @edytlab/desktop tauri dev
```

The first build is slow — `cargo` compiles the full Rust workspace plus Tauri. Subsequent runs are incremental.

## AI provider setup

edytlab supports six providers out of the box. Each hosted provider has its own API key slot in the OS keychain (Ollama needs no key), and you can switch the active provider from the gear icon in the app header without restarting.

| Provider | Get a key |
|---|---|
| Anthropic | <https://console.anthropic.com/settings/keys> |
| OpenRouter | <https://openrouter.ai/keys> |
| OpenAI | <https://platform.openai.com/api-keys> |
| Groq | <https://console.groq.com/keys> |
| Gemini | <https://aistudio.google.com/apikey> |
| Ollama | none — runs locally, point it at your own daemon |

Keys are stored via the [`keyring`](https://crates.io/crates/keyring) crate — Keychain on macOS, Credential Manager on Windows, and on Linux the kernel keyring (`keyutils`, the crate's `linux-native` feature), which is in-memory and does not survive a reboot. The keychain entry is namespaced per provider (`<provider_id>_api_key`); the active provider is mirrored in `active_provider`. Builds from before multi-provider support stored the Anthropic key as `anthropic_api_key`, which already matches this naming, so it is read as-is — no migration is needed.

The model picker is a combo (free-form input + curated suggestions from the live catalogue) so a brand-new model id works the moment you know it.

## Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│  Tauri shell (React 19 + Vite + Tailwind)                        │
│   Canvas (waveform)  ·  Chat panel  ·  Graph view  ·  Settings   │
└──────────────────────────────────────────────────────────────────┘
                       │ tauri::command + Tauri events
┌──────────────────────────────────────────────────────────────────┐
│  Rust core (cargo workspace)                                     │
│   crates/ai          — LlmProvider trait, agent loop, keychain   │
│   crates/tools       — 93 deterministic tools + dispatcher       │
│   crates/session     — DAG of session states, fork/diff/compare  │
│   crates/audio-*     — decode, engine, I/O, time-domain ops      │
│   crates/ml-*        — Demucs, Whisper (inference not shipped)   │
└──────────────────────────────────────────────────────────────────┘
                       │
┌──────────────────────────────────────────────────────────────────┐
│  LLM providers (pluggable at runtime)                            │
│   Anthropic · OpenRouter · OpenAI · Groq · Gemini · Ollama       │
└──────────────────────────────────────────────────────────────────┘
```

Single user turn: chat message → `crates/ai` agent loop (reading the provider's SSE stream) → tool calls dispatched in `crates/tools` → DSP in the audio engine → Tauri events (`agent://…`, emitted from `apps/desktop/src-tauri/src/events.rs`) stream back to the chat and timeline. The `LlmProvider` trait in `crates/ai/src/provider.rs` owns auth, endpoint path, request serialization, and stream parsing — that's the extension point for new providers.

For the long version, read [`docs/specs/2026-05-05-conversational-audio-editor-design.md`](docs/specs/2026-05-05-conversational-audio-editor-design.md) §5.

## Tech stack

- **Shell:** Tauri 2, Rust workspace (edition 2021, toolchain 1.88), `cargo` profile-release with `lto = true`
- **Frontend:** React 19, Vite 7, Tailwind 4, `@xyflow/react` for the graph view, `wavesurfer.js` for waveforms
- **Audio:** `cpal` (I/O), `symphonia` (decode), `rubato` (resampling), `realfft` (spectral), `hound` (WAV), `flac-codec` and `rusty_mp3` (export), plus in-house effects in `crates/audio-dsp`
- **ML:** `ort` (ONNX Runtime, `load-dynamic`) for Demucs / Whisper — the runtime library and models are not shipped and both decoders are stubs ([#383](https://github.com/laadtushar/edytlab/issues/383))
- **LLM:** `reqwest` + `eventsource-stream` for SSE; `keyring` for OS-keychain credential storage
- **Test:** `cargo test` (unit + integration), `vitest` (frontend), Playwright (`apps/desktop/e2e/`, the frontend in Chromium with the IPC boundary faked), `wiremock` for HTTP fakes
- **Tooling:** pnpm 9 workspace, conventional commits

## Project structure

```
apps/
  desktop/            Tauri shell — React frontend (src/) + Rust bridge (src-tauri/)
  cli/                Headless one-turn agent driver (edytlab-cli) for E2E tests and smoke runs
crates/
  ai/                 LLM provider abstraction, agent loop, keychain, prompt cache
  tools/              93 audio-editing tools (load, cut, gain, transcribe, render, …)
  session/            Session-graph data model, DAG storage, fork/diff/compare
  audio-decoder/      File decode (symphonia)
  audio-dsp/          Sample-level DSP shared by the tools and the renderer (no dependencies)
  audio-engine/       DSP graph + render
  audio-io/           cpal output stream — no caller in the app; playback runs in the webview (#388)
  audio-time/         Time-stretch and pitch-shift primitives
  audio-analysis/     BPM, key, beat-grid, transients
  recorder/           Microphone capture to WAV (cpal)
  memory/             Global and per-project markdown memory for the system prompt
  skills/             User skill library, appended to the prompt when a skill matches
  agent_profiles/     Saved model + tool-whitelist + prompt profiles
  mcp/                MCP server config, lifecycle and JSON-RPC
  ml-demucs/          Stem separation (inference is a stub, #385)
  ml-whisper/         Transcription (decoder is a stub, #384)
  ml-pipeline/        Shared ONNX runner + model cache (model download is a stub, #383)
docs/                 Specs, handover, packaging notes
.github/workflows/    CI, auto-release, unsigned dev/release and signed release pipelines
tests/                Shared fixture notes and golden-data placeholders
```

## Development

The full acceptance gate (mirrors `CLAUDE.md`) — run before opening a PR:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm --filter @edytlab/desktop test
pnpm --filter @edytlab/desktop test:slow-scheduler   # same suite, React's scheduler 40 ms late
pnpm --filter @edytlab/desktop typecheck      # tsc -b: app, tests and vite.config.ts
pnpm --filter @edytlab/desktop typecheck:e2e
pnpm --filter @edytlab/desktop test:e2e       # Playwright in Chromium; needs `playwright install chromium` once
```

Common one-shots:

```bash
pnpm tauri:dev                                       # run the desktop app in dev mode
pnpm --filter @edytlab/desktop test:watch            # vitest in watch mode
cargo test -p ai                                     # test a single crate
cargo run -p edytlab-cli -- --message "<text>"       # one agent turn, headless (key from ANTHROPIC_API_KEY)
```

The Tauri bundle build is intentionally **not** in CI (too slow); release workflows cover that path.

## Releases

- **Auto dev releases.** `auto-release.yml` listens to `ci.yml`'s `workflow_run` on `main`. On green, it tags `v<version>-dev.<run_number>` and dispatches `release-dev.yml`, which builds **unsigned** bundles — a universal macOS `.dmg`, Windows `.msi` + NSIS `.exe`, Linux `.deb` + AppImage — and attaches them to a GitHub Release. On a green matrix the `publish` job flips that release out of draft, so every green push to `main` publishes a dev prerelease; it is marked `prerelease` with `make_latest=false`, so it never takes the Latest badge from a real release. A draft is what a *failed* matrix leaves behind.
- **Versioned releases (unsigned for now).** `release-dev.yml` dispatched with `channel: release` publishes a real release — not a prerelease, marked Latest. v0.2.0 shipped this way.
- **Signed releases.** `release-signed.yml` builds all three platforms in one matrix — Apple notarization on mac, Authenticode + DigiCert timestamp on Windows, unsigned `.deb`/AppImage on Linux. **Manual** `workflow_dispatch`, gated on signing secrets being provisioned. They are not provisioned yet, so every release so far is unsigned ([#386](https://github.com/laadtushar/edytlab/issues/386)). It builds first and uploads only after signing, so the published assets are always the signed bytes. See [`docs/packaging-windows.md`](docs/packaging-windows.md) for the Windows side, including the SmartScreen reputation note.
- Bundle targets are pinned in `tauri.conf.json` to `["app", "dmg", "msi", "nsis", "deb", "appimage"]` — don't revert to `"all"`.

App version is canonical in `apps/desktop/src-tauri/tauri.conf.json`; the `package.json` files and the Cargo workspace's `[workspace.package] version` mirror it, and `apps/desktop/src/__tests__/appVersion.test.ts` fails if they drift.

## Contributing

Working-style rules, branch naming, conventional-commit prefixes, and acceptance gates are codified in [`CLAUDE.md`](CLAUDE.md). One concern per PR; open as draft, squash-merge once CI is green. No formal CONTRIBUTING wall — read `CLAUDE.md` and ship.

## Roadmap

v1 (in progress) ships in three phases — single-track edit, mashup, conversational mix engineer. Spec §9 has the breakdown.

Out-of-scope for v1, on the post-v1 roadmap:

- **v2 — backlog.** Real-time DJ performance mode (live decks, controllers, beat-jump). Note-level / RipX-style harmonic editing. VST3 / CLAP plugin hosting. DAW round-trip (OMF / AAF / Logic project export).
- **Deferred.** ASIO support on Windows. Mobile companion. Multi-user collaboration. Cloud project sync.
- **Out.** Music *generation* (Suno / Udio territory) — edytlab edits and mixes existing audio.

## Configuration

- **API keys:** stored in the OS keychain via the `keyring` crate (per-provider slot). Never committed; never logged.
- **Active provider + model:** stored in the keychain (`active_provider`, `<provider>_model`, plus `<provider>_base_url` when overridden) and mirrored in `localStorage` (`edytlab.provider`, `edytlab.model.<provider>`).
- **Rust toolchain:** pinned via `rust-toolchain.toml`. CI uses the same pin.
- **Tauri config:** `apps/desktop/src-tauri/tauri.conf.json` — bundle targets, identifier, signing entitlements, WebView2 install mode.

## License

MIT — see [`LICENSE`](LICENSE).
