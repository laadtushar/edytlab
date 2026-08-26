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
  libappindicator3-dev \
  librsvg2-dev \
  libssl-dev \
  libasound2-dev \
  patchelf
```

### ML Model Files (Optional — for Whisper/Demucs)

Whisper and Demucs models are large binary files not committed to the repo. They are downloaded on first use or can be pre-staged:

```bash
# Create the model cache directory
mkdir -p ~/.edytlab/models

# Whisper large-v3 (~1.5 GB)
# edytlab will attempt to download automatically on first transcription

# Demucs htdemucs (~80 MB)
# edytlab will attempt to download automatically on first stem separation
```

Set `EDYTLAB_MODEL_DIR` to override the default model cache location.

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

`pnpm tauri:dev` is defined in `apps/desktop/package.json` and expands to:
```bash
pnpm --filter @edytlab/desktop tauri dev
```

Which in turn runs `cargo tauri dev` inside `apps/desktop/src-tauri/`.

### Setting up an API Key (required to use the agent)

1. Get a key from any supported provider:
   - Anthropic: https://console.anthropic.com/settings/keys
   - OpenRouter: https://openrouter.ai/keys
   - OpenAI: https://platform.openai.com/api-keys

2. In the running app, click the gear icon → enter your key. The key is stored in your OS keychain — never on disk or committed to git.

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
3. Run `pnpm --filter @edytlab/desktop exec tsc --noEmit` to typecheck

### Making a Rust Change

1. Edit files under `crates/` or `apps/desktop/src-tauri/`
2. `pnpm tauri:dev` will detect the change, recompile Rust, and restart the Tauri shell
3. Full recompile takes 5–30 seconds depending on the changed crate

### Adding a New Tauri Command

1. Add the function to `apps/desktop/src-tauri/src/commands.rs`
2. Add `#[tauri::command]` attribute
3. Register in `tauri::Builder::invoke_handler` in `lib.rs`
4. Add a matching TypeScript wrapper in `apps/desktop/src/lib/tauri-bridge.ts`
5. Update [API Reference](./api-reference.md)

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
│   ├── Canvas.tsx             # Waveform canvas rendering
│   ├── CapabilitiesMenu.tsx   # + button for capabilities
│   ├── Chat.tsx               # Chat panel, message streaming
│   ├── EmptyState.tsx         # No-audio-loaded state
│   ├── ErrorBanner.tsx        # Error display with recovery CTA
│   ├── GraphView.tsx          # DAG visualization (@xyflow)
│   ├── MarkerLayer.tsx        # Timeline annotations overlay
│   ├── McpServersEditor.tsx
│   ├── MemoryEditor.tsx
│   ├── MessageBubble.tsx      # Single chat message + tool badges
│   ├── Ruler.tsx              # Timeline ruler (time markers)
│   ├── Settings.tsx           # Settings modal (all config)
│   ├── ShortcutsOverlay.tsx   # ? key shortcut display
│   ├── SkillsEditor.tsx
│   ├── ThinkingIndicator.tsx  # Agent working indicator
│   ├── Timeline.tsx           # WaveSurfer waveform + controls
│   └── ToolBadge.tsx          # Tool call badge in messages
├── lib/
│   ├── tauri-bridge.ts        # Type-safe IPC + event wrappers
│   ├── file-open.ts           # Audio file picker + drag-drop
│   ├── graph.ts               # Graph layout (dagre)
│   └── undoRedo.ts            # Undo/redo DAG traversal
└── __tests__/
    ├── App.undoRedo.test.ts
    ├── ShortcutsOverlay.test.tsx
    └── ...
```

### `apps/desktop/src-tauri/src/` (Rust Shell)

```
src/
├── commands.rs    # All ~50 #[tauri::command] functions
├── lib.rs         # AppState, builder, lock helpers
└── main.rs        # Entry point
```

`AppState` (defined in `lib.rs`):
```rust
pub struct AppState {
    pub store:        Arc<Mutex<Option<Store>>>,
    pub engine:       Arc<Mutex<Engine>>,
    pub agent:        Arc<Mutex<Option<Agent>>>,
    pub clipboard:    Arc<Mutex<Option<Vec<f32>>>>,
    pub plan_notify:  Arc<Notify>,
    pub memory:       Arc<MemoryStore>,
    pub skills:       Arc<Mutex<SkillLibrary>>,
    pub profiles:     Arc<Mutex<ProfileLibrary>>,
    pub mcp:          Arc<Mutex<McpRegistry>>,
    pub project_dir:  Arc<Mutex<Option<PathBuf>>>,
}
```

### `crates/ai/src/` (AI subsystem)

```
src/
├── lib.rs          # Agent, LlmConfig, AgentEvent, TurnResult
├── agent_loop.rs   # Tool-calling state machine
├── anthropic.rs    # Canonical message/stream types
├── keychain.rs     # OS credential storage via keyring
├── models.rs       # Model catalogue + 10-min TTL cache
├── prompt.rs       # System prompt construction
├── provider.rs     # LlmProvider trait + 3 implementations
├── session_context.rs  # Selection/marker context for turns
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
pnpm --filter @edytlab/desktop exec tsc --noEmit
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
pnpm --filter @edytlab/desktop exec tsc --noEmit
```

### Test Organization

| Layer | Framework | Location |
|-------|-----------|---------|
| Rust unit tests | `cargo test` | `#[cfg(test)]` mod in each file |
| Rust integration tests | `cargo test` | `tests/` in each crate |
| Frontend unit tests | Vitest | `apps/desktop/src/__tests__/` |
| HTTP mock tests | `wiremock` | `crates/ai/tests/` |

### Writing New Tests

**Rust tools tests** — test against a minimal `SessionState`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use session::test_helpers::minimal_state;

    #[test]
    fn normalize_rejects_nan_lufs() {
        let mut ctx = test_context();
        let result = NormalizeTool.call(
            serde_json::json!({ "target_lufs": f64::NAN }),
            &mut ctx,
        );
        assert!(matches!(result, ToolResult::Error(_)));
    }
}
```

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

**`tracing` crate** — structured logging throughout the Rust codebase:
```bash
# Enable debug logs
RUST_LOG=debug pnpm tauri:dev

# Enable trace logs for specific crate
RUST_LOG=edytlab_ai=trace pnpm tauri:dev
```

Log output appears in the terminal where `tauri:dev` is running.

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

**Undo/redo not working:**
Check that `head` is in the `useEffect` dependency array for the undo/redo handlers.

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
4. `release-dev.yml` builds unsigned bundles for macOS (universal) + Windows
5. Attaches to a draft GitHub Release (not auto-published)

### Signed Mac Build

Requires Apple Developer Program membership + signing secrets in GitHub.

**Required secrets:**
- `APPLE_CERTIFICATE` (base64-encoded .p12)
- `APPLE_CERTIFICATE_PASSWORD`
- `APPLE_SIGNING_IDENTITY` (e.g., "Developer ID Application: Your Name (TEAMID)")
- `APPLE_TEAM_ID`
- `APPLE_ID` (for notarization)
- `APPLE_ID_PASSWORD` (app-specific password)

Trigger `release-signed.yml` via GitHub Actions manual dispatch.

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
"version": "0.1.0"
```

`package.json` files mirror this. When bumping a version:
1. Update `tauri.conf.json`
2. Update `apps/desktop/package.json`
3. Update `apps/desktop/src-tauri/Cargo.toml`
4. Commit as `chore: bump version to X.Y.Z`

---

## 8. CI Pipeline

### Workflow: `ci.yml`

**Trigger:** Push to `main` + all PRs.

**Matrix:** macOS 14 (arm64) · Windows latest (x86_64) · Ubuntu 22.04 (x86_64).

**Jobs:**
1. Install platform build deps (Linux only: GTK, WebKit, AppIndicator, libssl, libasound2)
2. Install Rust toolchain (from `rust-toolchain.toml`)
3. Cache cargo registry + `target/` (keyed per OS + `Cargo.lock` hash)
4. Install pnpm + Node 20
5. `pnpm install --frozen-lockfile`
6. `cargo fmt --all -- --check`
7. `cargo clippy --workspace --all-targets -- -D warnings`
8. `cargo test --workspace --no-fail-fast -- --test-threads=1`
9. `pnpm --filter @edytlab/desktop build`

**Important:** CI concurrency is set to cancel in-flight PR runs but **never cancel main runs** — `auto-release.yml` waits on the `workflow_run` completion event.

### Workflow: `auto-release.yml`

Fires when `ci.yml` completes successfully on `main`. Tags and dispatches `release-dev.yml`.

### Workflow: `release-signed.yml`

Manual `workflow_dispatch`. Requires signing secrets. Builds signed, notarized installers for macOS, Windows and Linux in one matrix, then publishes.

Replaced the earlier separate release workflows,which suffered from race-condition tagging problems and uploaded Windows artifacts prior to code-signing.

---

## 9. Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `RUST_LOG` | `info` | Tracing log level. Format: `level` or `crate=level` |
| `ORT_DYLIB_PATH` | Auto-detected | Path to `libonnxruntime.{so,dylib,dll}` |
| `EDYTLAB_MODEL_DIR` | `~/.edytlab/models` | ML model cache directory |
| `EDYTLAB_DATA_DIR` | OS app data dir | App data root (skills, profiles, MCP config) |

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

### `cargo test` OOM on CI

Add `-- --test-threads=1` to serialize tests. Model loading in `ml-pipeline` tests is memory-intensive when concurrent.

### Keychain access dialog on macOS

On macOS, the first `keyring::Entry::get_password()` call triggers a system dialog. In CI, keychain access is pre-authorized. Locally, approve the dialog once.

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

# 5. TypeScript check (no emit)
pnpm --filter @edytlab/desktop exec tsc --noEmit
```

All five must pass. CI blocks merge on any failure.

---

*Last updated: 2026-05-17. Reflects edytlab v0.1.0-dev.*
