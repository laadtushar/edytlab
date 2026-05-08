# edytlab documentation

This directory holds long-form documents that don't belong in the top-level [`README.md`](../README.md). Marketing copy lives in `/website`; project-level workflow rules live in [`CLAUDE.md`](../CLAUDE.md).

## What's here

| File | What it covers |
|---|---|
| [`specs/2026-05-05-conversational-audio-editor-design.md`](specs/2026-05-05-conversational-audio-editor-design.md) | Canonical product + architecture spec. Problem, goals/non-goals, locked decisions, lighthouse scenarios, component breakdown, session-graph data model, tool surface, AI agent design, build phases, error handling, testing strategy, open questions, risks. Start here. |
| [`HANDOVER.md`](HANDOVER.md) | Standalone product one-pager and phase recap, written as a kickoff prompt for a fresh Claude Code session. Useful as a short brief; superseded by the spec for any detail. |
| [`packaging-windows.md`](packaging-windows.md) | Windows packaging notes — WebView2 bootstrapper choice, Authenticode signing, required GitHub secrets, SmartScreen reputation reality check. Read before touching `release-win.yml`. |

## Conventions

- **Specs** go under `docs/specs/` and are dated (`YYYY-MM-DD-<slug>.md`). They are append-only history; supersede an old spec by writing a new one and linking back, don't rewrite in place.
- **Operational notes** (packaging, signing, CI gotchas) live at the top level of `docs/`.
- **What does NOT belong here:** API reference (generated from rustdoc / TypeScript), screenshots and copy for the marketing site (those live in `/website`), and per-PR change notes (use commit messages and the PR body).
