# edytlab documentation

Long-form technical documentation for contributors and integrators. Marketing copy lives in `/website`; workflow rules live in [`CLAUDE.md`](../CLAUDE.md).

## Documents

| Document | What it covers |
|----------|---------------|
| [**architecture.md**](./architecture.md) | Full system design: workspace layout, AI subsystem, tool dispatch, session DAG, audio engine, ML pipeline, IPC, security model, data flow, extension points. |
| [**development-guide.md**](./development-guide.md) | Dev setup for macOS/Windows/Linux, daily workflow, testing, debugging, building for release, CI pipeline, common issues, acceptance gate. |
| [**api-reference.md**](./api-reference.md) | Every Tauri command and TypeScript bridge function: signature, parameters, return type, error conditions, examples. |
| [**tools-reference.md**](./tools-reference.md) | All 28 audio-editing tools the agent can call: input schema, output, notes, prompt tips. |
| [**contributing.md**](./contributing.md) | PR workflow, code style, commit conventions, how to add a tool/provider/component, test requirements. |
| [**specs/2026-05-05-conversational-audio-editor-design.md**](specs/2026-05-05-conversational-audio-editor-design.md) | Original product + architecture spec. Canonical product reference. |
| [**HANDOVER.md**](./HANDOVER.md) | One-page product brief and phase recap. |
| [**packaging-windows.md**](./packaging-windows.md) | Windows signing, WebView2 bootstrapper, Authenticode, SmartScreen reputation. Read before touching `release-win.yml`. |

## Quick Links

- Adding a tool → [contributing.md §6](./contributing.md#6-adding-a-new-audio-tool)
- Adding a provider → [contributing.md §7](./contributing.md#7-adding-a-new-llm-provider)
- All Tauri commands → [api-reference.md](./api-reference.md)
- Understanding the DAG → [architecture.md §6](./architecture.md#6-session-graph-dag)
- CI not passing → [development-guide.md §10](./development-guide.md#10-common-issues-and-fixes)
- Test commands → [development-guide.md §5](./development-guide.md#5-running-tests)

## Conventions

- Specs go under `docs/specs/` dated `YYYY-MM-DD-<slug>.md`. Append-only — supersede by writing a new one and linking back.
- Operational notes (packaging, signing, CI gotchas) live at the top level of `docs/`.
- Auto-generated docs (rustdoc, .d.ts) are not committed — run `cargo doc --workspace --open` locally.
