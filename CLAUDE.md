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

### Research

- Browse the web freely for any research: library and API docs, platform behaviour, prior art, model pricing, bug reports upstream. Prefer primary sources (official docs, specs, source code, release notes) and cite the URLs in the PR or issue that relies on them.

## Repo specifics

- Tauri 2 desktop app under `apps/desktop/`. Frontend in `apps/desktop/src/`, Rust backend in `apps/desktop/src-tauri/`.
- Architecture decisions are recorded in `docs/adr/` (one record per decision; read the record for an area before reshaping it, and supersede a record with a new one rather than editing it); domain terms are defined in `GLOSSARY.md`.
- Workspace crates under `crates/`: `ai` (LLM provider abstraction and agent loop), `tools` (the agent's audio editing tools — `docs/tools-reference.md` is generated from them), `audio-engine` (render), `audio-time` (phase vocoder), `session` (the node graph), others.
- App version is canonical in `apps/desktop/src-tauri/tauri.conf.json` — the `package.json` files and Cargo's `[workspace.package] version` mirror it, and `appVersion.test.ts` fails if they drift.

## Area guides

Each guide in `docs/agents/` holds the design notes and rules for one area. Read the guide before changing its area.

- Running git, pnpm or cargo on the owner's Windows machine → `docs/agents/windows-local.md`
- Touching a provider, the keychain, a provider slot, the slim tool set or the session block → `docs/agents/providers.md`
- Touching the ChatGPT plan provider or its sign-in → `docs/agents/chatgpt.md`
- Touching transcription, diarization, stem separation, a model download or an ONNX Runtime session → `docs/agents/ml-models.md`
- Touching which files import (decoders, M4A, Opus) → `docs/agents/formats.md`
- Touching how project audio is named, written or deleted, a writer's destination, clip frames, envelopes or what a render reports → `docs/agents/audio-store.md`
- Touching playback, recording, latency calibration or punch-in → `docs/agents/recording.md`
- Touching undo/redo, the head trail, lane controls, the window's mix, WaveSurfer or dialogs → `docs/agents/frontend-state.md`
- Adding a Tauri command, taking a lock, the plan gate, opening a store, thinking-block replay or bundled skills → `docs/agents/tauri-conventions.md`
- Adding or changing an agent tool → `docs/agents/agent-tools.md`
- Touching a workflow, a release, a dependency's notices, the ML workflow or the Mintlify docs → `docs/agents/ci-release.md`
- Touching `website/` → `docs/agents/website.md`

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

## Staleness sweep

Facts in this repo are copied by hand into many files, and most copies have no test. Run this sweep at the end of every working day, and for any trigger below inside the PR that causes it. Where a test already guards a copy, it is named, so the sweep covers the unguarded ones.

**Every day, before stopping:**
- Tracking issue #366: rewrite it to the current state (released, in flight, queued, blocked, needs the owner) and bump its "Last updated" date.
- Issues: confirm merged PRs closed what they fixed (a "Refs" PR leaves its issue open on purpose), post a plan comment on anything paused, and file what the day turned up.
- Leftovers: remove worktrees of merged branches, list any `claude/backup/*` branch in #366 until its work lands, and scan every diff for API keys (`sk-ant-`, `sk_`) before pushing.

**When a change lands, update its copies in the same PR:**
- A tool added, renamed or removed: regenerate `docs/tools-reference.md` (`UPDATE_TOOLS_REFERENCE=1 cargo test -p tools --test tools_reference_doc`); the site copy is guarded by `website_tool_docs.rs`. The hand-written "110 tools" counts are not: `README.md` (architecture box and crate list), `docs/README.md`, `docs/architecture.md`, `marketing/press-kit.md`, `marketing/awesome-lists.md`, `marketing/launch/*`, `marketing/social/*`, `marketing/social-kit/**` (the captions, posts and image notes; marketing describes the released app, so these move with the release).
- A crate added, removed or renamed: the `README.md` crate list and architecture box, the workspace layout in `docs/architecture.md`, its mirror in `website/app/docs/developer-guide/page.tsx`, "Repo specifics" above, and the hand-written crate counts in `marketing/social-kit/more.md` and `marketing/social-kit/reddit.md` ("a Rust workspace of N crates"; they too move with the release).
- A Tauri command added or changed: `docs/api-reference.md` and `website/lib/docs/api-data.ts` (the fake backend in `e2e/backend.ts` is already an acceptance gate). The site's API reference is one page per area, all made from that file (`/docs/api-reference/[area]`; a new area is an object in `apiAreas` plus a `PAGE_UPDATED` date, no page file), and lists every command, one row each, and so does the md. The data is guarded by `website/lib/apiReferencePage.test.ts` (each name in `generate_handler!` in `lib.rs`, camel-cased, has a row, and no row outlives its command); the md is checked against the bridge's exports by `apiReferenceCoverage.test.ts`, not against `lib.rs`. A changed return type or error is edited in both. The hand-written command count (`grep -c '#\[tauri::command' apps/desktop/src-tauri/src/commands.rs`, 112 on 2026-11-02) is not guarded: `docs/architecture.md` (the "~N commands" box), `docs/development-guide.md` ("~N of them") and `website/app/docs/developer-guide/page.tsx` all say about 110, which holds until the count passes 115 or drops below 105 (the recording-latency commands took it past 105, so all three copies moved to a new round number), and `docs/saas-evaluation.md` gives the exact count with its async and plain split and the line count of `commands.rs`, so it moves with every command.
- An effect kind starts to stream, a chain tool gains or loses a scope, or what a render reports changes: the five kinds are copied into `docs/architecture.md` ("What the Render Processes"), the Audio engine bullet above, the site's user guide ("Effects, Buses and the Master") and tools page, and `marketing/README.md`; the render tools' outputs are on the tools page.
- A provider or keychain slot added or changed: `README.md`, the keychain bullet above, `docs/development-guide.md`, and the site's getting-started, user-guide and privacy pages.
- A provider ships a new model generation or retires one: the default in `crates/ai/src/prompt.rs` (`DEFAULT_MODEL`), the curated Anthropic list in `crates/ai/src/models.rs`, `CLASSIFIER_MODEL` in `crates/ai/src/lib.rs`, the OpenAI ranking in `models.rs`, and every doc or site line that names a model. Check ids against the provider's own models endpoint or docs; a retired id answers 404.
- A caveat resolved (something documented as not shipped or unused, such as #383–#385, or as lost on reboot, as #394 was): `git grep` the issue number and rewrite every line that cites it, here included.
- A new outbound network request from the app (a downloader, an updater, a new provider host, a lookup such as a provider's model list): the "Network traffic" and "Updates" sections of `website/app/privacy/page.tsx`, the local-first table on `website/app/use-cases/local-ai-audio-editor/page.tsx`, and the privacy lines in `marketing/press-kit.md`.
- A UI change: the site's user guide section that describes it, the `apps/desktop/e2e-native` stories that drive it (grep them for `"class"`, colours and `data-` attributes: a restyle breaks a story that reads a class name, which CI cannot see because the native suite is not in it), and any screenshot or recording that shows it: `docs/brand/`, the demo videos under `website/public/demos/`, `marketing/reels/`, `marketing/launch-video/`, and the LinkedIn screenshots and renders in `marketing/social-kit/linkedin/` (`shots/` and `out/`; `node render.mjs --shots` retakes and re-renders them).
- A new site page: `website/app/sitemap.ts`, plus `lib/seo.test.ts` and `lib/marketing-pages.test.ts`.
- A site page's content changes (or a page is added): set or bump its date in `PAGE_UPDATED` (`website/lib/seo-core.ts`), taken from `git log -1 --format=%cs -- <the page's source files>`. It is the sitemap's `lastmod` and the `dateModified` of the page's `Article` data. Move it for a change to what the page says, never to look fresh; `SITE_UPDATED` is only the fallback, and `seo.test.ts` fails when a page has no date of its own.
- A demo re-recorded, or a reel or the launch video re-cut: a re-cut is copied to `website/public/videos/` too, with its `lib/videos.ts` entry (`seconds`, poster frame) and, for the launch video, its WebVTT captions; `lib/videos.test.ts` fails while a copy differs from its source under `marketing/`. For a demo: `website/lib/demos.ts` (guarded by `lib/demos.test.ts`) and the hand-written durations in the `README.md` demo list, `website/app/press/page.tsx`, `website/lib/blog-posts-*.ts`, `marketing/press-kit.md`, `marketing/launch/show-hn.md` and `marketing/social/x-thread.md`. The reels in `marketing/reels/` are cut from those videos at measured times (`marketing/reels/tools/spec.py`), so rebuild them too. Nothing is deleted: every video, caption, contact sheet, cue or timing file a re-recording or re-cut replaces moves to `marketing/archive/` in the same PR (layout in its README).

**When a release ships:** follow "CI / release" above, then check the live site shows the new version (`website/lib/releases.ts` reads GitHub, cached up to an hour), update version mentions in `marketing/`, and check the release list has no new draft (a failed release leg leaves one). #387's 45 old drafts were deleted on 2026-10-25; a new one goes on #366 for the owner to delete, since this session gets HTTP 403 deleting a release.
