# Contributing to edytlab

edytlab is open source. This document covers how to contribute effectively — from bug reports and documentation to new tools and LLM providers.

---

## Table of Contents

1. [Ways to Contribute](#1-ways-to-contribute)
2. [Opening Issues](#2-opening-issues)
3. [Pull Request Workflow](#3-pull-request-workflow)
4. [Code Style and Standards](#4-code-style-and-standards)
5. [Commit Messages](#5-commit-messages)
6. [Adding a New Audio Tool](#6-adding-a-new-audio-tool)
7. [Adding a New LLM Provider](#7-adding-a-new-llm-provider)
8. [Adding a New Frontend Component](#8-adding-a-new-frontend-component)
9. [Writing Tests](#9-writing-tests)
10. [Documentation Updates](#10-documentation-updates)
11. [Release Process](#11-release-process)

---

## 1. Ways to Contribute

| Type | Examples | Effort |
|------|----------|--------|
| Bug reports | Crash reports, wrong behavior, UI regressions | Low |
| Documentation | Fix typos, add examples, clarify explanations | Low |
| Bug fixes | Off-by-one errors, null panics, incorrect output | Medium |
| New audio tools | A new deterministic DSP operation for the agent | Medium |
| Frontend features | New UI component, improved empty state | Medium |
| New LLM providers | Cohere, Mistral-native (Gemini, Groq and Ollama are already in) | High |
| New ML models | Alternative transcription, better stem separation | High |
| Architecture changes | Session model extensions, DAG operations | High — discuss first |

For any change larger than a bug fix, **open an issue first** to discuss the approach before writing code. This prevents duplicate work and ensures the change aligns with the project direction.

---

## 2. Opening Issues

### Bug Reports

A good bug report includes:

```markdown
**edytlab version:** v0.1.0-dev.31
**OS:** macOS 14.3 (arm64)
**Provider:** Anthropic / claude-sonnet-4-6

**Steps to reproduce:**
1. Load a WAV file
2. Type "normalize to -14 LUFS"
3. Observe

**Expected:** Track normalized to -14 LUFS
**Actual:** "normalize: NaN target" error appears

**Terminal output (when run with `pnpm tauri:dev`):**
[paste relevant output — `RUST_LOG` does nothing yet, see development-guide.md §6]
```

### Feature Requests

Describe the **use case**, not just the feature. "I want a low-pass filter tool" is less useful than "When preparing podcast audio, I need to roll off high frequencies above 12 kHz to reduce mic handling noise — currently I have to export and process externally."

---

## 3. Pull Request Workflow

### Before You Start

1. Check that an issue exists (or create one) for non-trivial changes
2. Make sure you are branching off latest `origin/main`:
   ```bash
   git fetch origin
   git checkout -b claude/feature/my-change origin/main
   ```
3. Read the [development guide](./development-guide.md) for setup

### Branch Naming

```
claude/feature/<short-kebab-summary>   # new functionality
claude/fix/<short-kebab-summary>       # bug fixes
```

### During Development

- One concern per PR. If you discover a related issue while working, open a separate PR for it.
- Run the [acceptance gate](./development-guide.md#11-acceptance-gate) before pushing.
- Write tests for new behavior (see [Writing Tests](#9-writing-tests)).
- Keep the diff focused — don't mix reformatting with logic changes.

### Opening the PR

- Open as a **draft** initially.
- Title follows Conventional Commits: `feat(tools): add spectral repair tool`
- Description should cover: what changed, why, how to test it manually.
- Link to the related issue with `Closes #123`.
- Flip to "Ready for Review" when CI passes.

### Merging

- PRs are **squash-merged** (single commit on `main`).
- You may merge your own PR once CI is green — no approval wait required.
- Do not merge with failing CI. Investigate the failure.
- After merge, the auto-release workflow creates a new dev build automatically.

### Review Comments

- AI review comments (Gemini, etc.) are informational. Reply only when:
  - Declining a suggestion (explain why)
  - Noting that a fix landed in a specific commit
- Skip "fixed, thanks" replies — they add noise.

---

## 4. Code Style and Standards

### Rust

**Format:** `cargo fmt` (enforced in CI). Run before committing:
```bash
cargo fmt --all
```

**Lints:** `cargo clippy -- -D warnings`. Warnings are errors in CI. Common fixes:
- Use `clippy::pedantic` suggestions where they improve clarity
- Suppress specific lints with `#[allow(clippy::...)]` only when the lint is wrong for the context — add a comment explaining why

**Error handling:**
- All commands return `CmdResult<T>` (= `Result<T, String>`)
- Use `thiserror` for crate-level error enums
- Convert to `String` only at the command boundary with `.map_err(|e| e.to_string())`
- Never `unwrap()` in production code paths — use `?` or explicit error handling

**Naming:**
- Structs/enums: `PascalCase`
- Functions/variables: `snake_case`
- Constants: `SCREAMING_SNAKE_CASE`
- Trait methods that are not yet implemented: `todo!("reason")` — never `unimplemented!()`

**Concurrency:**
- Acquire and drop the Store lock before opening the Engine lock (prevents double-borrow panics)
- Pattern:
  ```rust
  let state = {
      let store = lock_std(&state.store, "store")?;
      store.get(id)?
  };
  // store lock released here
  let engine = lock_std(&state.engine, "engine")?;
  ```

**Documentation:**
- Public API items get a `///` doc comment explaining the **why**, not the what
- Complex invariants get `//` inline comments
- No multi-line comment blocks on simple code

### TypeScript / React

**Format:** The project uses the default Vite/TypeScript formatting. Keep it consistent.

**No `any`:** All types must be explicit. Use the types defined in `tauri-bridge.ts`.

**React patterns:**
- Function components only — no class components
- Hooks for state and effects
- `useCallback` for event handlers passed to child components
- `useMemo` for expensive derivations (e.g., graph layout)
- No inline object/array props unless the reference is stable

**Event listeners:**
- `window.addEventListener("keydown", ...)` must be removed in the `useEffect` cleanup
- `onWheel` JSX prop is passive in Tauri/Chromium — use `addEventListener("wheel", ..., { passive: false })` for scroll zoom
- Guard with state flags instead of `stopPropagation` for window-level handlers

**Tauri IPC:**
- All Tauri calls go through `tauri-bridge.ts` — never call `invoke` directly from components
- The bridge is the type boundary; it must stay in sync with `commands.rs`

### CSS / Tailwind

- Use Tailwind utility classes — no custom CSS unless Tailwind cannot express it
- Dark-theme first (the app has a dark theme)
- `data-testid` attributes on interactive elements for testing

---

## 5. Commit Messages

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <short summary>

[optional body — what and why, not how]

[optional footer — BREAKING CHANGE, Closes #issue]
```

**Types:** `feat`, `fix`, `ci`, `chore`, `docs`, `test`, `refactor`

**Scope** (optional): `ai`, `tools`, `session`, `audio-engine`, `frontend`, `tauri`, `website`

**Rules:**
- Subject line ≤ 72 characters
- Imperative mood: "add tool" not "adds tool" or "added tool"
- No trailing period
- Body explains **why** if non-obvious

**Examples:**
```
feat(tools): add spectral noise reduction tool

Adds a new `denoise` tool that runs spectral subtraction against
a noise profile sampled from the first 500ms of the track.
Useful for room tone removal in podcast recordings.

Closes #88
```

```
fix(ai): prevent tool budget exceeded on long mashup sessions

Agent was re-running the full tool chain after partial completion
when the context window was truncated. Add explicit budget check
before each tool dispatch instead of only at the start.
```

---

## 6. Adding a New Audio Tool

Tools are the building blocks the AI agent uses to edit audio. Adding one is the most common contribution type.

### Step-by-Step

**1. Create the tool file:**

```bash
touch crates/tools/src/tool/my_tool.rs
```

**2. Implement the `Tool` trait** (defined in `crates/tools/src/dispatcher.rs`). This mirrors `tool/mute_track.rs`, a real tool:

```rust
// crates/tools/src/tool/my_tool.rs

use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, check_track_index, load_head_state};
use crate::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    amount_db: f32,
}

pub struct MyTool;

impl Tool for MyTool {
    fn name(&self) -> &'static str {
        "my_tool"
    }

    /// Name, description and input schema, in the Anthropic tool shape.
    /// The dispatcher validates every call against `input_schema` before
    /// `invoke` runs, and the tools reference is generated from it.
    fn schema(&self) -> Value {
        anthropic_tool(
            "my_tool",
            "One-sentence description of what this tool does and when to use it.",
            json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer", "description": "Index of the track." },
                    "amount_db": { "type": "number", "description": "Amount in dB (-60 to +12)." }
                },
                "required": ["track", "amount_db"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        // Bad input is a `ToolResult::Error` the model can read and
        // correct, not an `Err` and never a panic.
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        if !args.amount_db.is_finite() || !(-60.0..=12.0).contains(&args.amount_db) {
            return Ok(ToolResult::Error(format!("amount_db out of range: {}", args.amount_db)));
        }
        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        if let Err(e) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(e));
        }

        state.tracks[args.track].gain_db += args.amount_db;

        // Appends a new DAG node parented to the current head.
        let label = format!("my_tool {} {:+} dB", args.track, args.amount_db);
        let new_id = match append_state(ctx, state, label) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({ "node_id": new_id.to_hex() })))
    }
}
```

**3. Register it.** Export it from `crates/tools/src/tool/mod.rs`, then register it in `ToolDispatcher::default_dispatcher()` in `crates/tools/src/dispatcher.rs`:

```rust
// crates/tools/src/tool/mod.rs
pub mod my_tool;
pub use my_tool::MyTool;

// crates/tools/src/dispatcher.rs, inside default_dispatcher()
d.register(Box::new(MyTool));
```

**4. Write tests.** Integration tests in `crates/tools/tests/` drive tools through a real dispatcher, store and engine; `tools_integration.rs` is the model:

```rust
use serde_json::json;
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

#[test]
fn rejects_out_of_range_amount() {
    let tmp = TempDir::new().unwrap();
    let mut store = session::Store::open(tmp.path()).unwrap();
    let mut engine = audio_engine::Engine::new();
    let dispatcher = ToolDispatcher::default_dispatcher();
    let mut clipboard: Option<tools::Clipboard> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
        allowed_tools: None,
    };

    let result = dispatcher
        .invoke("my_tool", json!({ "track": 0, "amount_db": 99.0 }), &mut ctx)
        .unwrap();
    assert!(matches!(result, ToolResult::Error(_)));
}
```

The website's tool reference, `website/app/docs/tools/page.tsx`, must list every registered tool: `crates/tools/tests/website_tool_docs.rs` fails until it does.

**5. Regenerate the tools reference.**

[tools-reference.md](./tools-reference.md) is generated from the registry — do not edit it by hand:

```bash
UPDATE_TOOLS_REFERENCE=1 cargo test -p tools --test tools_reference_doc
```

Commit the result. The name, description and parameter table all come from the schema your tool returns, so whatever you write there is what a contributor reads. CI fails if the committed file does not match.

---

## 7. Adding a New LLM Provider

See [architecture.md §14](./architecture.md#14-extension-points) for the full guide. Summary:

1. Implement `LlmProvider` in `crates/ai/src/provider.rs` (an OpenAI-compatible API can delegate serialization and parsing to `OpenAIProvider`, as Groq, Gemini and Ollama do)
2. Add to `SUPPORTED_PROVIDER_IDS` + the `provider_from_id()` factory
3. Add an arm to `list_models_for_at()` in `crates/ai/src/models.rs`
4. Update the `ProviderId` union in `tauri-bridge.ts` and the `PROVIDERS` list in `components/Settings.tsx`
5. Write unit tests for request serialization and stream parsing
6. Test with a real API key against the provider's sandbox/test environment

Keychain slots need no code: they are keyed by provider id (`<id>_api_key`, `<id>_model`, `<id>_base_url`).

A provider for small-context (local) models overrides `tool_set()` to return `ToolSet::Slim`, so it is sent a core of tools plus the ones a message names rather than every tool, and the system prompt names the rest so the model knows they exist. Leave it at the default for a hosted provider; Anthropic must stay on the full list for its prompt cache. `cargo test -p ai --test request_size -- --nocapture` prints how big a first request is.

The hardest part is usually stream parsing — write exhaustive tests covering partial chunks, multi-event chunks, tool call id synthesis, and the `[DONE]` sentinel.

---

## 8. Adding a New Frontend Component

1. Create `apps/desktop/src/components/MyComponent.tsx`
2. Use Tailwind for styling — no custom CSS unless unavoidable
3. Add `data-testid` attributes to interactive elements
4. Export from the file: `export function MyComponent(...) { ... }`
5. Import in the parent component — avoid barrel re-exports in the components folder
6. Write a test in `apps/desktop/src/__tests__/MyComponent.test.tsx`

**For motion**, the desktop app has no animation library — `framer-motion` is not a dependency of `@edytlab/desktop`. Use CSS transitions on the shared duration and easing tokens in `src/styles.css` (`--dur-1`…`--dur-3`, `--ease-out`, `--ease-in-out`); [motion-audit.md](./motion-audit.md) says which to use when, and when to use none.

---

## 9. Writing Tests

### Test Priorities

1. **Tool input validation** — every tool must reject invalid input without panicking
2. **State mutation** — tools that mutate state must produce the correct next state
3. **Round-trips** — serialize/deserialize SessionState, DAG node load/save
4. **Provider parsing** — every StreamEvent variant from every provider
5. **Frontend event handling** — keyboard shortcuts, drag events, resize

### Test Helpers

There is no shared test-helper module. The patterns in use:

- **Rust tools:** build a real `ToolContext` over a `TempDir` store, as in [§6 step 4](#6-adding-a-new-audio-tool) and `crates/tools/tests/tools_integration.rs`.
- **Frontend:** mock the bridge module rather than Tauri's `invoke`, so a test names the calls it expects:

  ```typescript
  vi.mock("../lib/tauri-bridge", () => ({
    getSessionHead: vi.fn(() => Promise.resolve("abc123")),
  }));
  ```

  To control *when* a mocked call answers, use `src/__tests__/held.ts`, and make sure the suite also passes under `test:slow-scheduler`.

### Coverage Expectations

- New tools: 100% of input paths (valid, invalid, edge cases)
- New provider implementations: all StreamEvent variants, all error paths
- Frontend components: render, interaction, error state

---

## 10. Documentation Updates

Documentation lives in:
- `README.md` — project overview, quickstart
- `docs/architecture.md` — technical design
- `docs/development-guide.md` — dev setup and workflow
- `docs/api-reference.md` — Tauri commands + TypeScript bridge
- `docs/tools-reference.md` — all audio tools (**generated**; regenerate rather than edit)
- `docs/contributing.md` — this file

**Update the docs in the same PR as the code.** A PR that adds a new tool without regenerating `tools-reference.md` is incomplete — and now fails CI rather than shipping a reference that quietly omits it.

When adding a new Tauri command:
1. Document it in `api-reference.md` with signature, description, error cases, example
2. Add the TypeScript wrapper to `tauri-bridge.ts` before the PR

When changing an existing command's signature:
1. Update the Rust type
2. Update the TypeScript type in `tauri-bridge.ts`
3. Update `api-reference.md`
4. Run `pnpm --filter @edytlab/desktop typecheck` to catch any downstream type errors

---

## 11. Release Process

Releases are automated — contributors do not need to manage them.

1. Merge to `main` with a passing CI run
2. `auto-release.yml` tags `v<version>-dev.<run_number>` automatically
3. `release-dev.yml` builds unsigned bundles for macOS (universal), Windows and Linux into a draft GitHub Release, and publishes it as a prerelease once every platform is green

Signed production releases require maintainer access and signing credentials, which are not provisioned yet ([#386](https://github.com/laadtushar/edytlab/issues/386)) — every release so far, v0.2.0 included, is unsigned. See [`docs/development-guide.md#7-building-for-release`](./development-guide.md#7-building-for-release).

---

## Code of Conduct

Be direct, technical, and respectful. Assume good intent. Focus feedback on code and design, not people. Disagreements about approach are expected and healthy — work them out in the issue or PR discussion.

---

*Last updated: 2026-05-17. Reflects edytlab v0.1.0-dev.*
