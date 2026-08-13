# CLAUDE.md

Project-level instructions for Claude Code sessions in this repository. These persist across sessions; treat them as durable user preferences.

## Workflow

### Pull requests

- **Always squash-merge a PR you opened once its CI is green.** Don't wait for explicit approval each time. If CI fails, investigate the failure rather than merging anyway.
- Open PRs as **drafts** by default; flip to ready when CI passes (squash-merge implies ready).
- One concern per PR. If a follow-up touches different files / different reviewer focus, open a separate PR rather than tacking it onto the current one.
- Reply to AI review comments (Gemini etc.) only when the suggestion is being declined or when noting a fix landed in a specific commit. Skip routine "fixed it" replies on threads that are already self-evident.

### Branches

- Feature branches: `claude/feature/<short-kebab-summary>`
- Fix branches: `claude/fix/<short-kebab-summary>`
- Always branch off latest `origin/main`, not whatever the working tree happens to be on.

### Commits

- Conventional-commit prefixes: `feat:`, `fix:`, `ci:`, `chore:`, `docs:`, `test:`, `refactor:`. Scope optional (`feat(ai): ...`).
- Each commit message ends with the session footer:
  ```
  https://claude.ai/code/session_<id>
  ```
  where `<id>` is the current session id (substituted by the harness at write time).

## Shell / path conventions

This repo lives at `C:\Users\tusha\Work\Playground\Edytlab\edytlab` on Windows 11.

- **Always use the PowerShell tool for git commands** (`git`, `pnpm`, `cargo`). The Bash tool's git binary cannot resolve `/c/...` paths reliably on this machine.
- In PowerShell, prefix every command with `cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab";` or chain with `;`.
- The Bash tool works fine for read-only file operations (`find`, `ls`, path inspection) using `/c/Users/tusha/...` Unix-style paths, but **not for git or pnpm**.
- `pnpm` is not on the Bash PATH — always invoke it via PowerShell.

## Repo specifics

- Tauri 2 desktop app under `apps/desktop/`. Frontend in `apps/desktop/src/`, Rust backend in `apps/desktop/src-tauri/`.
- Workspace crates under `crates/`: `ai` (LLM provider abstraction), `tools` (audio editing primitives), others.
- Multi-provider LLM support: Anthropic, OpenRouter, OpenAI. The `LlmProvider` trait in `crates/ai/src/provider.rs` is the extension point — request serialization + SSE parsing are per-provider.
- Per-provider keychain slots: `<provider_id>_api_key` plus an `active_provider` slot. Legacy unsuffixed `anthropic_api_key` is still read for back-compat.
- App version is canonical in `apps/desktop/src-tauri/tauri.conf.json` — `package.json` files mirror it.

## CI / release

- `ci.yml` runs on push to main + PRs: fmt, clippy, cargo test, frontend build, vitest. Tauri bundle is intentionally NOT in CI (too slow); release workflows cover that.
- `auto-release.yml` fires off CI's `workflow_run` on main: tags `v0.1.0-dev.<run_number>` and dispatches `release-dev.yml`.
- `release-dev.yml` uses a `create-release` job + matrix to avoid the parallel-job race that produced duplicate releases for the same tag.
- `release-dev.yml` has a `channel` dispatch input: `dev` (default — prerelease, not Latest, "unsigned dev build") and `release` (a real versioned release, not a prerelease, marked Latest). Tag pushes are always `dev`; a bare `vX.Y.Z` tag deliberately does not trigger it, so `release-signed.yml` owns those tags once certs exist. Both channels emit the same install warnings, because those describe the artifact rather than the channel.
- Signed releases (`release-signed.yml`) are manual `workflow_dispatch` and require Apple/Windows signing secrets. Same `create-release` + matrix + `publish` shape as `release-dev.yml`. tauri-action is given neither `tagName` nor `releaseId` there — it only builds, and upload happens after signing, so unsigned artifacts can never reach the release.
- Bundle targets are explicit in `tauri.conf.json`: `["app", "dmg", "msi", "nsis", "deb", "appimage"]`. Don't revert to `"all"` — it silently dropped installer formats under some build conditions.

## Audio engine / WaveSurfer quirks

- `wsRef.current.zoom()` throws "No audio loaded" when WaveSurfer has no decoded data — always guard with `if (!wsRef.current || duration === 0) return` and include `duration` in the useEffect dep array.
- React's `onWheel` is passive in Chromium/Tauri — Ctrl+scroll requires `el.addEventListener("wheel", handler, { passive: false })` via useEffect, not the `onWheel` JSX prop.
- Window-level `keydown` listeners do NOT stop each other via `e.stopPropagation()` — guard with a state variable (e.g., `!showShortcuts`) instead.
- Keyboard shortcut handlers in App.tsx that should fire without audio must be placed **before** `if (!timelineRef.current) return`, not after.

## Tauri command conventions

- Return type alias: `CmdResult<T>` (= `Result<T, String>`) — all commands use it; never raw `Result<T, String>`.
- Store and engine locks must be in separate scopes: acquire and drop store lock before opening engine lock to avoid double-borrow panics. Use `let state = { let store = lock_std(...); store.get(id)? };` pattern.

## Website (`website/`)

- Separate Next.js 16 app; build with `pnpm --filter @edytlab/website build`.
- `framer-motion` v11 already installed — import from `"framer-motion"` (not `"motion/react"`).
- All components using framer-motion hooks need `"use client"` directive.

## Acceptance gates

Before merging anything:
- `cargo fmt --all -- --check` clean
- `cargo clippy --workspace --all-targets -- -D warnings` clean
- `cargo test --workspace` passes
- `pnpm --filter @edytlab/desktop test` passes
- `pnpm --filter @edytlab/desktop exec tsc --noEmit` clean
