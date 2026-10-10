# Running edytlab as a browser-based SaaS — evaluation

> **Status:** evaluation for [#389](https://github.com/laadtushar/edytlab/issues/389). Claims about the code are pinned to main at `e75f094`, 2026-10-10. Nothing in this document is built, and none of it changes what the desktop app or the site says. The decisions that belong to the owner are collected in [§10](#10-decisions-needed-from-the-owner).

#389 asks whether edytlab can run as a browser-based SaaS and what it would take. This document answers with what the code says today, what each way of doing it costs, what a hosted version would have to give up, and an order of work.

## Verdict

- **Feasible.** The frontend already reaches the backend through one file, the Rust crates already run without Tauri (`apps/cli` is a headless host), and renders are deterministic.
- **Recommended path: A, growing into C.** A server runs the Rust core behind HTTP and SSE with the same React UI ([§3](#3-options-compared)). A browser-side preview renderer is added later if latency demands it.
- **The first step is shared by every option except D:** move command bodies into a crate with no Tauri types, and put a `Transport` interface behind `apps/desktop/src/lib/tauri-bridge.ts` ([§4](#4-recommended-architecture-a-then-c)).
- **The hard parts are two.** `AppState` is one user's state and has to become one workspace per user and project ([§1](#1-what-ties-edytlab-to-the-desktop-today)). And a hosted mode cannot keep the local-first promise, so it has to be a separate, honestly named product surface ([§6](#6-privacy-and-local-first)).
- **The safety work comes before any other user's input reaches a server:** close remote code execution (MCP stdio), server-side request forgery (base URLs, remote MCP) and path escape ([§7](#7-security-risks-of-hosting)).

## How to read the numbers

- A figure with a source (a path or an issue) is something the repository states or that was measured there.
- A figure marked **estimate** is arithmetic on stated inputs, or comes from the issue's table and was not re-derived here.
- Third-party facts and prices are as researched on 2026-10-07 in #389, and are not re-checked here. **Verify before deciding.**
- Anything not checked is listed in [§9](#9-unverified).

---

## 1. What ties edytlab to the desktop today

| Concern | Where | Consequence on the web |
|---|---|---|
| **Transport seam** | 88 commands in `generate_handler!` (`apps/desktop/src-tauri/src/lib.rs`), all defined in `commands.rs` (5,240 lines): 79 plain `#[tauri::command]` and 9 `#[tauri::command(async)]`. The frontend reaches them only through `apps/desktop/src/lib/tauri-bridge.ts`, which uses `invoke`, plus `listen` for the `agent://*` events (`src-tauri/src/events.rs`), `tool-progress` (emitted from `lib.rs`) and `marker-changed` (emitted from `commands.rs`). | An HTTP transport replaces `invoke` and `listen`. The bridge's typed functions do not change. |
| **Direct Tauri calls outside the bridge** | Six imports in four files. `convertFileSrc` in `components/Timeline.tsx` (each track's lane audio, and the mix) and `components/AuditionPlayer.tsx`. The `open` dialog and `getCurrentWebview().onDragDropEvent` in `lib/file-open.ts`. `listen("menu://open-file")` and the `save()` dialog (Export Selection) in `App.tsx`. | These are the only `@tauri-apps` imports outside the bridge, so they are the places in the UI the port has to touch. Each needs an equivalent: a file URL, an upload, a download, an in-page "open" action in place of the native menu. |
| **Process-wide, single-user state** | `src-tauri/src/state.rs` `AppState` holds one of each: agent (a `tokio` mutex held across a whole turn), store, engine, dispatcher, project directory, API key and active provider, plan gate, selection, clipboard, memory, skills, agent profiles and the MCP registry. Recording has its own `RecorderState`. | One process serves one user. A server needs one `Workspace` per user and project, looked up per request. |
| **Process globals in `tools`** | `crates/tools/src/progress.rs`: `static SINK: OnceLock` and `static CANCELLED: AtomicBool`. The module's own comment says cancellation is process-wide because "there is one user and one foreground batch". | On a shared server every tenant would receive every tenant's progress events, and one tenant's cancel would stop another's `batch_apply`. |
| **Lock scope** | `crates/ai/src/agent_loop.rs` ("Dispatch under a single lock acquisition") holds the dispatcher, store, engine and clipboard guards for the whole of a tool call. Making the commands async (step 1 of #421) landed; releasing the lock during DSP (step 2) is open. | With one shared dispatcher and store, every tenant's tool runs one at a time. Per-workspace state removes the cross-tenant effect. Step 2 of #421 is still needed for a single tenant's own UI. |
| **Disk layout** | A project is a directory. `crates/session/src/store.rs` writes `.audiograph/` with `nodes/` sharded by id, a `head` file, and beside them `derived/`, `clipboard/` and `previews/` (the last owned by `crates/tools/src/preview_cache.rs`). `.audiograph/view.json` records where you were, and `project.json` sits beside `.audiograph/` (`src-tauri/src/project.rs`). The store states a single-writer assumption. Global state lives in `~/.edytlab/`: `memory.md`, `skills/`, `agents/`, `mcp.json`, `recents.json` (`src-tauri/src/project.rs`). A default project lives in the app data dir (`lib.rs` setup). | Everything is a path on a local filesystem, read through `std::fs`. There is no storage abstraction. Object storage therefore needs a local working copy anyway ([§4](#4-recommended-architecture-a-then-c)). The global directory has to become per tenant. |
| **Absolute paths everywhere** | Commands: `open_project`, `batch_load`, `save_project_as`, `forget_recent_project`, `stop_recording` (`output_path`) and `render_range` (`out_path`). `render_preview` and `prepare_compare` return paths that the frontend feeds to `convertFileSrc`. Agent tools that take paths: `load`, `render_final`, `export_multiple`, `export_recipe`, `apply_recipe`, `batch_apply`, `analyze_track`, `punch_in` (`take_path`) and the two ML stubs (`docs/tools-reference.md`). | The model chooses those tool arguments, so path confinement is a prompt-injection boundary and not only an API one ([§7](#7-security-risks-of-hosting)). Paths in results need to become opaque ids or signed URLs. |
| **Keys** | The OS keychain through `keyring` (`crates/ai/src/keychain.rs`). Slots are `<provider>_api_key`, `<provider>_model`, `<provider>_base_url`, and `active_provider`. MCP server `env` values can reference keychain slots as `<keychain:slot>` (`crates/mcp/src/config.rs`). On Linux the keychain is the kernel keyring, which does not survive a reboot (#394). | A hosted mode needs its own key store ([§6](#6-privacy-and-local-first)). |
| **Native or unsafe on a server** | `crates/recorder` uses `cpal` for microphone capture. Since #466 it is the only crate that does. `ort` is built with `load-dynamic` in the three `ml-*` crates, and the inference behind them is a stub (#383–#385). MCP stdio spawns any `command` with `args` and `env` from `mcp.json` (`crates/mcp/src/transport.rs`). `install_plugin` downloads a `github:` repository's `main` branch and copies its skills and agent profiles into `~/.edytlab/`; MCP servers it declares are registered disabled (`register_plugin_mcp_servers`). `set_base_url_for` accepts any `http://` or `https://` URL with a host and sends that provider's traffic, key included, there. The MCP HTTP transport (`crates/mcp/src/http.rs`) posts to a configured URL. | Stdio MCP is remote code execution on a shared host. Base URLs and remote MCP are server-side request forgery. Recording cannot use the server's microphone. The ML crates add a native dependency for features that do not work yet. |

Two points the table does not make:

- `ToolContext` is built by struct literal at a large number of sites (`progress.rs` says 112 when written; a search for `ToolContext {` finds about 140 today, definition and tests included). Putting per-workspace progress on it is a wide, mechanical diff. [§4(b)](#4-recommended-architecture-a-then-c) gives an alternative.
- Playback is not on this list. Since #466 removed the native playback path (`crates/audio-io` and `play_state`), all playback is WaveSurfer on an HTML media element in the webview, so there is no native audio output to replace.

## 2. What already favours a web version

- **One seam, already proven.** `apps/desktop/e2e/harness.ts` runs the unmodified production frontend in plain Chromium with `mockIPC`, and replaces `convertFileSrc`. `apps/desktop/e2e/vite.config.ts` serves audio at `FILE_ROUTE` (`e2e/routes.ts`) the way Tauri's asset protocol does: confined to a root (403 outside it), single byte ranges, 416 for unsatisfiable ranges, and Tauri's 1000 KiB cap per reply. That is a working prototype of web audio serving, and its confinement rule is the one the real route needs.
- **The agent stack already runs without Tauri.** `apps/cli` (`edytlab-cli`) wires `ai::Agent`, `tools::ToolDispatcher`, `session::Store` and `audio_engine::Engine` and runs a turn headless, with the key taken from a flag or the environment and never the keychain. `Agent::turn` reports through `on_event`, an `FnMut(AgentEvent)` sink (`crates/ai/src/agent_loop.rs`), which maps directly onto an SSE stream. What is missing is the layer between the crates and Tauri: `commands.rs` and `AppState`.
- **Deterministic renders.** `crates/audio-engine/src/render.rs` ("Determinism invariant") fixes sample, track and effect order, with no parallel reductions. `NodeId` is a blake3 hash of the session state, so previews are cacheable by node id and re-derivable byte for byte (`crates/tools/src/preview_cache.rs`). On a server, previews belong on ephemeral disk, not in object storage: a lost entry is recomputed. The same property would let a future browser renderer be tested against the server one, if bit-identity holds across targets ([§9](#9-unverified)).
- **A graph that suits object storage.** Nodes are immutable and content-addressed, and written before `head` moves (`crates/session/src/store.rs`). Only `head` (and the small `view.json` and `project.json`, which the app tolerates losing or finding corrupt) is mutable, so it needs a conditional write (compare-and-swap) or a lease, not a distributed lock.
- **Playback is already in the webview**, so a browser needs no new audio path, only a URL the media element can fetch with range requests.
- **LLM traffic is text.** Every provider sits behind the `LlmProvider` trait (`crates/ai/src/provider.rs`), and requests carry messages and tool results, never audio.
- **Likely small WASM candidates** (unverified): `crates/audio-dsp` has no dependencies, and `crates/audio-time` depends on `realfft`, `thiserror` and `tracing`.

## 3. Options compared

The table is the issue's, kept as written. Efforts, monthly costs (about 100 users, LLM tokens excluded) and risks are **estimates from 2026-10-07 that this document does not re-derive.**

| Option | Effort | ~Monthly | UX | Biggest risk |
|---|---|---|---|---|
| **A.** Rust core behind axum HTTP/SSE, same React UI | 4–8 wk | $50–300 | High | Multi-tenant refactor of `AppState`; DSP CPU on the server |
| **B.** Full WASM in the browser (OPFS storage, onnxruntime-web/WebGPU) | 8–16+ wk | $0–50 | Medium–high | Unverified crate ports (encoders), Safari OPFS eviction, model download size |
| **C.** Hybrid: UI and WASM preview in the browser; storage, agent/LLM proxy and GPU ML on a server | 6–12 wk | $50–500 | High | Two runtimes to keep in sync; GPU cold starts |
| **D.** Stream the desktop app (AppStream, Kasm) | 1–2 wk | $10–200 per concurrent user | Low | Latency and audio round trip; cost per seat |

**A — server-side core.**
- *Changes:* the core crate, the frontend `Transport`, an axum server, tenancy, object storage, auth and a key store ([§4](#4-recommended-architecture-a-then-c)).
- *Stays:* every crate under `crates/`, the React components, and the render and graph semantics.
- *Privacy:* audio, edits and chat live on our infrastructure. The local-first promise does not hold for this mode ([§6](#6-privacy-and-local-first)).
- *Unverified:* DSP throughput per core in a release build ([§9](#9-unverified)).

**B — everything in the browser.** Closest to today's promise, but the most work, because compiling the crates is the small part:
- The render path, the decoder and `Store` read and write through `std::fs` and `std::path` (for example `hound` file readers in the streaming mix, and the O(N) node directory scan in `Store::list_nodes`). The browser has no such filesystem, so B needs a storage layer under all of them (OPFS behind a virtual filesystem, or a storage trait added through the crates).
- `crates/tools` depends on `ml-demucs`, `ml-pipeline` and `ml-whisper`, which use `ort` with `load-dynamic`. That must be put behind a feature before a WASM build, and the ML itself is a stub today.
- The agent loop uses `tokio` with `rt-multi-thread` and `reqwest` (`crates/ai/Cargo.toml`), so running it in the browser means a WASM port or a TypeScript rewrite.
- Keys live in the browser, with weaker protection than an OS keychain.
- The decoders and encoders (`symphonia`, `hound`, `flac-codec`, `rusty_mp3`) and `rubato` are unverified for `wasm32` ([§9](#9-unverified)).
- *Privacy:* audio stays on the device. The LLM is still remote, so the chat and tool results leave, as on desktop.

**C — hybrid.** A browser renderer gives instant scrubbing and preview; the server owns storage, the agent and, later, GPU work.
- *Cost:* B's WASM work for the render path only, on top of A. Two renderers must agree. The determinism tests are the tripwire, but bit-identity across native and WASM is a hypothesis ([§9](#9-unverified)).
- *Privacy:* same as A.

**D — stream the desktop app.** Fastest to try, since the app already builds for Linux (`deb` and `appimage` bundle targets), but it is a remote desktop, not a web product.
- Latency on seeking and scrubbing, and an audio round trip, are inherent. Cost scales with concurrent seats, not users.
- On Linux the keychain is the kernel keyring (#394), so every fresh session starts without a key unless one is injected.
- Audio sits on the server, as in A.
- It is a way to test demand cheaply, not a basis for the product. This is a judgment, not a measurement.

**Sub-option: browser-side LLM calls.** Under A or C, the browser, not the server, could call the LLM provider.
- *Gain:* the key never reaches the server, and a user's local Ollama can be reached if its CORS settings allow the page's origin. That keeps the "no cloud, no key" path that a server-side agent removes ([§6](#6-privacy-and-local-first)).
- *Cost:* the `crates/ai` agent loop has to run in the browser (a WASM port or a TypeScript rewrite), and the tools it calls are on the server, so each tool call is a round trip.
- *Unverified:* browser CORS support for each provider ([§9](#9-unverified)).

## 4. Recommended architecture (A, then C)

```
┌──────────────────────────────────────────────────────────────────────────┐
│ BROWSER   the existing React app, components unchanged                   │
│   lib/tauri-bridge.ts ──► Transport { invoke, listen, fileUrl }          │
│        TauriTransport (desktop)  ·  HttpTransport (fetch + EventSource)  │
│   <input type=file> / drag-drop ──► upload      export ──► download      │
│   WaveSurfer ◄── <audio src = fileUrl(...)>   (byte-range requests)      │
└────────────────────────────────┬─────────────────────────────────────────┘
                                 │ HTTPS   POST /rpc/:command
                                 │         GET  /events   (SSE)
                                 │         GET  /files/...  (Range)
┌────────────────────────────────▼─────────────────────────────────────────┐
│ API SERVER   axum, in one long-lived container                           │
│   auth ─► tenant ─► Workspace (one per user and project)                 │
│   core crate: command bodies · ToolDispatcher · Agent · Store · Engine   │
│   Host trait: events · file URLs · KeyStore · tenant root                │
└───────┬─────────────────────────┬────────────────────────┬───────────────┘
        │                         │                        │
┌───────▼──────────┐   ┌──────────▼───────────┐   ┌────────▼───────────────┐
│ local volume     │   │ object storage       │   │ LLM providers          │
│ working copy,    │◄─►│ (R2 / S3)            │   │ Anthropic · OpenRouter │
│ preview cache    │   │ sources, derived,    │   │ OpenAI · Groq · Gemini │
│ (ephemeral)      │   │ nodes, head (CAS)    │   │ text only              │
└──────────────────┘   └──────────────────────┘   └────────────────────────┘
                                                  GPU workers: later (C)
```

Compare [architecture.md §1](./architecture.md#1-high-level-architecture): the application layer is the part that changes; the crates below it are reused.

**(a) A core crate.** Move command bodies out of `commands.rs` into a crate with no Tauri types. The `#[tauri::command]` functions become thin shims over it. (Avoid the name `core`: it is the name of one of Rust's own crates.)
- `Workspace` is today's `AppState` minus whatever is process-wide.
- A `Host` trait replaces what the commands take from Tauri. `AppHandle::emit` becomes an event sink. `allow_assets_in_dir` and `allow_asset_file` (the asset-scope grants in `commands.rs`) become signed URL issuance. The keychain becomes a `KeyStore`. Path resolution, today `edytlab_home()` and absolute paths, becomes a tenant root. About 31 lines of `commands.rs` mention `AppHandle` or `emit(`, so the surface is small.
- `project.rs` already takes `home` as a parameter in functions such as `recents_path`, so some of this is parametrised.
- Tests that read `commands.rs` as text have to move with the bodies. `src-tauri/tests/main_thread_commands.rs` scans `#[tauri::command]` bodies for store-lock use; after the move it would see only shims and pass vacuously unless it is retargeted. The e2e fake backend (`e2e/backend.ts`) answers "from the Rust source" and cites `commands.rs` by name.

**(b) Progress and cancel become per workspace.** Two ways to do it, and the choice is open.
- Put a reporter and a cancel flag on `ToolContext`. This is explicit, but touches the many construction sites, so a constructor would have to come first.
- Keep `progress::report` and `progress::cancelled` as the tool-facing API, but resolve the sink and flag from a scope the dispatcher sets around each `invoke`, instead of from process statics. This keeps tool code unchanged. It is sound only while tools report from the thread that called them, which has to be checked for `batch_apply`.

**(c) The dispatcher and its lock.** `ToolDispatcher::invoke` takes `&self`, while MCP tools are registered and unregistered at run time, which needs `&mut` (`register`, `unregister_prefix`, called from `mcp_tool.rs`). The built-in tool set could be shared read-only across tenants, with each workspace holding only its own MCP additions, or each workspace could build its own dispatcher. Building one compiles the tools' schema validators, and its cost has not been measured. Narrowing the store lock during DSP (#421 step 2) is independent of this and needed either way.

**(d) Frontend `Transport`.** An interface with `invoke`, `listen` and `fileUrl`, with Tauri and HTTP implementations, selected at build or boot.
- The six direct imports from [§1](#1-what-ties-edytlab-to-the-desktop-today) move behind it. `fileUrl` replaces `convertFileSrc`.
- Open becomes `<input type=file>` and browser drag-drop, followed by an upload. Save and export become a download. The native menu action becomes an in-page control.
- The e2e harness already shows the frontend runs on a page that is not a Tauri webview, and it applies the app's shipped CSP (`shippedCsp()` in `e2e/vite.config.ts`). A hosted page needs its own CSP and headers.

**(e) Server routes.**
- `POST /rpc/:command` mirrors `invoke` one to one, so each bridge function keeps its name, arguments and result.
- Each workspace has an SSE stream for the `agent://*` events, `tool-progress` and `marker-changed`.
- A range-capable file route behaves like `FILE_ROUTE`, with confinement to the tenant's files.
- Uploads are presigned and go straight to object storage.

**(f) Hosting needs a long-lived container.** An agent turn streams for as long as the model and tools take, and the plan gate parks a turn until the user answers: `crates/ai/src/approval.rs` waits up to five minutes (`ANSWER_TIMEOUT`). Function platforms with short time limits are a poor fit. The static frontend can be hosted anywhere.

**Storage.** Because the engine, decoder and tools read paths, a project must be on a local filesystem while it is open. The simplest design that works is a working copy on a local volume, hydrated from object storage on open and written back as nodes and audio are created, with `head` updated last by a conditional write. This matches the store's own ordering: node file first, `head` second, so a crash leaves an orphan node, never a dangling head. The alternative, a storage trait through `session`, `audio-engine` and `tools`, is a larger change and is not needed to ship A.

## 5. Cost model

Formulas with stated inputs. Totals are left to the owner once the unmeasured inputs exist.

**Storage per project** = sources + `derived/` + `previews/`.
- Each destructive edit writes new audio under `derived/`, named by a content hash and written as a 16-bit PCM WAV (`crates/tools/src/tool/util.rs`, `crates/audio-engine/src/encode.rs`), so the directory grows with edit count (`crates/session/src/relocate.rs`).
- A five-minute stereo 48 kHz preview is about 55 MB (`preview_cache.rs`). A derived file of the same length and format is the same size. A source can be larger or smaller, depending on its format (estimate).
- The preview cache is capped at 1 GiB per project (`DEFAULT_CAP_BYTES`). On a server, lower it with `EDYTLAB_PREVIEW_CACHE_BYTES` and keep it on ephemeral disk.
- *Illustration, all inputs assumed:* 10 sources and 20 derived files per project at 55 MB each is about 1.65 GB. Three projects for each of 100 users is about 500 GB. At the issue's R2 figure of about $15 per TB-month (verify), that is about $7 a month. Storage is not where the money goes.

**CPU.** Rendering is single-threaded on purpose (`crates/audio-engine/src/render.rs`), so one render holds one core for its duration.
- The only timing measured so far is a time-stretch of 32 s of stereo taking about 10 s in a debug build (#421).
- Release-build timings are unmeasured, and so is the number of concurrent renders a core can sustain. That figure sets the instance size, and so most of the compute line in the table above. It is the first thing [Phase 0](#8-phased-milestones) measures.

**LLM.** Per-turn cost = the sum over requests of (input tokens × input price + output tokens × output price).
- The first request is about 15,000 tokens before the user has said anything (#395). Only the total was measured; the split between system prompt and tool schemas is not known.
- Every tool round trip resends the context. A turn may make 20 tool calls (`MAX_TOOL_CALLS_PER_TURN`, [architecture.md "Tool budget"](./architecture.md#tool-budget)). A turn that uses the whole budget is therefore on the order of twenty requests, each carrying the full context, plus the one-shot classifier and plan requests when they run. A step can carry several calls, so the real number is usually lower.
- For Anthropic, the agent loop already marks the system prompt and the tool list cacheable (`cache_control: ephemeral`, `crates/ai/src/agent_loop.rs`), so a repeated prefix may bill at a cached rate. That is the provider's rule and its effect was not measured here. The other providers were not checked.
- With bring-your-own keys, this cost is not on our bill. Managed keys need metering and quotas, and the issue notes that Stripe's token billing is in private preview (verify). Do not hard-code model prices in code or docs; read the provider's pricing page at decision time.

**Egress.** Every head change can stream a full preview mix to the browser, so egress scales with editing activity. The issue records that R2 has no egress fees (verify); on other object stores egress is billed and belongs in the model.

**Always-on container.** An agent turn may last minutes, so a container serving many users is up whenever any user is. Its size follows from the CPU line, which is unmeasured.

## 6. Privacy and local-first

Today's promises, quoted exactly:

- [architecture.md, Design Invariants](./architecture.md#design-invariants): "Audio bytes never leave the machine" and "API keys never touch edytlab servers".
- `README.md` (Key features): "**Local-first.** Audio never leaves your machine unless you export."
- `website/app/privacy/page.tsx`: the app "has no server of ours behind it", and "There are no “cloud projects” and there is no server-side processing."
- `website/app/use-cases/local-ai-audio-editor/page.tsx`, the what-stays-local table: audio files go "Nowhere. Decoded, processed and rendered locally."

What follows from that, plainly:

- Under A, C and D, **those statements are false for the hosted mode.** Audio, the edit graph and the chat are on our infrastructure.
- B keeps audio on the device (OPFS), but the LLM is still a remote service, as on desktop.
- So a hosted mode has to be **a separate, clearly named product surface** with its own privacy policy, terms and data processing agreement. The desktop copy stays true for the desktop app.
- Site, README and privacy copy change only when a hosted mode ships. **This document changes none of them, and no automated guard covers the claim, so reviewers must.**

**What a hosted mode would hold:**

- Audio: sources, derived files and previews.
- The edit graph, including labels and markers.
- Chat and tool results. These go to the LLM provider as they do today.
- `memory.md`, skills and agent profiles, per user.
- Stored bring-your-own keys. This is a **new liability**: encrypt each with an envelope scheme under a managed key service, never log them, and never return them to the browser.
- Transcripts, once #384 ships. They are personal data and reach the LLM in tool results.

**The "no cloud, no key" path.** With a server-side agent, Ollama's local path goes away, because the server cannot reach a user's `localhost`. It survives only if LLM calls move to the browser ([§3](#3-options-compared)).

**GDPR** (a list of questions for a lawyer, not legal advice):

- Voice recordings are personal data. Speaker identification (#168) may count as biometric data under Article 9, which needs a legal review before it is offered in a hosted mode.
- Retention and deletion must reach object storage, local volumes and backups.
- Data processing agreements are needed with the storage, LLM and GPU sub-processors, and the region has to be pinned.
- Data export is cheap: a project is already a portable directory, and `save_project_as` already copies one.

## 7. Security risks of hosting

| Risk | Today's code | Mitigation |
|---|---|---|
| **Remote code execution through MCP stdio** | `McpServerConfig::Stdio { command, args, env }` is spawned as a child process (`crates/mcp/src/transport.rs`). Enabled servers start at launch. A plugin's MCP servers are registered disabled (`register_plugin_mcp_servers`), so installing is not running, but enabling is one action. | Reject stdio configs in hosted mode and accept only remote (URL) servers. If stdio is ever wanted, run each server in a per-tenant sandbox (Firecracker or E2B-style) with no network by default. |
| **Server-side request forgery** | `set_base_url_for` accepts any `http://` or `https://` URL with a host and saves it. The agent then sends requests, and the provider key, to that URL. The MCP HTTP transport posts to a configured URL. `probe_provider_for` does the same for a "test" button. | In hosted mode, fix provider base URLs by provider id and allow no override. Resolve DNS and block private, loopback and link-local ranges (including cloud metadata addresses), on redirects too. Restrict egress at the network. Never allow a base-URL override on a managed key. |
| **Path escape** | Commands and tools take absolute paths from the client or the model ([§1](#1-what-ties-edytlab-to-the-desktop-today)). `open_project` checks only that a path is absolute. `install_plugin` accepts `local:` paths. | Every path goes through one tenant-root resolver: canonicalise, refuse `..` and symlinks that leave the root, refuse absolute paths. Uploads land in a sources area and exports in an exports area. Tool results carry opaque ids, not server paths. Test with a user A / user B case. |
| **Prompt injection into tool calls** | The model chooses tool arguments and sees file names, tool results, `memory.md`, skills and MCP output. Anything it reads can steer a later call. `Tool::mutates()` and Plan first gate edits, not reads. | Treat confinement as the boundary and prompt hygiene as a mitigation only. Keep Plan first available. Allow-list remote MCP servers. |
| **Untrusted uploads into decoders** | `symphonia`, `hound`, `flac-codec` and `rusty_mp3` parse uploaded bytes in the API process. They are Rust, but a header can still claim an enormous length or channel count. | Cap size, duration, channels and sample rate from the header before decoding. Rate-limit uploads. Run decoding in a resource-limited, sandboxed worker with a timeout. |
| **Resource exhaustion** | A render holds a core. The preview cache defaults to 1 GiB per project. A turn can wait five minutes on the plan gate while holding the agent lock. The tool budget bounds calls per turn, not CPU time. | One active turn per user. A job queue with per-tenant CPU-time and storage quotas. A lower preview cache cap. Cancel scoped to the workspace. |
| **Cross-tenant leakage** | `progress.rs` globals ([§1](#1-what-ties-edytlab-to-the-desktop-today)). Any other process-wide state added later has the same property. | Per-workspace progress and cancel ([§4(b)](#4-recommended-architecture-a-then-c)). A test that runs two workspaces at once and asserts neither sees the other's events. |
| **Concurrent writers** | `Store` assumes a single writer (`crates/session/src/store.rs`). Two browser tabs on one project would create two workspaces. | One active workspace per project through a lease. A second tab reads, or takes over explicitly. A conditional write on `head` in object storage as a backstop. |
| **Stored provider keys** | Today the OS keychain. | Envelope encryption under a managed key service, scoped decryption, no logging, rotation. Prefer browser-held keys if the sub-option in [§3](#3-options-compared) is chosen. |
| **New web surface** | None today, since IPC is in-process. | Authentication on every route, CSRF protection, a closed CORS policy, per-user rate limits, request size limits. |

## 8. Phased milestones

Each phase is its own PR series. This keeps the issue's seven phases in order, adds a measurement phase before them and an optional phase for option C after them. The unsafe capabilities (phase 5) are closed before anything is hosted (phase 6).

**0. Measure and de-risk (no hosting).**
- Time release-build renders and time-stretches per minute of audio on a candidate instance type.
- Split the first request's tokens into system prompt and tool schemas (#395).
- Run `cargo check --target wasm32-unknown-unknown` for `audio-dsp` and `audio-time`, then for the decoder and encoder crates, to settle B against C.
- Land step 2 of #421 (lock scope). #388 is done (#466).
- *Exit:* the numbers are written into this document.

**1. Core extraction (desktop only).**
- Move command families into the core crate one at a time behind shims. Make progress and cancel per workspace ([§4(b)](#4-recommended-architecture-a-then-c)).
- *Exit:* all CLAUDE.md acceptance gates green, no behaviour change, `cargo tree` for the core crate shows no `tauri`, and the text-scanning tests ([§4(a)](#4-recommended-architecture-a-then-c)) still guard what they guarded.

**2. Frontend `Transport`.**
- Introduce the interface and the Tauri implementation, then move the six direct imports behind it.
- *Exit:* no `@tauri-apps` import outside the Tauri transport module, enforced by a guard in the style of `lib/no-node-globals.ts`, with the vitest and e2e gates green.

**3. axum server (single tenant, dev only).**
- `/rpc/:command`, the SSE stream and the file route over local disk, bound to localhost.
- *Exit:* a subset of the Playwright e2e suite runs against the real server instead of `mockIPC`.

**4. Tenancy, storage, auth and keys.**
- Per-user roots, object storage with a conditional write on `head`, the path resolver for commands and tools, a server `KeyStore` and an auth provider (Clerk, Auth.js or a Supabase JWT, per the issue).
- *Exit:* a test proves user A cannot read or write user B's files or events, and deletion works end to end, including the retention rules for backups.

**5. Unsafe capabilities.**
- Turn off MCP stdio, `install_plugin` and `batch_apply` over server folders on the web. Fix each provider's base URL by provider id. Allow remote MCP only, from an allow-list.
- Move recording to `getUserMedia` and an AudioWorklet, then upload.
- *Exit:* the [§7](#7-security-risks-of-hosting) rows for RCE, SSRF and path escape each have a test.

**6. Hosting and operations.**
- A long-lived container platform (Fly Machines, Railway, Render, ECS or Cloudflare Containers, per the issue), quotas, metering, observability and backups.
- *Exit:* a load test at the target concurrency, using the Phase 0 numbers.

**7. Compliance and launch.**
- The hosted privacy policy and terms, data processing agreements, region pinning and retention. The site separates desktop and hosted copy ([§6](#6-privacy-and-local-first)).

**8. Optional, option C.**
- A WASM preview in the browser, if Phase 0 shows it is feasible and latency needs it.
- GPU ML workers (Modal or RunPod serverless, per the issue) only after #383–#385 land. The issue records that Fly GPUs are deprecated (verify).

## 9. Unverified

- WASM builds of `symphonia`, `hound`, `flac-codec` 1.3, `rusty_mp3` 0.7, `rubato` and `realfft`.
- Whether `ort` can be built for `wasm32` at all, and `onnxruntime-web` / WebGPU model sizes.
- Whether render output is bit-identical between native and WASM. The invariant in `crates/audio-engine/src/render.rs` is stated across Mac and Windows; whether `wasm32` math matches is untested.
- Release-build DSP throughput, and the cost of building a `ToolDispatcher` per workspace.
- Third-party storage, compute and GPU prices, and the issue's monthly ranges.
- Browser-direct LLM calls (CORS) for each provider, including Ollama.
- Safari eviction of OPFS.
- That tools report progress only from the thread that called them (needed for the scope approach in [§4(b)](#4-recommended-architecture-a-then-c)).
- Anything legal in [§6](#6-privacy-and-local-first).

## 10. Decisions needed from the owner

From the issue:

- [ ] Go or no-go, and which option (recommended: A, then C).
- [ ] Hosting provider and region.
- [ ] Key model: bring your own, managed, or both.
- [ ] Pricing and billing.

Raised by this evaluation:

- [ ] Whether a hosted mode may ship under the edytlab name, given the local-first positioning ([§6](#6-privacy-and-local-first)).
- [ ] Bring-your-own keys only at first, or managed keys from the start.
- [ ] Whether recording and MCP are needed in a first hosted version. Leaving them out removes most of [§7](#7-security-risks-of-hosting).
- [ ] Whether the browser-side LLM sub-option is worth its cost for the sake of Ollama and keeping keys off the server.

## 11. What this document does not do

- It adds no code, and changes no behaviour.
- It changes no privacy policy, README or site copy.
- It does not claim that stem separation or transcription work. Both are stubs (#383–#385).
- It does not decide anything in [§10](#10-decisions-needed-from-the-owner).
