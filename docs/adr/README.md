# Architecture decision records

One file per decision that a future change could otherwise reopen by accident. Each says what was
decided, why, what was considered and rejected, and how we will know it worked. A record is never
edited to say something else: a later decision that replaces it is a new record that names the old
one as superseded.

They came out of the architecture review of 2026-10-22, which looked for shallow modules (an
interface nearly as wide as the code behind it) in the parts of the code that kept breaking. The
review's vocabulary: a **module** has an **interface** and an **implementation**; it is **deep**
when a small interface hides a lot; a **seam** is where one module can be swapped for another
(an **adapter**); **locality** means the rules for one concept live in one place; **leverage**
means one interface serves many callers.

**Read the Built column before you read a record as a description of the code.** A record is
"Accepted" when the decision is made, not when it is built. A record marked **Not yet** describes a
target: the names it introduces do not exist, and the code it wants to replace is what you will
find. Checked against `main` on 2026-10-27; when you build (or finish building) a decision, change
its row in the same PR. The records themselves are not edited (see above).

| # | Decision | Status | Built | Read before touching |
|---|---|---|---|---|
| [0001](0001-session-state-in-the-window.md) | The window changes the session through one module, built on TanStack Query | Accepted | Not yet. No `@tanstack/react-query` in `apps/desktop/package.json`; `lib/sessionQueue.ts` and the two scans it names still exist; the first step, the Accept B fix, landed (`App.sessionMoves.test.tsx`) | `apps/desktop/src/hooks/useSession.ts`, `lib/sessionQueue.ts`, `lib/ownedControls.ts`, `lib/headTrail.ts`, the session handlers in `App.tsx` |
| [0002](0002-plan-gate-is-a-oneshot.md) | The plan gate is one value built on a oneshot channel | Accepted | Yes. `ai::PlanGate` in `crates/ai/src/plan_gate.rs`, held by `Agent`, `AppState` and the CLI | `crates/ai/src/plan_gate.rs`, `approval.rs`, `approve_plan` and `reject_plan` in `commands.rs` |
| [0003](0003-rendering-a-node.md) | One module renders a node and returns what it left out | Accepted | Not yet. No `render` module in `crates/tools`, no `LeftOutEffect` or `Rendered`; `cached_mix` is still in `commands.rs`, and the left-out list still comes in three forms | `crates/tools/src/tool/render_preview.rs` and `render_final.rs`, `preview_cache.rs`, `rederive.rs`, `render_preview`, `prepare_compare` and `render_range` in `commands.rs`, `crates/audio-engine/src/effect_chain.rs` |
| [0004](0004-time-edits.md) | A time edit is a pure function over the session state | Accepted | Not yet. No `time_edit` module or `TimeEdit`; the helpers it replaces are still in `crates/tools/src/tool/util.rs`, and `proptest` is not a dependency of `tools` | `crates/tools/src/tool/util.rs` (`sync_other_tracks`, `remap_after_*`, `cut_annotations`) and the tools that move time: `cut_range`, `trim`, `insert_silence`, `paste_region`, `time_shift`, `time_stretch`, `truncate_silence` |
| [0005](0005-provider-settings.md) | Provider settings are saved in one call by one module | Accepted | Not yet. No `agent_settings` module or `save_provider_settings` command; the per-field commands remain, and only the `KeychainWrites` trait it keeps exists. 0009 moved where the values are stored, not who saves them | `set_api_key_for`, `set_base_url_for`, `set_effort_for`, `set_active_provider`, `set_active_model` and `rebuild_agent_*` in `commands.rs`, `state.rs`, `crates/ai/src/keychain.rs`, the Settings dialog |
| [0006](0006-the-open-project.md) | The open project is one module that owns its locks | Accepted | Not yet. No `open_project` module (only the command of that name); commands still take `store_handle()` and `lock_std(` themselves. The pieces it would sit on exist: `LiveStore`, `AppState::set_open_project`, `release_replaced_project`, `window_mix.rs` | `apps/desktop/src-tauri/src/commands.rs` (`open_project`, `release_replaced_project`), `state.rs`, `window_mix.rs`, `compare.rs`, `lib.rs` setup, `crates/session/src/live.rs` |
| [0007](0007-a-turn.md) | A turn is split at the model exchange, behind a port with two adapters | Accepted | Not yet. No `ModelExchange` or scripted model; `run_turn` is still one function with `#[allow(clippy::too_many_arguments)]` and still matches on `wire_format()`. Its prerequisite, 0002, is built | `crates/ai/src/agent_loop.rs`, `provider.rs`, `anthropic.rs`, `responses.rs`, `chatgpt/provider.rs`, `tests/prior_turn_thinking.rs`, `tests/request_size.rs` |
| [0008](0008-project-audio-files.md) | The project's own audio files are one content-addressed store | Accepted, deferred | Not yet, waits on the stem writers (#385). No store module in `crates/session`; naming, writing, resolving and sweeping are still spread over `provenance.rs`, `reclaim.rs`, `rederive.rs`, `repair.rs`, `util.rs` and `session::relocate` | `crates/tools/src/provenance.rs`, `reclaim.rs`, `rederive.rs`, `repair.rs`, `destination.rs`, `crates/session/src/relocate.rs` (with its `Rebinder`), `store.rs` |
| [0009](0009-settings-and-secrets-storage.md) | Secrets are one keychain item, and settings are a file | Accepted | Yes. `secrets` item and `provider-settings.json` in `crates/ai/src/keychain.rs` (`SECRETS_ACCOUNT`, `MIGRATED_MARKER`, `SETTINGS_FILE`, `Field::is_secret`, the `Legacy` layout) | `crates/ai/src/keychain.rs`, `keychain_layout_tests.rs`, `apps/desktop/src-tauri/src/lib.rs` (`set_settings_dir`), `chatgpt.rs` (the token store) |

Terms are defined in [`GLOSSARY.md`](../../GLOSSARY.md).
