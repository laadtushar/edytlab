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
| New LLM providers | Cohere, Mistral-native, Gemini | High |
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

**Logs (RUST_LOG=debug):**
[paste relevant log output]
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

**2. Implement the `Tool` trait:**

```rust
// crates/tools/src/tool/my_tool.rs

use crate::{Tool, ToolContext, ToolError};
use serde_json::{Value, json};
use session::SessionState;

pub struct MyTool;

impl Tool for MyTool {
    fn name(&self) -> &'static str { "my_tool" }

    fn description(&self) -> &'static str {
        "One-sentence description of what this tool does and when to use it."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "track_id": {
                    "type": "string",
                    "description": "The track to operate on."
                },
                "amount_db": {
                    "type": "number",
                    "description": "Amount in dB (-60 to +12)."
                }
            },
            "required": ["track_id", "amount_db"]
        })
    }

    fn call(
        &self,
        input: Value,
        ctx: &mut ToolContext,
    ) -> Result<Value, ToolError> {
        let track_id = input["track_id"].as_str()
            .ok_or_else(|| ToolError::InvalidInput("track_id required".into()))?;
        let amount_db = input["amount_db"].as_f64()
            .ok_or_else(|| ToolError::InvalidInput("amount_db required".into()))?;

        // Validate
        if amount_db.is_nan() || amount_db < -60.0 || amount_db > 12.0 {
            return Err(ToolError::InvalidInput(
                format!("amount_db out of range: {}", amount_db)
            ));
        }

        // Get current session state
        let store = ctx.store.lock().map_err(|_| ToolError::Internal("store lock".into()))?;
        let head = store.head().ok_or_else(|| ToolError::InvalidInput("no session open".into()))?;
        let node = store.get(head)?;
        drop(store);

        // Mutate state
        let mut new_state = node.state.clone();
        let track = new_state.tracks.iter_mut()
            .find(|t| t.id.as_str() == track_id)
            .ok_or_else(|| ToolError::InvalidInput(format!("track {} not found", track_id)))?;
        
        track.gain_db += amount_db as f32;

        // Append new DAG node
        let mut store = ctx.store.lock().map_err(|_| ToolError::Internal("store lock".into()))?;
        let new_id = store.append(Some(head), Some("my_tool applied".into()), new_state)?;
        store.set_head(new_id)?;

        Ok(json!({ "node_id": new_id.to_string() }))
    }
}
```

**3. Register in the dispatcher:**

```rust
// crates/tools/src/lib.rs

mod tool {
    // ...existing modules...
    pub mod my_tool;
}

impl ToolDispatcher {
    pub fn new() -> Self {
        let mut tools: HashMap<String, Box<dyn Tool>> = HashMap::new();
        // ...existing registrations...
        tools.insert("my_tool".into(), Box::new(tool::my_tool::MyTool));
        Self { tools }
    }
}
```

**4. Write tests:**

```rust
// crates/tools/src/tool/my_tool.rs (continued)

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{minimal_context};

    #[test]
    fn rejects_nan_amount() {
        let mut ctx = minimal_context();
        let result = MyTool.call(
            serde_json::json!({ "track_id": "t1", "amount_db": f64::NAN }),
            &mut ctx,
        );
        assert!(result.is_err());
    }

    #[test]
    fn applies_gain_correctly() {
        let mut ctx = minimal_context();
        // Load a track first...
        let result = MyTool.call(
            serde_json::json!({ "track_id": "t1", "amount_db": 3.0 }),
            &mut ctx,
        );
        assert!(result.is_ok());
        // Verify state change...
    }
}
```

**5. Regenerate the tools reference.**

[tools-reference.md](./tools-reference.md) is generated from the registry — do not edit it by hand:

```bash
UPDATE_TOOLS_REFERENCE=1 cargo test -p tools --test tools_reference_doc
```

Commit the result. The name, description and parameter table all come from the schema your tool returns, so whatever you write there is what a contributor reads. CI fails if the committed file does not match.

---

## 7. Adding a New LLM Provider

See [architecture.md §14](./architecture.md#14-extension-points) for the full guide. Summary:

1. Implement `LlmProvider` in `crates/ai/src/provider.rs`
2. Add to `SUPPORTED_PROVIDER_IDS` + `from_id()` factory
3. Add keychain slot handling in `commands.rs`
4. Update `ProviderId` union in `tauri-bridge.ts`
5. Write unit tests for request serialization and stream parsing
6. Test with a real API key against the provider's sandbox/test environment

The hardest part is usually stream parsing — write exhaustive tests covering partial chunks, multi-event chunks, tool call id synthesis, and the `[DONE]` sentinel.

---

## 8. Adding a New Frontend Component

1. Create `apps/desktop/src/components/MyComponent.tsx`
2. Use Tailwind for styling — no custom CSS unless unavoidable
3. Add `data-testid` attributes to interactive elements
4. Export from the file: `export function MyComponent(...) { ... }`
5. Import in the parent component — avoid barrel re-exports in the components folder
6. Write a test in `apps/desktop/src/__tests__/MyComponent.test.tsx`

**For animated components**, import from `framer-motion`:
```typescript
import { motion, AnimatePresence } from "framer-motion";
// All components using framer-motion hooks need "use client" in Next.js,
// but in the Tauri app this is not needed — it's already a client-side app.
```

---

## 9. Writing Tests

### Test Priorities

1. **Tool input validation** — every tool must reject invalid input without panicking
2. **State mutation** — tools that mutate state must produce the correct next state
3. **Round-trips** — serialize/deserialize SessionState, DAG node load/save
4. **Provider parsing** — every StreamEvent variant from every provider
5. **Frontend event handling** — keyboard shortcuts, drag events, resize

### Test Helpers

Use shared helpers to avoid boilerplate:

```rust
// Rust — minimal session context
use crate::test_helpers::{minimal_context, minimal_state_with_track};

// Frontend — mock Tauri invoke
import { mockIPC } from "@tauri-apps/api/mocks";
mockIPC((cmd, args) => {
  if (cmd === "get_session_head") return "abc123";
});
```

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
4. Run `tsc --noEmit` to catch any downstream type errors

---

## 11. Release Process

Releases are automated — contributors do not need to manage them.

1. Merge to `main` with a passing CI run
2. `auto-release.yml` tags `v<version>-dev.<run_number>` automatically
3. `release-dev.yml` builds unsigned bundles and attaches to a draft GitHub Release

Signed production releases require maintainer access and signing credentials. See [`docs/development-guide.md#7-building-for-release`](./development-guide.md#7-building-for-release).

---

## Code of Conduct

Be direct, technical, and respectful. Assume good intent. Focus feedback on code and design, not people. Disagreements about approach are expected and healthy — work them out in the issue or PR discussion.

---

*Last updated: 2026-05-17. Reflects edytlab v0.1.0-dev.*
