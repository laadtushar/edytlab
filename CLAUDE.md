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
- Docs and chores: `claude/docs/<…>`, `claude/chore/<…>` (a release bump is `claude/chore/release-<version>`).
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
- In a Linux cloud session (claude.ai/code) none of this applies: the checkout is a normal Linux path and Bash runs git, pnpm and cargo directly.

## Repo specifics

- Tauri 2 desktop app under `apps/desktop/`. Frontend in `apps/desktop/src/`, Rust backend in `apps/desktop/src-tauri/`.
- Workspace crates under `crates/`: `ai` (LLM provider abstraction and agent loop), `tools` (the agent's audio editing tools — `docs/tools-reference.md` is generated from them), `audio-engine` (render), `audio-time` (phase vocoder), `session` (the node graph), others.
- Multi-provider LLM support: Anthropic, OpenRouter, OpenAI, Groq, Gemini and Ollama (local, no key). The `LlmProvider` trait in `crates/ai/src/provider.rs` is the extension point — request serialization + SSE parsing are per-provider.
- Per-provider keychain slots: `<provider_id>_api_key`, `<provider_id>_model`, `<provider_id>_base_url`, plus an `active_provider` slot. Anthropic's slot is `anthropic_api_key`, the name builds before multi-provider used, so old keys are found with no migration. Slots also include `<provider_id>_effort`. On Linux the store is the Secret Service (GNOME Keyring/KWallet/KeePassXC); with none running, values fall back to the kernel keyring for that boot only, `get_keychain_persistence` reports it and the app shows a banner (#394). `has_api_key` rejects when the keychain cannot be read — that is not the same as `false`.
- App version is canonical in `apps/desktop/src-tauri/tauri.conf.json` — the `package.json` files and Cargo's `[workspace.package] version` mirror it, and `appVersion.test.ts` fails if they drift.
- Demucs and Whisper (`crates/ml-*`) are wired in as tools but their inference is not shipped (#383–#385). Don't describe stem separation or transcription as working in docs or site copy.
- Undo/redo follow the path the user took (`apps/desktop/src/lib/headTrail.ts`, held by `useSession`), never `node.parent` alone. Node ids hash state only, so a revisited state keeps its first parent (#398). Every frontend head move must go through `setHeadLocal` (edits, agent, graph) or `resetHead` (opening a project). `GraphView` never calls `set_head_to` itself (Set as head goes through App's `onSetHead`), and anything showing session state re-reads on a `head` change, since `set_head_to` emits no event.

## CI / release

- `ci.yml` runs on push to main + PRs: fmt, clippy, cargo test, frontend build (whose `tsc -b` type-checks the app, its tests and `vite.config.ts`), vitest, and a separate `e2e (chromium)` job — Playwright against a production build of the frontend, Linux only — and a `website (test)` job (the site's vitest suite). Tauri bundle is intentionally NOT in CI (too slow); release workflows cover that.
- `auto-release.yml` fires off CI's `workflow_run` on main: tags `v<version>-dev.<run_number>` (e.g. `v0.3.0-dev.N`) and dispatches `release-dev.yml`.
- To cut a versioned release: bump the version in a `chore(release)` PR (all manifests, `Cargo.lock`, a changelog entry in `website/app/changelog/page.tsx`), merge it, then dispatch `release-dev.yml` with `tag=vX.Y.Z`, `channel=release` and the notes. Check the release is Latest, not a prerelease, and carries all six installers.
- `release-dev.yml` uses a `create-release` job + matrix to avoid the parallel-job race that produced duplicate releases for the same tag.
- `release-dev.yml` has a `channel` dispatch input: `dev` (default — prerelease, not Latest, "unsigned dev build") and `release` (a real versioned release, not a prerelease, marked Latest). Tag pushes are always `dev`; a bare `vX.Y.Z` tag deliberately does not trigger it, so `release-signed.yml` owns those tags once certs exist. Both channels emit the same install warnings, because those describe the artifact rather than the channel.
- Signed releases (`release-signed.yml`) are manual `workflow_dispatch` and require Apple/Windows signing secrets. Same `create-release` + matrix + `publish` shape as `release-dev.yml`. tauri-action is given neither `tagName` nor `releaseId` there — it only builds, and upload happens after signing, so unsigned artifacts can never reach the release.
- Bundle targets are explicit in `tauri.conf.json`: `["app", "dmg", "msi", "nsis", "deb", "appimage"]`. Don't revert to `"all"` — it silently dropped installer formats under some build conditions.

## Audio engine / WaveSurfer quirks

- Playback runs in the webview: WaveSurfer plays the rendered preview mix through an HTML media element (WebKitGTK's GStreamer on Linux). There is no native playback path — #388 removed the unused `audio-io` crate and `play_state`; the only native audio I/O is microphone capture in `crates/recorder` (cpal input).

- `wsRef.current.zoom()` throws "No audio loaded" when WaveSurfer has no decoded data — always guard with `if (!wsRef.current || duration === 0) return` and include `duration` in the useEffect dep array.
- React's `onWheel` is passive in Chromium/Tauri — Ctrl+scroll requires `el.addEventListener("wheel", handler, { passive: false })` via useEffect, not the `onWheel` JSX prop.
- Window-level `keydown` listeners do NOT stop each other via `e.stopPropagation()` — guard with a state variable (e.g., `!showShortcuts`) instead.
- Keyboard shortcut handlers in App.tsx that should fire without audio must be placed **before** `if (!timelineRef.current) return`, not after.

## Tauri command conventions

- Return type alias: `CmdResult<T>` (= `Result<T, String>`) — all commands use it; never raw `Result<T, String>`.
- Store and engine locks must be in separate scopes: acquire and drop store lock before opening engine lock to avoid double-borrow panics. Use `let state = { let store = lock_std(...); store.get(id)? };` pattern.

## Website (`website/`)

- Separate Next.js 16 app, deliberately **outside** the pnpm workspace (so Vercel builds it standalone), with its own lockfile. The package is `edytlab-website`, so `pnpm --filter` does not reach it: `cd website && pnpm install --ignore-workspace`, then `pnpm test`, `pnpm typecheck`, `pnpm build`. `pnpm lint` calls `next lint`, which Next 16 removed — use `npx eslint .`.
- Animation is GSAP (`gsap`, `@gsap/react`); framer-motion is not installed. Components that animate need the `"use client"` directive, must respect `prefers-reduced-motion`, and must render their content visible without JavaScript.
- Demo videos: `website/lib/demos.ts` lists them, the files live in `website/public/demos/` (`<slug>.mp4` + `<slug>.jpg`), and `lib/demos.test.ts` fails if the list and the folder drift.
- `crates/tools/tests/website_tool_docs.rs` checks tool names and counts in the site copy, so a tool rename or a new tool needs the site updated in the same PR.

## Acceptance gates

Before merging anything:
- `cargo fmt --all -- --check` clean
- `cargo clippy --workspace --all-targets -- -D warnings` clean
- `cargo test --workspace` passes
- `pnpm --filter @edytlab/desktop test` passes
- `pnpm --filter @edytlab/desktop test:slow-scheduler` passes — the same suite with React's scheduler 40 ms late (#349). A failure here that `test` does not show is a test asserting on async React state without waiting for it: wait on the state under test, or hold the backend's answer with `src/__tests__/held.ts`.
- `pnpm --filter @edytlab/desktop typecheck` clean — `tsc -b` over three programs: the app (no Node types, because it runs in a webview — #336), its tests (Node types, because vitest runs them in Node) and `vite.config.ts`. **Not** bare `tsc --noEmit`: the root `tsconfig.json` is a solution file with no inputs of its own, so that checks nothing and exits 0.
- `pnpm --filter @edytlab/desktop typecheck:e2e` clean
- Website changes: `pnpm test`, `pnpm typecheck` and `pnpm build` inside `website/` (see above).
- `pnpm --filter @edytlab/desktop test:e2e` passes — for anything that changes what mounts, draws, scrolls or decodes. jsdom cannot see those; `apps/desktop/e2e/` runs the real frontend in Chromium with only the IPC boundary replaced. Its fake backend answers from the Rust source (`e2e/backend.ts`), so a new command the app calls needs an answer there, taken from the command's own body.
