# edytlab — Development Guide

> Complete guide to setting up, building, testing, and debugging edytlab. Covers every platform.

---

## Table of Contents

1. [Prerequisites](#1-prerequisites)
2. [Initial Setup](#2-initial-setup)
3. [Development Workflow](#3-development-workflow)
4. [Project Structure Deep Dive](#4-project-structure-deep-dive)
5. [Running Tests](#5-running-tests)
6. [Debugging](#6-debugging)
7. [Building for Release](#7-building-for-release)
8. [CI Pipeline](#8-ci-pipeline)
9. [Environment Variables](#9-environment-variables)
10. [Common Issues and Fixes](#10-common-issues-and-fixes)
11. [Acceptance Gate](#11-acceptance-gate)

---

## 1. Prerequisites

### All Platforms

| Tool | Version | Install |
|------|---------|---------|
| Rust | 1.88 (auto via `rust-toolchain.toml`) | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` |
| Node.js | 20+ | [nodejs.org](https://nodejs.org) or `nvm install 20` |
| pnpm | 9.15+ | `npm install -g pnpm` |
| Git | Any recent | |

### macOS

```bash
# Xcode Command Line Tools (required for linker, ar, etc.)
xcode-select --install

# Homebrew (recommended for dev tooling)
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
```

Minimum macOS version: **11.0 (Big Sur)**. The app targets Universal (arm64 + x86_64).

### Windows

1. **Visual C++ Build Tools**: Install via [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/). Select "Desktop development with C++".
2. **WebView2**: Bundled with Windows 11 and updated Windows 10. The installer bootstraps it if absent.

> **Note:** On Windows, always use PowerShell (not Git Bash / WSL bash) for `git`, `pnpm`, and `cargo` commands. The Bash tool in Claude Code cannot resolve Windows paths reliably.

### Linux (Ubuntu 22.04+)

```bash
sudo apt-get update
sudo apt-get install -y \
  libgtk-3-dev \
  libwebkit2gtk-4.1-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  libssl-dev \
  libasound2-dev \
  libdbus-1-dev \
  pkg-config \
  patchelf
```

`libdbus-1-dev` and `pkg-config` are for the keychain: on Linux `crates/ai` reaches the Secret Service over D-Bus (#394). This is the list `ci.yml` and the release workflows install — note `libayatana-appindicator3-dev`, not the older `libappindicator3-dev`. `patchelf` is only needed to bundle an AppImage; the release build also adds `libfuse2` for that.

### ML Model Files (Whisper/Demucs)

**Transcription works; stem separation does not.** What each needs:

- **Transcription** (`transcribe`, #384) runs the pinned Whisper export on
  the CPU. It needs two things on the machine, and the tool never fetches
  either:
  - **ONNX Runtime 1.23.2.** The release installers carry it; a build from
    source does not. Point `ORT_DYLIB_PATH` at a copy
    (`scripts/fetch-ml-runtime.sh <platform> <dir>` prints the path; see the
    table below). Without it the tool answers `[onnx_runtime_unavailable]`
    and the window shows a card that says so.
  - **The speech model**, about 139 MB. In the app the user presses
    **Download** in Settings → Account (from the release after v0.7.0). From
    source you can do the same, or set `WHISPER_MODEL_PATH` to a folder of the
    three files (below), which takes precedence. Without either the tool
    answers `[speech_model_not_downloaded]` and the window's card sends the
    user to Settings → Account. A `WHISPER_MODEL_PATH` that is set to
    something that is not that folder answers `[speech_model_path_invalid]`
    (it does not fall back to the downloaded model: you asked for that
    folder). A downloaded model that will not load answers
    `[speech_model_damaged]` (Remove it in Settings and download it again).
  Nothing downloads except the click: not at launch, not when Settings
  opens, not from a tool or the agent (`ml-pipeline`'s
  `only_the_user_downloads.rs` fails if another path could). Transcribing
  sends no audio anywhere and makes no request; the words it finds reach
  your AI provider like any tool result, as a bounded listing.
- `DemucsModel::separate` returns `NotImplemented`: **stem separation is a
  stub**, and no configuration makes it work.
- `ml_pipeline::fetched_model_path` downloads, hash-checks and caches a
  pinned model artifact (tested against a local HTTP server). One model is
  pinned, the speech model `whisper-base` (about 139 MB from
  huggingface.co, `crates/ml-pipeline/models.json`), and the app downloads
  it **only when you click Download** in Settings → Account (`Speech
  model`; from the release after v0.7.0), with a progress bar, Cancel (a resume with a `Range` request
  carries on from what arrived) and Remove. The `transcribe` tool finds the
  files there with `ml_pipeline::models::cached`, which hashes them (0.7 to 0.9 s in a release
  build; later calls skip the hashing while each file's size and modified time
  are as they were when it verified them). To fetch the real files from a shell, as
  a check of the pins:
  `cargo test -p ml-pipeline --test model_real_download -- --ignored --nocapture`.
- If a model variable below names a model but there is no ONNX Runtime
  library, `separate_stems` returns a tool error that says so (and that the
  feature is not implemented anyway), and `transcribe` returns the
  `[onnx_runtime_unavailable]` answer. They used to hang the agent.
  Installing the library does not make `separate_stems` work.

<!-- historical:begin -->
Earlier revisions of this guide described a `~/.edytlab/models` cache, an
automatic first-use download, a `scripts/fetch-models.sh` installer and
an `EDYTLAB_MODEL_DIR` override. None of those existed: the script was
never in the repository, the download path was never wired up, and
`EDYTLAB_MODEL_DIR` was read by no code in the tree. They are removed
rather than corrected, because the honest instruction at the time was "wait".
<!-- historical:end -->

`WHISPER_MODEL_PATH` and `DEMUCS_MODEL_PATH` *are* read. `WHISPER_MODEL_PATH`
is how a developer points `transcribe` at a speech model that is not the one
the app downloaded; setting `DEMUCS_MODEL_PATH` today only changes which
error `separate_stems` gives. `WHISPER_MODEL_PATH` names the speech model's
**folder**, the one that holds `onnx/encoder_model.onnx`,
`onnx/decoder_model_merged_int8.onnx` and `tokenizer.json` as
`crates/ml-pipeline/models.json` pins them (it used to name a single file);
`DEMUCS_MODEL_PATH` names a file. The ignored `ml-whisper` and `tools` tests
read the same variable. `htdemucs` is the one Demucs
model id, and the default, so `DEMUCS_MODEL_PATH` is the one Demucs
variable. `htdemucs_ft` and its `DEMUCS_FT_MODEL_PATH` are gone: it is a
bag of four models with no pinned ONNX export ([#385](https://github.com/laadtushar/edytlab/issues/385)).

#### Running the model tests locally

These are the tests CI cannot run, because they need the real speech model
and the real ONNX Runtime. They are `#[ignore]`d, so a plain `cargo test`
lists them as ignored, and asking for them with `--ignored` makes them
**fail** (not skip) if what they need is missing:

```bash
# ONNX Runtime 1.23.2, checked against apps/desktop/src-tauri/ml-runtime.json
export ORT_DYLIB_PATH="$(scripts/fetch-ml-runtime.sh linux-x64 /tmp/ml/ort)"
# the folder of the three whisper-base files (about 139 MB), each checked
# against the size and SHA-256 in crates/ml-pipeline/models.json; run again,
# it downloads nothing, and a file that has been damaged is downloaded again
export WHISPER_MODEL_PATH="$(scripts/fetch-ml-model.sh whisper-base /tmp/ml/whisper-base)"

# the words, their start times and the realtime factor (--release is much faster)
cargo test -p ml-whisper --release --test transcribe_known_words -- --ignored --nocapture --test-threads=1
# the `transcribe` tool end to end: the known words through a session, the
# cache, cut_words and remove_fillers on the real transcript, language, the
# model found the way the app finds it, and Cancel
cargo test -p tools --release --test transcribe_end_to_end -- --ignored --nocapture
# the tool's answers for a model that is not there or will not load
cargo test -p tools --test transcribe_load_errors -- --ignored
# the two ignored tests in ml-whisper's smoke file, which load the model
cargo test -p ml-whisper --release --test transcribe_smoke -- --ignored --nocapture --test-threads=1
# the downloader against huggingface.co (writes to a temporary folder of its own)
cargo test -p ml-pipeline --release --test model_real_download -- --ignored --nocapture --test-threads=1
```

`scripts/fetch-ml-model.sh` is for this and for CI: the app never runs it,
and nothing it ships downloads a model except the user's click on Download.
To run all of it on GitHub instead, dispatch the **ML** workflow
([Workflow: `ml.yml`](#workflow-mlyml-ml)).

The test audio, `crates/ml-whisper/tests/fixtures/speech_words.wav`, is
`espeak-ng` output (`make_speech.py` regenerates it, with the exact word
times in `speech_words.json`); see the header of
`tests/transcribe_known_words.rs` for what it does and does not show, and for
`speech_words_first_draft.wav`, the first list of words, which is measured
beside the one the test asserts on and not asserted on.

---

## 2. Initial Setup

```bash
# 1. Clone
git clone https://github.com/laadtushar/edytlab.git
cd edytlab

# 2. Install Node dependencies (all workspaces)
pnpm install

# 3. Verify the Rust toolchain is installed
# (rust-toolchain.toml is auto-applied by rustup)
rustup show

# 4. Build the Rust workspace to catch any compile errors early
cargo build --workspace

# 5. Start the desktop app in development mode
pnpm tauri:dev
```

The **first build takes 5–20 minutes** — Cargo compiles the full workspace plus Tauri dependencies. Subsequent builds are incremental and typically complete in under 30 seconds.

### Directory of the `pnpm tauri:dev` shortcut

`pnpm tauri:dev` is defined in the root `package.json` and expands to:
```bash
pnpm --filter @edytlab/desktop tauri dev
```

Which in turn runs the Tauri CLI (`@tauri-apps/cli`, the desktop package's `tauri` script) — `tauri dev`, which starts Vite and builds `apps/desktop/src-tauri/`.

### Setting up an API Key (required to use the agent)

1. Get a key from any supported provider:
   - Anthropic: https://console.anthropic.com/settings/keys
   - OpenRouter: https://openrouter.ai/keys
   - OpenAI: https://platform.openai.com/api-keys
   - Groq: https://console.groq.com/keys
   - Google Gemini: https://aistudio.google.com/apikey
   - Ollama: no key — run a local Ollama daemon (`ollama serve`, then `ollama pull llama3.2`)

2. In the running app, click the gear icon → pick the provider → enter your key. The key is stored in your OS keychain — never on disk or committed to git.

---

## 3. Development Workflow

### Daily Dev Loop

```bash
# Start the dev server with hot reload
pnpm tauri:dev

# In a separate terminal — watch frontend tests
pnpm --filter @edytlab/desktop test:watch

# In another terminal — watch Rust tests for a specific crate
cargo test -p ai -- --test-threads=1
```

### Branch Naming

```
feat/    → feature branches    → claude/feature/<short-kebab-summary>
fix/     → bug fix branches    → claude/fix/<short-kebab-summary>
```

Always branch off latest `origin/main`:
```bash
git fetch origin
git checkout -b claude/feature/my-feature origin/main
```

### Commit Convention

Commits follow [Conventional Commits](https://www.conventionalcommits.org/):

```
feat(ai): add OpenAI streaming state machine
fix(tools): normalize tool rejects NaN LUFS target  
refactor(session): extract diff logic to own module
docs: update architecture diagram for Phase 2
test(tools): add edge cases for cut_range bounds
ci: pin Node 20 in release matrix
```

Allowed prefixes: `feat`, `fix`, `ci`, `chore`, `docs`, `test`, `refactor`.

### Making a Frontend Change

1. Edit files under `apps/desktop/src/`
2. Vite HMR reloads the WebView automatically — no restart needed for UI changes
3. Run `pnpm --filter @edytlab/desktop typecheck` to typecheck (`tsc -b` over the app, its tests and `vite.config.ts` — bare `tsc --noEmit` checks nothing, because the root `tsconfig.json` is a solution file with no inputs of its own)

### Making a Rust Change

1. Edit files under `crates/` or `apps/desktop/src-tauri/`
2. `pnpm tauri:dev` will detect the change, recompile Rust, and restart the Tauri shell
3. Full recompile takes 5–30 seconds depending on the changed crate

### Adding a New Tauri Command

1. Add the function to `apps/desktop/src-tauri/src/commands.rs`
2. Add the `#[tauri::command]` attribute. If the command takes a lock a running tool can hold, it must not run on the main thread: make it an `async fn` (#421), and do the waiting inside `blocking(&state, |state| ..)` so it happens on Tokio's blocking pool and not on one of the async runtime's few workers (#498). `src-tauri/tests/main_thread_commands.rs` fails a synchronous command when its body, or the body of a function or method it calls (matched by name), does either of these:
   - names the `engine`, `dispatcher` or `clipboard` field of `AppState`, however the lock is then taken: `state.engine.lock()`, `lock_std(&state.engine, ..)`, `let e = &state.engine`
   - reaches the session store with `store_handle()` or `lock_std(&store, ..)`

   So a command that calls `run_track_tool`, `AppState::all_tool_names` or `apply_blacklist` fails too. The scan reads the `.rs` files directly in `src-tauri/src/`. It does not see a call through a closure or a trait object, `AppState` taken apart by a pattern (`let AppState { engine, .. } = ..`), or a function in another crate that locks for itself.

   The same test fails an `async` command that does either of those outside a `blocking(..)` or `spawn_blocking(..)` call: a `std` lock blocks the thread, so a command left waiting in its task holds a worker until the lock is free, and enough of them leave none to run any async command. The agent rebuild (`rebuild_agent` and the two it is made of) is exempt from that scan, because it takes no lock a tool holds, only clones the handles; but `rebuild_agent` waits on the agent's own `tokio` mutex, which `send_message` holds for a whole turn. A command that is in the session's line (see below) must therefore not call it: use `rebuild_agent_soon`, as `open_project` does, which rebuilds at once if no turn is running and otherwise leaves the rebuild to run when the turn ends. `tests/blocking_commands.rs` shows both effects: the runtime with one worker, and `open_project` answering while a turn holds the agent.

   Whatever edits the session also has to be sent in order by the frontend. Send it through the session's line (`inOrder` from `useSession`, `src/lib/sessionQueue.ts`), with its head adopted inside the turn, so two edits made in quick succession cannot apply or be answered the wrong way round (#498).
3. Register in `tauri::Builder::invoke_handler` in `lib.rs`
4. Add a matching TypeScript wrapper in `apps/desktop/src/lib/tauri-bridge.ts`
5. Update [API Reference](./api-reference.md) and the site's `website/app/docs/api-reference/page.tsx`, and give the fake backend (`apps/desktop/e2e/backend.ts`) an answer for it, taken from the command's own body

---

## 4. Project Structure Deep Dive

### `apps/desktop/src/` (Frontend)

```
src/
├── App.tsx                   # Root component, global keyboard handlers, state wiring
├── main.tsx                  # React root mount
├── components/
│   ├── ABCompareBar.tsx       # A/B compare mode controls
│   ├── AgentProfilesEditor.tsx
│   ├── CapabilitiesMenu.tsx   # + button for capabilities
│   ├── Chat.tsx               # Chat panel, message streaming
│   ├── EmptyState.tsx         # No-audio-loaded state
│   ├── ErrorBanner.tsx        # Error display with recovery CTA
│   ├── GraphView.tsx          # DAG visualization (@xyflow)
│   ├── LabelLane.tsx          # The lane labels are typed into, below the tracks
│   ├── MarkerLayer.tsx        # Timeline annotations overlay
│   ├── McpServersEditor.tsx
│   ├── MemoryEditor.tsx
│   ├── MessageBubble.tsx      # Single chat message + tool badges
│   ├── RenderWarningsNotice.tsx # Stored effects a render leaves out (ErrorBanner, warning tone)
│   ├── Ruler.tsx              # Timeline ruler (time markers)
│   ├── Settings.tsx           # Settings modal (all config)
│   ├── ShortcutsOverlay.tsx   # ? key shortcut display
│   ├── SkillsEditor.tsx
│   ├── ThinkingIndicator.tsx  # Agent working indicator
│   ├── Timeline.tsx           # WaveSurfer waveform + controls
│   └── ToolBadge.tsx          # Tool call badge in messages
├── lib/
│   ├── tauri-bridge.ts        # Type-safe IPC + event wrappers
│   ├── confirmAction.ts       # Confirm dialog that works in the packaged app (never window.confirm)
│   ├── file-open.ts           # Audio file picker + drag-drop
│   ├── graph.ts               # Graph layout (dagre), path steps for the graph
│   ├── headTrail.ts           # The path taken through history: undo/redo targets
│   ├── keychainNotice.ts      # The banner's words, for the launch banner and a Save that fell back
│   ├── ownedControls.ts       # A lane control the user holds stays theirs until the session confirms it (#596)
│   ├── renderWarnings.ts      # The words for a render's left-out effects
│   └── undoRedo.ts            # Undo/redo chords, text-entry check
└── __tests__/
    ├── App.undoRedo.test.ts
    ├── ShortcutsOverlay.test.tsx
    └── ...
```

### `apps/desktop/src-tauri/src/` (Rust Shell)

```
src/
├── bundled_skills.rs # Keeps the skills that ship in the app current in ~/.edytlab/skills/ (#574, #631)
├── commands.rs    # The #[tauri::command] functions (~100 of them)
├── compare.rs     # The open A/B compare and the previews it holds (#575)
├── events.rs      # agent://… event names and payloads
├── lib.rs         # Builder, command registration, startup
├── main.rs        # Entry point
├── mcp_tool.rs    # Wraps MCP server tools as `tools::Tool`
├── pre_manifest_skills.json # Hashes of the bundled skills shipped before the manifest; frozen
├── project.rs     # Project metadata, view state, recents, Save As's copy
├── reclaimer.rs   # Background cap on a project's derived audio
├── settings.rs    # ~/.edytlab/settings.json: Plan first, kept across a restart (#569)
├── state.rs       # AppState
└── window_mix.rs  # The mix the window plays and the preview-cache entry it holds (#607)
```

`AppState` (defined in `state.rs`; abridged — read the struct for the directory fields):
```rust
pub struct AppState {
    pub agent:                    Arc<tokio::sync::Mutex<Option<Agent>>>,
    pub dispatcher:               Arc<Mutex<ToolDispatcher>>,
    pub store:                    Arc<Mutex<Option<LiveStore>>>, // only `session`'s registry makes a LiveStore (#566)
    pub engine:                   Arc<Mutex<Engine>>,
    pub project_dir:              Arc<Mutex<Option<PathBuf>>>,
    pub api_key:                  Arc<Mutex<Option<String>>>,
    pub active_provider:          Arc<Mutex<String>>,
    pub active_model_by_provider: Arc<Mutex<HashMap<String, String>>>,
    pub plan_gate:                ai::PlanGate, // the plan card's one-answer wait (ADR 0002)
    pub selection:                Arc<Mutex<Option<Range>>>,
    pub clipboard:                Arc<Mutex<Option<tools::Clipboard>>>,
    pub memory:                   Arc<Mutex<Option<Arc<MemoryStore>>>>,
    pub skills:                   Arc<Mutex<SkillLibrary>>,
    pub agent_profiles:           Arc<Mutex<ProfileLibrary>>,
    pub mcp_config:               Arc<Mutex<McpConfig>>,
    pub mcp:                      Arc<McpRegistry>,
    // …
}
```

### `crates/ai/src/` (AI subsystem)

```
src/
├── lib.rs          # Agent, LlmConfig, AgentEvent, TurnResult
├── agent_loop.rs   # Tool-calling state machine
├── anthropic.rs    # Canonical message/stream types
├── approval.rs     # The approval gate shared by a plan and a held first edit (Plan first)
├── chatgpt/        # Sign in with ChatGPT: protocol rules, registration store, session (#474; the app does not call it yet)
├── context_window.rs  # "The model's context is too small" error (#395)
├── keychain.rs     # OS credential storage via keyring
├── models.rs       # Model catalogue + 10-min TTL cache
├── oauth/          # Generic OAuth 2.0 / OpenID Connect client: PKCE, loopback listener, OIDC checks, token endpoint (#474)
├── plan_gate.rs    # The plan gate: one oneshot per card, one answer, a timeout (ADR 0002)
├── prompt.rs       # System prompt construction
├── provider.rs     # LlmProvider trait + 6 implementations
├── responses.rs    # OpenAI Responses API codec (#474; no provider uses it yet)
├── session_context.rs  # Per-turn context: head, tracks, buses, master chain, selection, markers; written within a byte budget (#573)
├── tool_selection.rs   # The slim tool set sent to small-context models
└── validate.rs     # API key validation (1-token probe)
```

---

## 5. Running Tests

### Full Test Suite (CI-equivalent)

```bash
# Rust
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace -- --test-threads=1

# Frontend
pnpm --filter @edytlab/desktop test
pnpm --filter @edytlab/desktop test:slow-scheduler
pnpm --filter @edytlab/desktop typecheck

# Frontend in Chromium (Playwright); once: pnpm --filter @edytlab/desktop exec playwright install chromium
pnpm --filter @edytlab/desktop typecheck:e2e
pnpm --filter @edytlab/desktop test:e2e
```

> **Why `--test-threads=1`?** Some tests in `crates/ai` use a shared model cache that is not safe for concurrent test runs. The `--test-threads=1` flag serializes them.

### Targeted Test Runs

```bash
# Single crate
cargo test -p ai
cargo test -p tools
cargo test -p session

# Single test function
cargo test -p tools test_normalize_rejects_nan

# Frontend tests only
pnpm --filter @edytlab/desktop test

# Frontend in watch mode
pnpm --filter @edytlab/desktop test:watch

# TypeScript only
pnpm --filter @edytlab/desktop typecheck
```

### Test Organization

| Layer | Framework | Location |
|-------|-----------|---------|
| Rust unit tests | `cargo test` | `#[cfg(test)]` mod in each file |
| Rust integration tests | `cargo test` | `tests/` in each crate |
| Frontend unit tests | Vitest | `apps/desktop/src/__tests__/` |
| Frontend in a real browser | Playwright (Chromium), IPC faked by `e2e/backend.ts` | `apps/desktop/e2e/` |
| HTTP mock tests | `wiremock` | `crates/ai/tests/` |

### Writing New Tests

**Rust tools tests** — drive the tool through a real dispatcher over a temporary store, as `crates/tools/tests/tools_integration.rs` does (there is no shared test-helper module):
```rust
use serde_json::json;
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

#[test]
fn gain_needs_a_session() {
    let tmp = TempDir::new().unwrap();
    let mut store = session::Store::open(tmp.path()).unwrap();
    let mut engine = audio_engine::Engine::new();
    let mut clipboard: Option<tools::Clipboard> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
        allowed_tools: None,
    };
    let result = ToolDispatcher::default_dispatcher()
        .invoke("gain", json!({ "track": 0, "db": 3.0 }), &mut ctx)
        .unwrap();
    assert!(matches!(result, ToolResult::Error(_)));
}
```

`session::Store::open` is the raw open, which asks the registry of live stores nothing: it exists only in `session`'s test build and under its `raw-open` feature, which `tools`, `ai`, the CLI and the desktop crate enable in `[dev-dependencies]` only. A test over a temporary folder can use it; shipped code opens a project with `Store::open_shared` (the project the window has), `Store::open_private` (a folder of its own) or `Store::open_read_only`, and `crates/session/tests/no_unregistered_open.rs` fails a raw open outside test code (see [architecture.md](./architecture.md#off-lock-tools)).

**Frontend Vitest tests** — use `@testing-library/react`:
```typescript
import { render, screen } from "@testing-library/react";
import { ShortcutsOverlay } from "../components/ShortcutsOverlay";

test("renders shortcuts table when open", () => {
  render(<ShortcutsOverlay open onClose={() => {}} />);
  expect(screen.getByRole("dialog")).toBeInTheDocument();
  expect(screen.getByText("Play / Pause")).toBeInTheDocument();
});
```

---

## 6. Debugging

### Frontend Debugging

**DevTools in Tauri dev mode:**
Right-click anywhere in the window → "Inspect Element" (macOS/Linux) or press `F12` (Windows). Full Chrome DevTools available.

**Console logging:**
```typescript
console.log("debug:", value);  // Appears in DevTools console
```

**Tauri event debugging:**
```typescript
import { listen } from "@tauri-apps/api/event";
listen("*", (event) => console.log("tauri event:", event));
```

### Rust Debugging

**`tracing` crate** — the Rust crates emit `tracing` events, but neither the desktop app nor `edytlab-cli` installs a subscriber (there is no `tracing-subscriber`, `env_logger` or log plugin in the dependency tree), so those events are currently discarded and `RUST_LOG` has no effect. Until a subscriber is added, use `eprintln!` / `dbg!` for ad-hoc output; it appears in the terminal where `tauri:dev` is running.

**Breakpoints:**
Attach a native debugger (LLDB on macOS, WinDbg on Windows) to the running `edytlab` process. Or use `println!` / `dbg!` macros for quick inspection.

**Printing SessionState:**
```rust
// All core types implement Debug
println!("{:#?}", store.get(head_id)?);
```

### Diagnosing IPC Errors

All Tauri command errors surface as rejected Promises in TypeScript. The error message is the string from the Rust `Err(String)` return:

```typescript
try {
  await bridge.renderFinal(nodeId);
} catch (e) {
  console.error("render failed:", e);  // e is the Rust error string
}
```

On the Rust side, the `CmdResult<T>` alias means:
```rust
type CmdResult<T> = Result<T, String>;
// Any Err(_) here becomes a thrown JS error
```

### Common Debug Patterns

**WaveSurfer "No audio loaded" error:**
```typescript
// Guard before any zoom call
if (!wsRef.current || duration === 0) return;
wsRef.current.zoom(level);
```

**Undo/redo not working, or going to the wrong node:**
Undo and redo read the path the user took through `peekUndo()` / `peekRedo()` from `useSession`, never `node.parent` alone (a state reached again keeps its first parent, #398). Any code that moves the head must go through `setHeadLocal(id)` (an edit, an agent step, a graph click) or `resetHead(id)` (a different project); a head moved any other way is one the path does not know about. After a `set_head_to` that you made yourself, call `stepBack(from, to)` / `stepForward(from, to)`.

**Keyboard shortcut firing twice:**
Two `window.addEventListener("keydown")` handlers both firing. Add a guard:
```typescript
if (showShortcuts) return;  // Don't handle shortcuts when overlay is open
```

---

## 7. Building for Release

### Unsigned Dev Builds (automated)

Dev builds are created automatically by `auto-release.yml` on every push to `main` that passes CI. No manual steps needed. The workflow:

1. CI passes on `main`
2. `auto-release.yml` triggers, tags `v<version>-dev.<ci_run_number>`
3. Dispatches `release-dev.yml`
4. `release-dev.yml` builds unsigned bundles for macOS (universal), Windows and Linux (`.deb` + AppImage), each carrying the pinned ONNX Runtime (fetched by `scripts/fetch-ml-runtime.sh` and placed by the `tauri.ml-runtime.<os>.conf.json` overlay; on Windows with the Visual C++ runtime DLLs it imports)
5. Checks every installer with `scripts/installer-ml-runtime.py check`: the runtime and its notices are where the app looks, byte for byte, and the app binary from that installer, run with `--ml-runtime-status`, loads it and calls it bundled. Only then does it attach the same files to a GitHub Release, created as a draft
6. The `publish` job flips it out of draft once the whole matrix is green, so
   the prerelease is public without a manual step. It is marked `prerelease`
   with `make_latest=false`, so it never becomes the Latest release. A draft
   is what a *failed* matrix leaves behind.

To try the release build on a branch without releasing anything, dispatch
`release-dev.yml` on that branch with `dry_run: true` (the `tag` input is
required by the form but unused). Every leg builds and checks its installers
and keeps them as a workflow artifact for 5 days; no tag, release or upload is
made.

### Signed Mac Build

Requires Apple Developer Program membership + signing secrets in GitHub.

**Required secrets:**
- `APPLE_CERTIFICATE` (base64-encoded .p12)
- `APPLE_CERTIFICATE_PASSWORD`
- `APPLE_SIGNING_IDENTITY` (e.g., "Developer ID Application: Your Name (TEAMID)")
- `APPLE_TEAM_ID`
- `APPLE_ID` (for notarization)
- `APPLE_PASSWORD` (app-specific password)

Trigger `release-signed.yml` via GitHub Actions manual dispatch. None of these secrets (nor the Windows ones below) are provisioned yet, so every release so far is unsigned ([#386](https://github.com/laadtushar/edytlab/issues/386)). A versioned but unsigned release — every one from v0.2.0 to v0.7.0 — comes from dispatching `release-dev.yml` with `channel: release`.

### Signed Windows Build

Requires a code signing certificate + DigiCert timestamp authority.

**Required secrets:**
- `WINDOWS_CERTIFICATE` (base64-encoded .pfx)
- `WINDOWS_CERTIFICATE_PASSWORD`

See [`docs/packaging-windows.md`](./packaging-windows.md) for SmartScreen reputation details.

### Bundle Targets

Defined in `apps/desktop/src-tauri/tauri.conf.json`:

```json
"bundle": {
  "targets": ["app", "dmg", "msi", "nsis", "deb", "appimage"]
}
```

**Do not** set `"targets": "all"` — it silently drops installer formats under certain build configurations.

### Versioning

App version is **canonical** in `apps/desktop/src-tauri/tauri.conf.json`:
```json
"version": "0.7.0"
```

The other manifests mirror it, and `apps/desktop/src/__tests__/appVersion.test.ts` fails if any drifts. When bumping a version:
1. Update `tauri.conf.json`
2. Update the root `package.json`, `apps/desktop/package.json` and `website/package.json`
3. Update `[workspace.package] version` in the root `Cargo.toml` (every crate, the desktop one included, inherits it)
4. Add the entry to `website/app/changelog/page.tsx`
5. Commit as `chore(release): X.Y.Z`

---

## 8. CI Pipeline

### Workflow: `ci.yml`

**Trigger:** Push to `main` + all PRs.

**`build` job — matrix:** macOS 14 (arm64) · Windows latest (x86_64) · Ubuntu 22.04 (x86_64).

1. Install platform build deps (Linux only: GTK, WebKit, AppIndicator, libssl, libasound2)
2. Install Rust toolchain (from `rust-toolchain.toml`)
3. Cache cargo registry + `target/` (`Swatinem/rust-cache`, keyed per platform; saved from `main` only, so a PR restores `main`'s cache and never uploads its own, #610)
4. Install pnpm + Node 20
5. `pnpm install --frozen-lockfile`
6. `cargo fmt --all --check`
7. `cargo clippy --workspace --all-targets -- -D warnings`
8. `cargo test --workspace --no-fail-fast -- --test-threads=1`
9. Linux leg only: fetch the pinned ONNX Runtime (`scripts/fetch-ml-runtime.sh linux-x64`, cached by the hash of `apps/desktop/src-tauri/ml-runtime.json`) and run `ml-pipeline`'s ignored `cache_smoke` tests against it with `ORT_DYLIB_PATH` set (see [Environment Variables](#9-environment-variables))
10. `pnpm --filter @edytlab/desktop test`
11. `pnpm --filter @edytlab/desktop test:slow-scheduler` (Linux leg only)
12. `pnpm --filter @edytlab/desktop build` (`tsc -b` + `vite build`)

**`e2e (chromium)` job** (Ubuntu only): installs Chromium for Playwright, then `typecheck:e2e` and `test:e2e` against a production build of the frontend.

**`website (test)` job:** in `website/`, `pnpm install --frozen-lockfile --ignore-workspace`, `pnpm test`, `pnpm typecheck`. It does not run `pnpm build`; run that locally for a change to `website/`.

**Important:** CI concurrency is set to cancel in-flight PR runs but **never cancel main runs** — `auto-release.yml` waits on the `workflow_run` completion event.

### Workflow: `audit.yml` (Audit)

`cargo audit` scans `Cargo.lock` against the RustSec advisory database: weekly (Monday 06:23 UTC), on `workflow_dispatch`, and on PRs that change `Cargo.lock` or a `Cargo.toml`. A vulnerability fails it; an unmaintained, unsound or yanked crate only warns. There is no `ignore` list: clear a red run by bumping the crate (`cargo update -p <crate> --precise <version>`). The `cargo-audit` version is pinned in the workflow's `CARGO_AUDIT_VERSION`.

It is a workflow of its own, not a `ci.yml` job, and it does not run on push: `auto-release.yml` waits only on the run named "CI", so an advisory published on a quiet day can never turn that run red and skip a dev release. Do not make it a required check, since a PR its path filter skips would wait on it forever.

### Workflow: `ml.yml` (ML)

The tests that need the real speech model and the real ONNX Runtime, on
Linux, with `--ignored`, against the pins in
`apps/desktop/src-tauri/ml-runtime.json` and `crates/ml-pipeline/models.json`
(about 22 MB of runtime and 139 MB of model, both from third parties):

- `ml-whisper`: `transcribe_known_words`, and the two ignored tests of `transcribe_smoke`;
- `tools`: `transcribe_end_to_end` (the `transcribe` tool on the real model,
  through a session, #668) and `transcribe_load_errors` (needs only the runtime);
- `ml-pipeline`: `model_real_download` (the downloader against huggingface.co).

**Trigger:** `workflow_dispatch` only. It does not run on push, on a pull
request or on a schedule, so a run is always someone's choice. Dispatch it
from the Actions tab (ML, Run workflow, pick the branch) or with
`gh workflow run ml.yml --ref <branch>`, then `gh run watch`. (GitHub lists a
workflow for dispatch only once its file is on the default branch, so a change
to `ml.yml` itself can be tried on a branch only after a version of the file
has merged; to try the test steps on a branch before that, run the commands
in [Running the model tests locally](#running-the-model-tests-locally).)

**Caches:** the runtime and the model are cached under keys made from their
manifests (`hashFiles` of `ml-runtime.json` and of `models.json`), so changing
a pin misses. The two fetch steps run on a hit as well and check what was
restored against the manifest: a file whose size or SHA-256 is not the pinned
one is deleted and downloaded again, and that download is checked in turn. A
damaged cache therefore costs a download and does not fail the run; a download
that is not the pin fails the step, with nothing left under the file's name.
`crates/tools/tests/ml_workflow.rs` runs the model script against a damaged
folder to hold that.

It is a workflow of its own, not a `ci.yml` job, for the reason `audit.yml` is:
`auto-release.yml` waits on the run named "CI", and a failure here (a pin that
upstream removed) must not skip a release. Do not make it a required check.
`ml_workflow.rs` also keeps it dispatch-only, pins its actions to release tags
and checks that the tests it names exist and are ignored. No secrets are needed.

### Workflow: `auto-release.yml`

Fires when `ci.yml` completes successfully on `main`. Tags and dispatches `release-dev.yml`.

### Workflow: `release-signed.yml`

Manual `workflow_dispatch`. Requires signing secrets. Builds signed, notarized installers for mac, Windows and Linux in one matrix, then publishes.

Replaced the earlier `release-mac.yml` / `release-win.yml` pair, which raced each other for the same tag and — on the Windows side — uploaded the artifacts *before* signing them.

---

## 9. Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `ORT_DYLIB_PATH` | unset | Path to `libonnxruntime.{so,dylib,dll}`, ONNX Runtime 1.23 or newer (`ort` is built for API level 23, and 1.23.2 is the version to develop against, see [#383](https://github.com/laadtushar/edytlab/issues/383#issuecomment-6106785529)); a relative path is resolved against the executable's directory, never the working directory. Read by `ml_pipeline::runtime::ensure`, which tries it first, then the directory the installer puts the library in (`Contents/Frameworks` on macOS, Tauri's resource directory on Linux, registered by the app at startup), then next to the executable, and never searches the working directory or the system. Nothing in the app sets it. The release installers carry the pinned library since v0.7.0 ([#383](https://github.com/laadtushar/edytlab/issues/383)), and a build from source has none: to bundle it in a local `tauri build`, fetch it into `apps/desktop/src-tauri/ml-runtime/<platform>` (on Windows also run `python -I scripts/installer-ml-runtime.py stage-vc-runtime apps/desktop/src-tauri/ml-runtime/vc-runtime-x64`) and pass `--config src-tauri/tauri.ml-runtime.<linux|macos|windows>.conf.json`, as the release workflows do. Settings shows which library loaded and its version (`ml_runtime_status`), and on macOS older than 13.4 the app refuses to open any library it finds, including one `ORT_DYLIB_PATH` names (a reported version of `10.16`, which is what macOS answers under `SYSTEM_VERSION_COMPAT=1`, is not a real version and does not refuse). On Windows the library's own imports (the VC++ runtime DLLs) are searched for in its directory and `System32` only, never the working directory or `PATH`. `scripts/fetch-ml-runtime.sh linux-x64 ml-runtime/linux-x64` downloads the pinned one, checked against `apps/desktop/src-tauri/ml-runtime.json` (`win-x64` and `osx-universal2` fetch the others), and CI runs `ml-pipeline`'s ignored `cache_smoke` and `runtime_bundle_real` tests against it on Linux: `ORT_DYLIB_PATH="$PWD/ml-runtime/linux-x64/libonnxruntime.so" cargo test -p ml-pipeline --test cache_smoke --test runtime_bundle_real -- --ignored --test-threads=1` |
| `WHISPER_MODEL_PATH` | unset | The speech model's **folder** (`onnx/encoder_model.onnx`, `onnx/decoder_model_merged_int8.onnx`, `tokenizer.json`). `transcribe` uses it instead of the model downloaded in Settings; the ignored `ml-whisper` and `tools` tests need it — see [ML Model Files](#ml-model-files-whisperdemucs) |
| `DEMUCS_MODEL_PATH` | unset | The Demucs model **file** — see [ML Model Files](#ml-model-files-whisperdemucs) |

`RUST_LOG` is not listed because nothing reads it (see [Rust Debugging](#rust-debugging)). Skills, agent profiles and the MCP config live under `~/.edytlab/`, which is not configurable.

These are set at runtime; no `.env` file is needed for development.

---

## 10. Common Issues and Fixes

### `error: linker 'cc' not found` (Linux)

Install build-essential:
```bash
sudo apt-get install -y build-essential
```

### `WebView2 not found` (Windows)

The installer handles this automatically via the `embedBootstrapper` mode in `tauri.conf.json`. During development, install WebView2 from [microsoft.com/edge/webview2](https://developer.microsoft.com/microsoft-edge/webview2/).

### `pnpm: command not found` (Windows PowerShell)

```powershell
$env:PATH += ";$env:APPDATA\npm"
```

Add this to your PowerShell profile for persistence.

### Frontend HMR not refreshing after Rust changes

Tauri dev mode recompiles Rust and restarts the shell — the WebView needs a manual refresh in some edge cases. Press `Ctrl+R` in the app window.

### `zoom() throws "No audio loaded"`

See [WaveSurfer Quirks in architecture.md](./architecture.md#wavesurfer-quirks). Guard:
```typescript
if (!wsRef.current || duration === 0) return;
```

### `cargo test` flaking on shared state

Add `-- --test-threads=1` to serialize tests within each binary, as CI does: tests that share global state — `crates/ai`'s model-catalogue cache, for one — race otherwise (`cache_returns_within_ttl` flaked on Windows next to a test that calls `clear_cache()`).

### Keychain access dialog on macOS

On macOS, the first `keyring::Entry::get_password()` call triggers a system dialog. In CI, keychain access is pre-authorized. Locally, approve the dialog once.

### Linux: settings gone after a restart, or the "No Secret Service" banner

On Linux edytlab keeps keys and settings in the **Secret Service** (GNOME Keyring, KWallet or KeePassXC), which survives a reboot. The banner "No Secret Service … is running" means nothing answered on the session D-Bus, so the app fell back to the kernel keyring, and what you save lasts only until the machine restarts (`get_keychain_persistence` reports `persistent: false` and why). This is common in a bare window manager, a container, an SSH session or a CI box. A Secret Service that goes away *after* launch is not in the banner; the save commands report it instead (each answers `persistent: false` and why for a value that fell back), and Settings shows the same words after the Save that did, then waits for Continue.

To get a persistent store on a machine without a desktop, start a throwaway Secret Service for one session and run the app inside it:

```bash
dbus-run-session -- sh -c 'echo -n dev | XDG_DATA_HOME="$(mktemp -d)" gnome-keyring-daemon --unlock --components=secrets; pnpm --filter @edytlab/desktop tauri dev'
```

The `XDG_DATA_HOME` on the daemon matters: `gnome-keyring-daemon --unlock` writes `$XDG_DATA_HOME/keyrings/login.keyring`, so without it the daemon works in your real `~/.local/share/keyrings` and can create a `login.keyring` there that outlives the session. With it, the keyring lives in a temporary directory and goes when you delete that. (It is set for the daemon only, so the app keeps its usual data directory.) For a permanent store, run GNOME Keyring, KWallet or KeePassXC with its Secret Service integration turned on in your normal session. (A KeePassXC with no database exposed does not work: both reading and saving fail.)

If the banner is "Couldn't read your saved settings from the system keychain", the Secret Service is there but locked or the unlock prompt was dismissed: unlock it and restart edytlab. Until then the keychain is not asked again, so no agent is built (a chat message says "no agent configured: the keychain refused access at launch …"); pressing Save in Settings tries again, and a save that goes through builds it.

The real-keychain tests in `crates/ai/tests/keychain_secret_service.rs` are `#[ignore]`: they write to the keychain of whoever runs them, and CI has no Secret Service. Run them inside the same kind of session, with its own `XDG_DATA_HOME`:

```bash
dbus-run-session -- sh -c 'echo -n dev | XDG_DATA_HOME="$(mktemp -d)" gnome-keyring-daemon --unlock --components=secrets; cargo test -p ai --test keychain_secret_service -- --ignored'
```

What CI does pin, with no Secret Service: the store logic against fake slots, and `the_linux_default_store_is_the_secret_service_not_the_kernel_keyring`, which fails if the `keyring` features in `crates/ai/Cargo.toml` stop selecting the Secret Service.

### `cannot find crate for X`

Run `cargo build --workspace` first to ensure all crate artifacts are built. If a workspace member is missing from `Cargo.toml [workspace.members]`, add it.

### TypeScript errors after adding a Tauri command

Ensure the return type in `commands.rs` matches the TypeScript interface in `tauri-bridge.ts`. Rust `u64` maps to TypeScript `number`, `String` maps to `string`, `Option<T>` maps to `T | null`.

---

## 11. Acceptance Gate

Run this before opening any PR. CI runs the same checks.

```bash
# 1. Format check
cargo fmt --all -- --check

# 2. Lint (zero warnings allowed)
cargo clippy --workspace --all-targets -- -D warnings

# 3. Rust tests (serialized)
cargo test --workspace -- --test-threads=1

# 4. Frontend tests
pnpm --filter @edytlab/desktop test

# 5. Frontend tests with React's scheduler 40 ms late (#349)
pnpm --filter @edytlab/desktop test:slow-scheduler

# 6. TypeScript check (tsc -b: app, tests, vite.config.ts)
pnpm --filter @edytlab/desktop typecheck

# 7–8. The frontend in Chromium — for anything that changes what mounts, draws, scrolls or decodes
pnpm --filter @edytlab/desktop typecheck:e2e
pnpm --filter @edytlab/desktop test:e2e
```

All eight must pass. CI blocks merge on any failure.

---

*Kept current by the staleness sweep in `CLAUDE.md`; it describes `main`, not a release.*
