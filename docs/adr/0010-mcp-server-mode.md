---
title: "0010. MCP server mode: an outside agent drives one project, edits by default as undoable nodes"
sidebarTitle: "0010 MCP server mode"
---

Status: Accepted, 2026-11-16, with six questions still open (Questions). The owner decided the
default mode, distribution, the SDK and live editing; nothing here is built yet.

## Context

edytlab is an MCP client only (`crates/mcp`). #762 asks it to serve its tools over stdio, so that
Claude Desktop, Claude Code and Cursor can edit a project without the window's own agent. The
issue names three questions an ADR must answer first: approvals, one live store per folder, and
files. This record answers them and fixes the shape of the server.

What the code does today, checked on `main` at 7f73dc4 (2026-11-15):

- **The registry has 115 tools** (`ToolDispatcher::default_dispatcher()`; the issue's 108 is old).
  13 are read-only (`mutates() == false`, pinned by `tests/read_only_tools.rs`), so 102 mutate. 51
  run off the store lock (`OFF_LOCK` in `tests/off_lock_tools.rs`). The server computes these
  numbers from the registry and never copies them.
- **Plan first is not in the dispatcher.** `ai::PlanGate` and `approval::ask` sit in the agent
  loop (`crates/ai/src/approval.rs`, `agent_loop.rs`). `tools::Shared::dispatch` runs whatever it
  is handed. A server that dispatches directly has no approval at all unless it adds one.
- **The live-store registry is per process.** `session::live::Registry` tells folders apart by
  device and inode, and hands every shared open in one process the same `LiveStore` (#501, #534,
  #566). Nothing covers a second process: `docs/saas-evaluation.md` says so ("nothing covers a
  second process"). The window and an MCP server are two processes. Each would hold its own head
  over one `.audiograph/head`, and the second writer's next node would skip what the first
  appended. That is the bug #501 closed inside one process.
- **The window always has a project open.** `lib.rs` opens `default-project` under the app data
  dir at launch.
- **Paths are not confined.** `load`, `analyze_track`, `separate_stems`, `import_labels` and
  `batch_apply` take any path. `render_final`, `export_transcript` and `export_recipe` write to
  any path. `export_multiple` takes a relative `output_dir` with no `..`. Only
  `destination::refuse_project_audio` (#653) guards what a write may land on, and only for audio
  the project reads. The window's agent is trusted with this because the user approves each plan.
- **`crates/mcp` is client code.** Its `StdioClient` speaks the `2024-11-05` handshake and its
  `ToolDescriptor` is `{name, description, schema}`. It has no server type, no stateless request
  handling and no cancellation.

What the protocol says (current revision `2026-07-28`):

- Servers offer tools, resources and prompts. Tools are model-controlled; resources and prompts are
  offered to the user or the application.
- "There SHOULD always be a human in the loop with the ability to deny tool invocations." The
  protocol cannot enforce it; the host does. Tool annotations "MUST" be treated as untrusted
  unless the server is trusted.
- Requests are stateless, there is no `initialize` handshake, and a server must answer
  `server/discover`. A server may serve legacy clients (`2025-11-25` and earlier, with
  `initialize`) as well. A tool list "MUST NOT vary per-connection".
- stdio: one JSON message per line; nothing but MCP messages on stdout, logs on stderr; the server
  exits when stdin closes.
- A failed call is a result with `isError: true`, which the model reads and corrects; a protocol
  error is for a malformed request.

What the hosts do:

- Claude Code: `claude mcp add [options] <name> -- <command> [args...]`, scopes `local`, `project`
  (`.mcp.json`) and `user`. It asks before an MCP tool runs unless a permission rule allows it
  (`mcp__<server>__<tool>`, `mcp__<server>__*`). Its documentation does not say annotations change
  that. A project-scoped server is approved once, and in `claude -p` and Agent SDK sessions it
  loads without asking.
- Claude Desktop: `claude_desktop_config.json` `mcpServers` with `command`, `args`, `env`; it asks
  for approval before each action.
- Cursor: `.cursor/mcp.json` or `~/.cursor/mcp.json` with `command`, `args`, `env`; it asks by
  default, and an allow-list or auto-run mode removes the prompt.

So the human in the loop is real by default and removable in every host. A server that depends on
it alone is only as safe as the user's most permissive setting.

## Decision

One new crate, `crates/mcp-server`, holds the server library and a binary, `edytlab-mcp`. One
process serves one project, named at launch (`--project <dir>`, required). It speaks stdio only.

### 1. Approvals: tiers, `edit` by default, enforced by the server

Options:

- **A. The MCP client's confirmation only.** Nothing to build. The server is as safe as the
  user's most permissive host setting: `claude -p`, Cursor auto-run and a blanket allow rule all
  skip the prompt, and annotations are untrusted by the spec. A prompt-injected model in an
  unattended run can call every mutating tool (102 of the registry today).
- **B. An allow-list in the server's own config.** The server enforces it, through the `allowed`
  set that `Shared::dispatch` already takes (nested dispatchers in `batch_apply`, `apply_recipe`
  and `rederive` carry it too, #238). It does not depend on any host. A flat list of 115 names is
  tedious to write and to review.
- **C. The server asks, with its own plan gate.** The 2026-07-28 revision lets a server answer a
  call with an input request (elicitation), so a mutating call could wait for a yes. Client
  support for it is not something the three hosts document, and a client that lacks it leaves the
  call stuck or refused. It also duplicates the host's prompt where the host has one.

**Recommendation: B, shaped as tiers, with A kept as the second layer.**

- A launch flag `--mode read|edit|full` picks a tier. The default is `edit` (the owner's
  decision, 2026-11-16): the issue's first example works with no flag, every edit is an undoable
  node, and the host's own prompt is the layer above it. `full` is never the default.
  - `read`: the tools that do not mutate (13 today). The server opens the store read-only.
  - `edit`: adds the mutating tools that stay inside the project: they append a session node and
    write only the project's own audio folders and caches. Every such edit is an undoable node.
  - `full`: adds tools that write a file named by the caller (`render_final`,
    `export_transcript`, `export_recipe`, `export_multiple`) and tools that rewrite or drop
    history (`compact_session`, `revert_to` and the like).
- `batch_apply` and `apply_recipe` are in no tier in v1. They build nested dispatchers and take
  paths from a recipe file, which a pre-dispatch path check cannot see (section 3).
- The tiers are a pinned list in a test, as `READ_ONLY` and `OFF_LOCK` are. **A tool is in `full`
  until someone classifies it**, the same default as `mutates() == true`. A new tool cannot reach
  a lower tier by being forgotten.
- The server sets annotations (`readOnlyHint` from `mutates()`, `destructiveHint` for `full`) so a
  host can show them. It never relies on them.
- `--tools a,b,c` narrows a tier further. It can only remove.
- The tool list is fixed for the life of the process, which the spec allows ("MUST NOT vary
  per-connection").
- The host's own prompt stays on top. The docs for each host say how to set it:
  `claude mcp add edytlab -- edytlab-mcp --project <dir>` (edit is the default).

Why not C now: it is the only option that could replace the host's prompt, and it is the one the
hosts support least. If a host documents elicitation, a later record can add "ask in `full`".

Consequence: with no flag, `claude -p` can edit, and the worst case is nodes the user can undo; the
host's allow rules decide whether each call is asked about. A user who only wants to read passes
`--mode read`, and a user who wants an export passes `--mode full`. Because `edit` holds the
project's lock (section 2), the default command is refused while the window has that project open;
the refusal names the window and says that `--mode read` works alongside it.

### 2. One live store per folder: refuse, detected by a lock across processes

Options:

- **A. Refuse.** The second process to open a project is told who has it and exits. Needs a
  cross-process check, because the registry cannot see another process.
- **B. Share.** The server connects to the running window and dispatches into the window's store,
  so edits show live. It needs a local socket or named pipe in the window, a protocol over it, and
  a window that is running. The window's store handling is still being moved into one module
  (ADR 0006, not built), so this would be built on the code that ADR means to replace.
- **C. A private copy.** The server works on a copy of the project and the user merges it later.
  Nodes are content-addressed, so copying new nodes back is possible, but audio is duplicated, the
  merge has no owner, and the issue's acceptance ("the window then shows it on reopen") would need
  the merge to exist.

**Decision: A now, with a lock file the window and the server both take. B is the planned target
(the owner's decision, 2026-11-16): the window will accept a server's edits live once ADR 0006's
`open_project` module is built, in a record of its own. Until then, A.**

- The first store on a folder in a process takes an exclusive advisory lock on
  `.audiograph/open.lock` (`File::try_lock`, stable since Rust 1.89; flock on unix, `LockFileEx` on
  Windows; the toolchain is 1.90; no new dependency). The lock lives with the folder's pin in
  `session::live`, so it is held exactly as long as the live store and released when the process
  dies. There is no stale lock to clear. The file holds the process id and its name, for the
  message only.
- `open_shared` and `open_private` take it. `open_read_only` does not: it cannot write.
- A second process gets `Error::OpenElsewhere` naming the holder ("open in edytlab (pid 4121)").
  The window shows that where it shows `Error::AlreadyOpen` today.
- A folder that cannot be locked (a filesystem without locks) is refused, with the reason. An
  override is the owner's call (Questions).
- The server needs `read` mode to work while the window has the project open, so `read` opens the
  store read-only and re-reads the head for every call (a `Store` is `{project_dir, head}`, so this
  is cheap). It sees the window's latest saved head and no more. `edit` and `full` hold the lock
  and refuse while the window has the project.
- The server never opens `default-project`: `--project` is required, and the window holds that
  folder from launch.
- One server serves one project. Another project is another entry in the host's config.

Consequence: the window must ship the lock before `edit` mode can. An older window without it
does not take the lock, so the guarantee holds only against windows from the release that adds it
(Questions). The window does not re-read the head from disk while it is open, so after a server
edit the project must be reopened to show it, as the issue says.

### 3. Files: one resolver, project root for writes, named roots for reads

Options:

- **A. No new confinement.** The window's agent already takes any path, under a plan the user
  approved. It is the least work and the weakest: an unattended client with `--mode full` could
  overwrite any file the user can write, and `refuse_project_audio` only protects the project's own
  audio.
- **B. Confine writes only.** Reads of any audio file stay open (`load` only decodes audio).
- **C. Confine reads and writes.** Everything resolves under the project root plus roots the user
  names at launch.

**Recommendation: C.**

- One function in `mcp-server` resolves every path argument before dispatch, as a read or as a
  write. It joins a relative path to the project root, canonicalises (for a write, the parent
  directory, since the file may not exist), and refuses a result outside the allowed roots. `..` and
  a symlink out are refused after canonicalisation, never by looking at the spelling (the lesson of
  #653 and of `FolderId`).
- Writes: `<project>/exports/` only, or roots given as `--export-dir`. Never `.audiograph/`,
  `derived/` or `generated/`; those belong to the audio store (ADR 0008).
- Reads: the project root and roots given as `--import-dir`. Without any, `load` can only read
  files already in the project.
- It runs first. `destination::refuse_project_audio` still runs inside the tools that call it, and
  is not replaced.
- The path-bearing arguments are found by a table, not by a name scan at run time. A test walks
  every tool's schema, lists each string argument whose name suggests a path (`path`, `out_path`,
  `output_dir`, `input_dir`, `recipe_path`, `file`), and compares it to the table. A new tool with a
  path argument fails the test until it is classified read or write. This is the approach
  `off_lock_tools.rs` takes for track arguments. `batch_apply` and `apply_recipe` stay out of the
  tiers because their paths come from inside a recipe.
- The server does not load `~/.edytlab/mcp.json`. Its dispatcher is the default registry only, so
  the app's own MCP client tools (`<server>__tool`) are never re-exported, and a loop of servers
  calling each other cannot form. A test asserts no served name contains `__`.
- Results name files relative to the project root where they can.

Consequence: a user who keeps source recordings elsewhere passes `--import-dir`. That is one flag
and the user sees the roots in the host's config.

### Where it lives

Options: a new `crates/mcp-server`; a subcommand of `apps/cli`; a mode of the desktop binary.

**Recommendation: `crates/mcp-server`, library plus `edytlab-mcp` binary.**

- `apps/cli` is, in its own header, test infrastructure "not packaged in user-facing builds"; it
  needs an API key and depends on `ai`, which links the keychain and an HTTP stack. The server
  needs neither, and the claim that it makes no outbound request is easier to keep, and to test
  (`cargo tree -p mcp-server` contains no `ai`, `reqwest` or `keyring`), as a crate of its own.
- The desktop binary is a Tauri app with a webview. It cannot run headless on a build machine, and
  a second desktop process is the two-writer problem of section 2.
- The crate depends on `tools`, `session`, `audio-engine` and the SDK. It does not depend on `ai`
  or `crates/mcp`.
- Adding a crate runs the "crate added" sweep in `CLAUDE.md` (README crate list, architecture
  workspace layout and its site mirror, the Repo map, the marketing crate counts).

### Dispatch

Every call goes through `tools::Shared::dispatch(name, args, "", Some(&allowed))`, with a
`ToolDispatcher::default_dispatcher()`, the project's store, a fresh engine and an empty
clipboard. That keeps what the dispatcher already guarantees: admission under the dispatcher lock
alone, locked tools under store, engine and clipboard in that order, `runs_off_the_lock` tools on a
staged store with one re-run and a locked third run, and the refusal when the user's edit changed a
track the call names (`addressed_tracks`). The server adds no lock of its own and calls no tool
directly. The dispatch is synchronous, so each call runs on the blocking pool, as a Tauri command
that waits on a lock does (`docs/agents/tauri-conventions.md`). Calls to one project may overlap;
the store lock orders them. The clipboard lives as long as the process and is lost on restart, which
those two tools' descriptions should say.

A tool's `ToolResult::Error` becomes `isError: true`. A schema failure or an unknown name is a
protocol error. `progress` events go to `notifications/progress` and `notifications/cancelled` to
`progress::request_cancel` (stage 3); these are process-wide, which is right for one project per
process.

### Reads: tools for the model, resources for the person

A model must be able to do everything it needs with tools, because a host may not let it read a
resource unprompted. So the read-only ones (13 today) are served as tools, `list_session` first among them
(it is paged and byte-capped, #618).

Resources are an addition for the user, and are read-only:

- `edytlab://project/tracks`, `.../buses`, `.../markers`: the lines `tools::session_lines` writes,
  the same ones `list_session` pages.
- `edytlab://project/history`: the node list with the head marked.

The session block (`ai::session_context`) is not served. It is a system-prompt fragment for
edytlab's own agent, it lives in `ai`, and a host brings its own prompt.

### Prompts: bundled skills, from the user's skills folder

The 12 bundled skills are markdown with a name and a description. They are templates a person
starts on purpose ("clean up this podcast"), which is what a prompt is, and Claude Code shows them
as slash commands. The server serves the files of `~/.edytlab/skills/` through
`skills::SkillLibrary::load_from`, as the window's agent does, in stage 4. If the folder is absent
(the window never ran) there are no prompts. It does not carry its own copy of the skills, so
there is one text to keep current (`bundled_skills.rs`).

### The SDK: `rmcp`

- `rmcp` is the official Rust SDK (`modelcontextprotocol/rust-sdk`). It implements `2026-07-28`
  and stays compatible with `2025-11-25` and earlier, has a stdio server (`transport-io`), on
  `tokio`, with `#[tool]` macros. **Decided (the owner, 2026-11-16): use `rmcp`**, pinned to an
  exact 3.x release that is at least two weeks old when it is added. At the time of writing: 3.10.0, published 2026-11-12,
  Apache-2.0, `rust-version` 1.88 (the repo pins 1.90), about 50 million downloads. Its prompt and
  resource support is not checked here; stage 0 checks it. The 3.x
  line broke from 2.x, so pin the exact minor and read the changelog on each bump.
- The repo has no `rmcp` in `Cargo.lock` today. Adding it adds a dependency (notices, `cargo audit`,
  FOSSA: `docs/agents/ci-release.md`). Apache-2.0 is compatible with the repo's MIT licence.
- Reusing `crates/mcp` is not possible and not worth it: it is the client half, sync, on a
  2024-11-05 handshake. It is still useful as a test client for the legacy era (below).
- The macros assume tools known at compile time. The server's list comes from the registry and the
  tier, so it implements the SDK's server handler (`list_tools`, `call_tool`) by hand. Stage 0
  checks that this works for both eras.

### Staged plan

Each stage is one PR that leaves `main` working.

- **Stage 0, spike (not merged).** `rmcp` 3.x with a hand-written handler serves a tool list from
  the registry over stdio and answers a legacy `initialize` and a modern `server/discover`.
  `claude mcp add`, Claude Desktop's config and Cursor's `mcp.json` each list it. Output: a note in
  the stage 1 PR on what failed.
- **Stage 1, read-only server.** The crate, `--project`, `--mode read` only (until stage 2 the flag
  is required and a missing one is refused with that reason; the `edit` default arrives with the
  lock), `open_read_only` per
  call, `Shared::dispatch` with the `read` set, tier test (`READ_ONLY` equals the `read` tier), the
  path resolver for reads, the "no `ai`, no network" tree test. **The stdio end-to-end test**
  (acceptance 3): a test starts `edytlab-mcp` on a fixture project and drives it with an MCP
  client twice, once as a legacy client (`crates/mcp`'s `StdioClient`) and once as a modern one
  (`rmcp`'s client), listing tools and calling `list_session` and `measure`. Which of the 13 work
  against a read-only store is found here; one that writes a cache the store refuses is served
  with the cache path outside the store or left out, and the stage says which. Acceptance 1 (the
  list half).
- **Stage 2, the lock and edit mode.** `.audiograph/open.lock` in `session::live`, taken by
  `open_shared` and `open_private`, `Error::OpenElsewhere`, the window's message. `--mode edit`,
  the pinned tier lists, the write resolver, tests: the second process is refused in both
  orders; a `read` server and the window coexist; an `edit` call appends a node a reopened store
  reads (acceptance 1, the edit half); an `edit` server refuses `render_final` (acceptance 2);
  the window's undo reaches the head from before the server's nodes (the head trail file is the
  window's, so this is checked here, not assumed).
- **Stage 3, `full`, progress and cancel.** Exports through the write resolver and the `full`
  tier, annotations, `notifications/progress` and cancellation, the schema walk for path
  arguments. Acceptance 2 again for the other direction: an `edit` server refuses an export, a
  `full` server writes inside `exports/` and refuses outside it.
- **Stage 4, resources and prompts.** The four resources and the skill prompts above, each with
  a stdio test that lists and reads them, and a test that the prompts equal the files in the
  skills folder.
- **Stage 5, documentation and packaging** (acceptance 4): README, `docs/architecture.md`, the
  developer guide on the site, the privacy page (no new outbound request; a new local entry
  point), `docs/agents/mcp-server.md` and its row in `CLAUDE.md`, and how the binary reaches the
  user. **Both ways (the owner, 2026-11-16):** the installers put `edytlab-mcp` next to the app
  (inside the `.app` bundle on macOS; the docs give each platform's path for `claude mcp add`), and
  each release also carries a standalone `edytlab-mcp` archive per platform. `release-dev.yml`,
  `docs/agents/ci-release.md` and the release-day asset count in `CLAUDE.md` (6 today) change with
  it. Documentation for each stage lands in that stage.

### How we will know

- Stage 1's end-to-end test passes for both eras, and `cargo tree -p mcp-server` has no `ai`,
  `reqwest` or `keyring`.
- The tier test fails when a tool is added without a tier, and the path test fails when a tool
  with a path argument is added without a class.
- A server and a window on one folder: the second to start is refused with the holder's name; with
  `--mode read` both run.
- A server killed in the middle of a call leaves a project the window opens (nodes are written
  whole, head last).

## Questions for the owner

1. ~~**The default.**~~ Decided 2026-11-16: `edit` (section 1).
2. **The tiers.** Which tools belong in `edit` and which in `full` is a product call. This record
   puts `revert_to` and `compact_session` in `full`, and `fork_node`, `name_node` and
   `separate_stems` (hundreds of MB, held by Plan first today) are open.
3. **Reads outside the project.** Is "only the project plus `--import-dir`" too strict for the
   people who will use this? The window's agent can load any path.
4. **A folder that cannot be locked** (a network share): refuse, or allow with a flag and a
   warning that two writers are possible?
5. **Older windows.** Is it acceptable that `edit` mode is only safe against a window from the
   release that adds the lock? The server can say so when it starts.
6. ~~**Distribution.**~~ Decided 2026-11-16: both, bundled and a standalone asset (stage 5).
7. **Skills as prompts.** Serve only the bundled skills, or the user's own skills too (the folder
   is shared)?
8. ~~**Live editing.**~~ Decided 2026-11-16: yes, as the planned target after ADR 0006, in its
   own record (section 2).
9. ~~**The SDK.**~~ Decided 2026-11-16: `rmcp`, an exact 3.x pin at least two weeks old.
10. **The cost of the schemas.** `read` serves 13 schemas. `edit` and `full` send about a hundred schemas to
    the host's model every turn, and the slim set that Ollama gets (`tool_selection.rs`) is not
    applied. Is `--tools` enough, or should the server offer a profile?

## Sources

- MCP specification, 2026-07-28: <https://modelcontextprotocol.io/specification/2026-07-28>
  (security and trust and safety), tools: <https://modelcontextprotocol.io/specification/2026-07-28/server/tools>,
  stdio: <https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/stdio>,
  versioning and the dual-era matrix: <https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning>.
  Read 2026-11-15.
- Claude Code, MCP: <https://code.claude.com/docs/en/mcp>; permissions:
  <https://code.claude.com/docs/en/permissions>. Read 2026-11-15.
- Claude Desktop, local servers: <https://modelcontextprotocol.io/docs/develop/connect-local-servers>.
  Read 2026-11-15.
- Cursor, MCP: <https://cursor.com/docs/context/mcp>. Read 2026-11-15.
- `rmcp` on crates.io: <https://crates.io/crates/rmcp> (3.10.0, Apache-2.0, 2026-11-12); repository:
  <https://github.com/modelcontextprotocol/rust-sdk>. Read 2026-11-15.
- `File::try_lock`, stable since 1.89: <https://doc.rust-lang.org/std/fs/struct.File.html>.
  Read 2026-11-15.
- In this repository: #762, #753, #501, #534, #566, #653, #238, #618; ADR 0002, 0006, 0008;
  `docs/saas-evaluation.md` (concurrent writers, path escape).
