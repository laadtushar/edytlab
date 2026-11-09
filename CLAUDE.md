# CLAUDE.md

Project-level instructions for Claude Code sessions in this repository. These persist across sessions; treat them as durable user preferences. This file holds what every task needs; each area's design notes and rules are in a guide under `docs/agents/` (index below).

## Workflow

### Pull requests

- **Squash-merge a PR you opened once it is green** (defined below), without waiting for approval each time. A red check is a failure to investigate and fix before merging.
- Open PRs as **drafts**; flip to ready when green (squash-merge implies ready).
- One concern per PR. A follow-up with different files or a different reviewer focus is a separate PR.

### Branches

- Feature branches: `claude/feature/<short-kebab-summary>`
- Fix branches: `claude/fix/<short-kebab-summary>`
- Docs and chores: `claude/docs/<…>`, `claude/chore/<…>` (a release bump is `claude/chore/release-<version>`).
- Branch off the latest `origin/main`, not whatever the working tree happens to be on.

### Commits

- Conventional-commit prefixes: `feat:`, `fix:`, `ci:`, `chore:`, `docs:`, `test:`, `refactor:`. Scope optional (`feat(ai): ...`).
- Each commit message ends with the session footer:
  ```
  https://claude.ai/code/session_<id>
  ```
  where `<id>` is the current session id (substituted by the harness at write time).

### Research

- Browse the web freely for any research: library and API docs, platform behaviour, prior art, model pricing, bug reports upstream. Prefer primary sources (official docs, specs, source code, release notes) and cite the URLs in the PR or issue that relies on them.

## Green

A PR or a main commit is **green** when every required check on its head commit succeeded: `build (linux-x86_64)`, `build (macos-arm64)`, `build (windows-x86_64)`, `frontend (test)`, `e2e (chromium)` and `website (test)` (all from `ci.yml`). On a PR that touches only docs, site or marketing files the macOS and Windows legs skip their steps and report Success; that counts. The separate workflows (`Audit`, `Notices`, `Docs`, `ML`) are not part of green, but a red `Notices` or `Docs` run caused by your PR is yours to fix (`docs/agents/ci-release.md`). The bots below are advisory: they never block green.

## Advisory bots

| Bot | What it is | Its status |
|---|---|---|
| SonarCloud | Static analysis with a new-code quality gate | Its new-code duplication counts edited lines inside the boilerplate every tool shares, so a tool PR can fail it while adding no real duplication |
| DeepSource | Static analysis (Rust, JavaScript, Python, shell, secrets) | Per-finding. It analyses with the config stored in its dashboard, not `.deepsource.toml`: change both together (the DeepSource connector's `save_repository_config` writes the stored one) |
| FOSSA | Licence and vulnerability scan | A repo-wide baseline, red on every PR: 38 quality, 27 licence (6 until #953 re-keyed `next`'s lockfile entries), 1 vulnerability. Causes: the Remotion project under `marketing/` (FOSSA's GitHub import scans it despite `.fossa.yml`), ESLint 9's minimatch 3.x, and the licence classifications of gsap, next and tailwindcss-animate. Only a change in those counts is news |
| Gitar, CodeRabbit (and Gemini) | AI reviewers | Per-comment |

- **Verify a bot finding against the code before acting.** Fix one that holds; decline one that does not.
- A repo-wide red status (FOSSA's baseline, SonarCloud's boilerplate duplication): explain it once per PR in one comment, then stay silent on it.
- Reply to an AI review comment only to decline it (with the reason) or to name the commit that fixed it.

## Release day

Do these in order; each step is done only when its criterion holds.

1. **Release PR merges first.** The `claude/chore/release-<version>` PR bumps every manifest and `Cargo.lock`, adds the changelog entry in `website/app/changelog/page.tsx`, and moves the version's rows in the "Shipped in" table (`docs/agents/ci-release.md`). Done when it is squash-merged green.
2. **Hold other pushes while it waits on CI.** macOS runners are scarce, and every push to a PR queues another macOS leg ahead of it. Done when the release PR has merged.
3. **Main is green.** Done when the `CI` run on the merge commit succeeded.
4. **Dispatch the release.** `gh workflow run release-dev.yml --ref main -f tag=vX.Y.Z -f channel=release -f notes='…'`. Done when that run succeeded.
5. **Verify it.** `gh api repos/laadtushar/edytlab/releases/latest` names `vX.Y.Z`, `prerelease` is false, and it carries 6 assets; the release list has no new draft (a failed leg leaves one; this session cannot delete a release, so list it on #366 for the owner). Done when all hold.
6. **Merge the site and marketing PRs that describe the release** (they describe the released app, so they wait for it). Done when merged and the live site shows the version (`website/lib/releases.ts` reads GitHub, cached up to an hour).
7. **Then tool PRs, one at a time.** Merge main into the next, re-run `cargo test -p ai --test request_size -- --nocapture` and update its ratchets, recompute the tool counts (Staleness sweep), wait for green, merge. Done when the queue is empty.

## Cloud sessions

In a Linux cloud session (claude.ai/code) the checkout is a normal Linux path and Bash runs git, pnpm and cargo directly; `docs/agents/windows-local.md` applies only on the owner's machine.

- **Disk is a fixed allowance** shared by every agent in the session. Give each worktree a private `CARGO_TARGET_DIR` and build with `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0`; run one cargo build at a time across parallel agents; delete the target dir when the branch is done.
- **One worktree per branch** (`git worktree add <dir> -b <branch> origin/main`), removed once the branch merges.
- **GitHub through REST.** GraphQL is refused here, so `gh pr list` and `gh pr view` fail: use `gh api repos/laadtushar/edytlab/...`, and the `/pulls/{n}/ccr/...` routes for draft, ready and auto-merge.
- **Runner starvation** (a job queued with no runner, usually macOS): re-run the job once (`gh api -X POST repos/<owner>/<repo>/actions/jobs/<job-id>/rerun`). If it is cancelled again, dispatch a fresh run on the branch with `gh workflow run ci.yml --ref <branch>`; its checks land on the same commit under the same names (#963).

## Repo map

- Tauri 2 desktop app under `apps/desktop/`. Frontend in `apps/desktop/src/`, Rust backend in `apps/desktop/src-tauri/`.
- Architecture decisions are recorded in `docs/adr/` (one record per decision; read the record for an area before reshaping it, and supersede a record with a new one rather than editing it); domain terms are defined in `GLOSSARY.md`.
- Workspace crates under `crates/`: `ai` (LLM provider abstraction and agent loop), `tools` (the agent's audio editing tools — `docs/tools-reference.md` is generated from them), `audio-engine` (render), `audio-time` (phase vocoder), `session` (the node graph), others.
- App version is canonical in `apps/desktop/src-tauri/tauri.conf.json` — the `package.json` files and Cargo's `[workspace.package] version` mirror it, and `appVersion.test.ts` fails if they drift.

## Area guides

Each guide in `docs/agents/` holds one area's design notes and rules. Read it before changing that area.

- Running git, pnpm or cargo on the owner's Windows machine → `docs/agents/windows-local.md`
- Touching a provider, the keychain, a provider slot, the slim tool set or the session block → `docs/agents/providers.md`
- Touching the ChatGPT plan provider or its sign-in → `docs/agents/chatgpt.md`
- Touching transcription, diarization, stem separation, a model download or an ONNX Runtime session → `docs/agents/ml-models.md`
- Touching which files import (decoders, M4A, Opus) → `docs/agents/formats.md`
- Touching how project audio is named, written or deleted, a writer's destination, clip frames, envelopes or what a render reports → `docs/agents/audio-store.md`
- Touching playback, recording, latency calibration or punch-in → `docs/agents/recording.md`
- Touching undo/redo, the head trail, lane controls, the window's mix, WaveSurfer or dialogs → `docs/agents/frontend-state.md`
- Adding or documenting a Tauri command, taking a lock, the plan gate, opening a store, thinking-block replay or bundled skills → `docs/agents/tauri-conventions.md`
- Adding or changing an agent tool → `docs/agents/agent-tools.md`
- Touching a workflow, a release, a dependency's notices, the ML workflow, the Mintlify docs, or whether a feature has shipped → `docs/agents/ci-release.md`
- Touching `website/`, a demo or a video → `docs/agents/website.md`

## Acceptance gates

Before merging anything:
- `cargo fmt --all -- --check` clean
- `cargo clippy --workspace --all-targets -- -D warnings` clean
- `cargo test --workspace` passes
- `pnpm --filter @edytlab/desktop test` passes
- `pnpm --filter @edytlab/desktop test:slow-scheduler` passes — the same suite with React's scheduler 40 ms late (#349). A failure here that `test` does not show is a test asserting on async React state without waiting for it: wait on the state under test, or hold the backend's answer with `src/__tests__/held.ts`.
- `pnpm --filter @edytlab/desktop typecheck` clean — `tsc -b` over three programs: the app (no Node types, because it runs in a webview — #336), its tests (Node types, because vitest runs them in Node) and `vite.config.ts`. **Not** bare `tsc --noEmit`: the root `tsconfig.json` is a solution file with no inputs of its own, so that checks nothing and exits 0.
- `pnpm --filter @edytlab/desktop typecheck:e2e` clean
- Website changes: `pnpm test`, `pnpm typecheck` and `pnpm build` inside `website/` (`docs/agents/website.md`).
- `pnpm --filter @edytlab/desktop test:e2e` passes — for anything that changes what mounts, draws, scrolls or decodes. jsdom cannot see those; `apps/desktop/e2e/` runs the real frontend in Chromium with only the IPC boundary replaced. Its fake backend answers from the Rust source (`e2e/backend.ts`), so a new command the app calls needs an answer there, taken from the command's own body.

## Staleness sweep

Facts in this repo are copied by hand into many files, and most copies have no test. Run this sweep at the end of every working day, and for any trigger below inside the PR that causes it. Where a test already guards a copy, it is named, so the sweep covers the unguarded ones. A count is computed at the time of writing, never copied from another file.

**Every day, before stopping:**
- Tracking issue #366: rewrite it to the current state (released, in flight, queued, blocked, needs the owner) and bump its "Last updated" date.
- Issues: confirm merged PRs closed what they fixed (a "Refs" PR leaves its issue open on purpose), post a plan comment on anything paused, and file what the day turned up.
- Leftovers: remove worktrees of merged branches, list any `claude/backup/*` branch in #366 until its work lands, and scan every diff for API keys (`sk-ant-`, `sk_`) before pushing.

**When a change lands, update its copies in the same PR:**
- A tool added, renamed or removed: regenerate `docs/tools-reference.md` (`UPDATE_TOOLS_REFERENCE=1 cargo test -p tools --test tools_reference_doc`); its first line states the tool count. `website_tool_docs.rs` guards the site, `README.md` and `docs/` (a count within three words of "tools"). The `marketing/` counts are not guarded: `marketing/press-kit.md`, `marketing/awesome-lists.md`, `marketing/launch/*`, `marketing/social/*`, `marketing/social-kit/**` (the captions, posts and image notes; marketing describes the released app, so these move with the release).
- A crate added, removed or renamed: the `README.md` crate list and architecture box, the workspace layout in `docs/architecture.md`, its mirror in `website/app/docs/developer-guide/page.tsx`, the Repo map above, and the hand-written crate counts in `marketing/social-kit/more.md` and `marketing/social-kit/reddit.md` ("a Rust workspace of N crates"; they too move with the release).
- A Tauri command added or changed: a row in `docs/api-reference.md` and in `website/lib/docs/api-data.ts`, and the hand-written command counts; both are in "Documenting a command" in `docs/agents/tauri-conventions.md`.
- An effect kind starts to stream, a chain tool gains or loses a scope, or what a render reports changes: the five kinds are copied into `docs/architecture.md` ("What the Render Processes"), "Effects a render cannot run" in `docs/agents/audio-store.md`, the site's user guide ("Effects, Buses and the Master") and tools page, and `marketing/README.md`; the render tools' outputs are on the tools page.
- A provider or keychain slot added or changed: `README.md`, `docs/agents/providers.md`, `docs/development-guide.md`, and the site's getting-started, user-guide and privacy pages.
- A provider ships a new model generation or retires one: the default in `crates/ai/src/prompt.rs` (`DEFAULT_MODEL`), the curated Anthropic list in `crates/ai/src/models.rs`, `CLASSIFIER_MODEL` in `crates/ai/src/lib.rs`, the OpenAI ranking in `models.rs`, and every doc or site line that names a model. Check ids against the provider's own models endpoint or docs; a retired id answers 404.
- A caveat resolved (something documented as not shipped or unused, or as lost on reboot, as #394 was): `git grep` the issue number and rewrite every line that cites it, `docs/agents/` included.
- A new outbound network request from the app (a downloader, an updater, a new provider host, a lookup such as a provider's model list): the "Network traffic" and "Updates" sections of `website/app/privacy/page.tsx`, the local-first table on `website/app/use-cases/local-ai-audio-editor/page.tsx`, and the privacy lines in `marketing/press-kit.md`.
- A UI change: the site's user guide section that describes it, the `apps/desktop/e2e-native` stories that drive it (grep them for `"class"`, colours and `data-` attributes: a restyle breaks a story that reads a class name, which CI cannot see because the native suite is not in it), and any screenshot or recording that shows it: `docs/brand/`, the demo videos under `website/public/demos/`, `marketing/reels/`, `marketing/launch-video/`, and the LinkedIn screenshots and renders in `marketing/social-kit/linkedin/` (`shots/` and `out/`; `node render.mjs --shots` retakes and re-renders them).
- A new site page, a site page's content changed, a demo re-recorded, or a reel or the launch video re-cut: "Site pages" and "Videos" in `docs/agents/website.md`.

**When a release ships:** Release day above, then update version mentions in `marketing/`.
