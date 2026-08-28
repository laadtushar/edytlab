# edytlab — API Reference

> Every function `tauri-bridge.ts` exports, which is the whole IPC surface the
> frontend can reach. Commands are invoked from the frontend via that module —
> nothing calls `invoke` directly.
>
> `apiReferenceCoverage.test.ts` fails if an export here has no entry, so this
> page cannot quietly fall behind the bridge again.

---

## Overview

All commands follow the pattern:
```typescript
// TypeScript
const result = await bridge.commandName(arg1, arg2);
// Throws string on Rust Err(_)
```

```rust
// Rust return type alias
type CmdResult<T> = Result<T, String>;
```

Events are subscribed to with the unlisten pattern:
```typescript
const unlisten = await bridge.onTextDelta((chunk) => { /* ... */ });
// Later:
unlisten();
```

---

## Table of Contents

- [Project Management](#project-management)
- [API Key Management](#api-key-management)
- [Provider Selection](#provider-selection)
- [Model Selection](#model-selection)
- [Session Graph (DAG)](#session-graph-dag)
- [Rendering and A/B Compare](#rendering-and-ab-compare)
- [Agent Conversation](#agent-conversation)
- [Selection and Markers](#selection-and-markers)
- [Transcript](#transcript)
- [Tracks](#tracks)
- [Clips](#clips)
- [Recording](#recording)
- [Templates](#templates)
- [Plugins](#plugins)
- [Capabilities](#capabilities)
- [Skills (CRUD)](#skills-crud)
- [Agent Profiles (CRUD)](#agent-profiles-crud)
- [Memory](#memory)
- [MCP Servers](#mcp-servers)
- [Events](#events)
- [TypeScript Types](#typescript-types)

---

## Project Management

### `openProject(path: string) → ProjectInfo`

Open an existing project directory or create a new one.

**Parameters:**
- `path` — absolute path to the project directory

**Returns:** `ProjectInfo`
```typescript
interface ProjectInfo {
  path: string;
  head: NodeId | null;  // null if project has no sessions yet
}
```

**Side effects:** Rebuilds the Agent if an API key is already configured.
Also stamps `last_opened_at` into `project.json`, creating it on the
first open, and moves the project to the top of the recents list.

**Example:**
```typescript
const project = await bridge.openProject("/Users/alice/Music/my-session");
console.log(project.head); // null | "abc123..."
```

### What a project is on disk

```
my-session/
  project.json            name, created, last opened, notes
  .audiograph/
    nodes/                the edit history
    head                  where you are in it
    view.json             zoom, selection, playhead
    derived/              audio produced by edits
    clipboard/            blobs a paste depends on
    previews/             render cache — disposable
```

The split is by lifetime. `project.json` is the project's identity and
sits outside the store because the store is an implementation detail.
`view.json` is disposable — losing it costs a scroll. `previews/` is a
cache: every entry re-derives byte-identically from the node it is named
for, so it is the one directory a copy of a project can safely skip.

Derived audio lives **inside** the project. It used to be written beside
whichever source file was opened, which meant a project folder held the
history and none of the sound — so it could not be copied, moved or
backed up.

### `getProjectMeta() → ProjectMeta`

```typescript
interface ProjectMeta {
  name: string;          // defaults to the folder name
  created_at?: string;   // ISO 8601
  last_opened_at?: string;
  notes?: string;
}
```

A missing or corrupt `project.json` returns a folder-named default
rather than failing: the audio and its history are not in that file.

### `setProjectMeta(name: string, notes?: string) → ProjectMeta`

Rename the open project. An empty name is refused. The recents row is
updated in step, so the list does not show the old name until the next
open.

### `getViewState() → ViewState` · `saveViewState(view: ViewState) → void`

```typescript
interface ViewState {
  head?: string | null;
  zoom_px_per_sec?: number | null;
  selection?: [number, number] | null;   // session seconds
  playhead_sec?: number | null;
}
```

Every field is independently optional, and an absent one means "was not
recorded" rather than "reset this". A recorded head that no longer
exists is ignored on restore — a folder copied without `.audiograph/`
leaves one behind, and refusing to open the project over it would be
absurd.

### `listRecentProjects() → RecentProject[]`

Most recent first, one entry per project, capped at 10. Entries whose
folder has gone are pruned on read and the pruning is written back, so a
dead row is never offered twice.

```typescript
interface RecentProject {
  path: string;
  name: string;
  last_opened_at?: string | null;
}
```

### `forgetRecentProject(path: string) → RecentProject[]`

Removes the row. Does not touch the project.

---

### `batchLoad(paths: string[]) → BatchLoadResult`

Load several files in one call, each as its own track. One node for the whole
batch, so it undoes as one action rather than as N.

**Returns:**
```typescript
interface BatchLoadResult {
  loaded: string[];      // Paths that became tracks
  failed: string[];      // Paths that could not be decoded
  last_node_id: string | null;
}
```

A file that fails to decode does not fail the call — it lands in `failed` and
the rest still load.

---

### `saveProjectAs(dest: string) → CopyReport`

Copy the current project to `dest` and continue working in the copy.

**Returns:**
```typescript
interface CopyReport {
  files: number;
  bytes: number;
  dest: string;
}
```

**Errors:** no session open; `dest` exists and is not empty; the copy fails
part way (the report is not written and `dest` is left for inspection).

---

## API Key Management

### `setApiKey(key: string) → void`

Set the API key for the active provider. Stored in the OS keychain.

```typescript
await bridge.setApiKey("sk-ant-api...");
```

### `setApiKeyFor(provider: ProviderId, key: string) → void`

Set the API key for a specific provider.

```typescript
await bridge.setApiKeyFor("openrouter", "sk-or-...");
```

### `hasApiKey() → boolean`

Check whether the active provider has a key configured.

```typescript
const ready = await bridge.hasApiKey();
```

### `hasApiKeyFor(provider: ProviderId) → boolean`

Check whether a specific provider has a key.

### `clearApiKey() → void`

Remove the API key for the active provider from the keychain.

### `clearApiKeyFor(provider: ProviderId) → void`

Remove the API key for a specific provider.

### `testApiKey(key: string) → void`

Validate an API key by making a minimal (1-token) test request to the active provider.

**Throws:** Error string if validation fails (invalid key, network error, quota exceeded).

```typescript
try {
  await bridge.testApiKey("sk-ant-...");
  console.log("Key valid");
} catch (e) {
  console.error("Key invalid:", e);
}
```

### `testApiKeyFor(provider, key, baseUrl?, model?) → ProbeReport`

Probe a specific provider. `baseUrl` and `model` are the values currently
on screen, not the saved ones — the point of a test is to check settings
before committing them.

Two requests: one for reachability and credentials, one that offers the
model a trivial tool and checks whether it calls it. Tool support is a
property of the model, not the server, and every edit in edytlab is a
tool call — so a model that connects but ignores tools is reported
rather than passed.

```typescript
interface ProbeReport {
  model: string;        // the model that was probed
  toolsOk: boolean;     // did it call the tool it was offered?
  detail: string | null; // what it said instead, when it did not
}
```

**Throws:** the endpoint is unreachable or the key was rejected —
`"<status> <body>"`, e.g. `"401 invalid x-api-key"`. A model that
connects but cannot call tools resolves with `toolsOk: false` instead of
throwing: the key is fine, the model is not.

---

## Provider Selection

### `listProviders() → ProviderId[]`

Returns all configured provider IDs.

```typescript
const providers = await bridge.listProviders();
// ["anthropic", "openrouter", "openai"]
```

### `getActiveProvider() → ProviderId`

Returns the currently active provider.

### `setActiveProvider(provider: ProviderId) → void`

Switch the active provider. Rebuilds the agent with the new provider configuration.

```typescript
await bridge.setActiveProvider("openrouter");
```

---

### `getBaseUrlFor(provider: ProviderId) → string | null`

The base URL this provider has been pointed at, or `null` when it is on its
default.

---

### `defaultBaseUrlFor(provider: ProviderId) → string`

The URL the provider ships with. Shown as the field's placeholder, so the user
can see what "empty" means.

---

### `setBaseUrlFor(provider: ProviderId, baseUrl: string) → void`

Point a provider somewhere else — a proxy, a gateway, a local server. **An
empty string restores the default**, which is how the field is cleared.

---

## Model Selection

### `listModelsFor(provider: ProviderId, apiKey?: string) → ModelInfo[]`

Fetch available models from the provider's API. Results are cached for 10 minutes.

**Parameters:**
- `provider` — provider to query
- `apiKey` — optional key override (uses stored key if omitted)

**Returns:** `ModelInfo[]`
```typescript
interface ModelInfo {
  id: string;
  display_name: string;
  context_length: number | null;
  provider_hint: string | null;
}
```

**Example:**
```typescript
const models = await bridge.listModelsFor("anthropic");
// [{ id: "claude-sonnet-4-6", display_name: "Claude Sonnet 4.6", ... }, ...]
```

### `getActiveModel(provider: ProviderId) → string`

Returns the model ID currently selected for the provider.

### `setActiveModel(provider: ProviderId, model: string) → void`

Set the active model for a provider. Persisted in keychain as `active_model_<provider>`.

```typescript
await bridge.setActiveModel("anthropic", "claude-opus-4-7");
```

---

## Session Graph (DAG)

### `getSessionHead() → NodeId`

Returns the ID of the current head node.

**Throws:** If no project is open or session is empty.

### `getNode(id: NodeId) → SessionNode`

Fetch a node from the DAG by ID.

**Returns:** `SessionNode`
```typescript
interface SessionNode {
  id: NodeId;
  parent: NodeId | null;
  created_at: string;       // ISO 8601
  label: string | null;
  reasoning: string | null;
  state: SessionState;
}
```

### `getGraph() → GraphSummary`

Fetch a summary of the entire DAG (all nodes, no full state).

**Returns:**
```typescript
interface GraphSummary {
  nodes: GraphNode[];
  head: NodeId | null;
}

interface GraphNode {
  id: NodeId;
  parent: NodeId | null;
  label: string | null;
  tool: string | null;      // Tool name that created this node
  created_at: string;
}
```

**Example:**
```typescript
const graph = await bridge.getGraph();
graph.nodes.forEach(n => console.log(n.id, n.label));
```

### `setHeadTo(nodeId: NodeId) → NodeId`

Move the head pointer to any existing node. This is a non-destructive revert.

**Returns:** The new head node ID (same as `nodeId`).

### `renameNode(nodeId: NodeId, label: string) → void`

Set a human-readable label on a node.

```typescript
await bridge.renameNode("abc123", "vocals boosted +3dB");
```

---

## Rendering and A/B Compare

### `renderPreview(node: NodeId) → string`

Render the session at `node` to a WAV file for playback. Returns the path.

**Returns:** Absolute path to the rendered WAV.

**Cached.** Renders live in `<project>/.audiograph/previews/`, keyed by node
id. A node id is a hash of the session state, so asking for the same head
twice does no work and undo/redo replay renders that already exist. The cache
is bounded (1 GiB by default, overridable with `EDYTLAB_PREVIEW_CACHE_BYTES`)
and evicts least-recently-used entries; an evicted entry is re-rendered
byte-identically on demand. `storage_report` reports what it is holding.

```typescript
const wavPath = await bridge.renderPreview(nodeId);
// Use path with WaveSurfer or audio element
```

### `renderRange(node: NodeId, startSec: number, endSec: number, outPath: string) → void`

Render a time range from the session at `node` to a specific output file.

**Parameters:**
- `node` — node to render
- `startSec` — start of range in seconds
- `endSec` — end of range in seconds
- `outPath` — absolute path for the output WAV

```typescript
await bridge.renderRange(nodeId, 10.0, 45.5, "/Users/alice/export.wav");
```

### `prepareCompare(a: NodeId, b: NodeId) → { a_path: string, b_path: string }`

Pre-render two nodes for A/B comparison. Returns paths to both rendered WAV files.

```typescript
const { a_path, b_path } = await bridge.prepareCompare(nodeA, nodeB);
```

### `acceptB(b: NodeId) → NodeId`

In A/B compare mode, accept node B as the new head. Returns the new head ID.

```typescript
const newHead = await bridge.acceptB(nodeBId);
```

---

## Agent Conversation

### `sendMessage(text: string) → void`

Send a user message to the agent. The agent turn runs asynchronously and emits events back to the frontend.

**Side effects:** Emits `agent:text-delta`, `agent:tool-call`, `agent:tool-call-end`, `agent:node-created`, and `agent:done` events during processing.

**Throws:** If no API key is configured or no project is open.

```typescript
await bridge.sendMessage("cut the silence at the start and normalize to -14 LUFS");
```

### `approvePlan() → void`

In mashup mode, approve the agent's proposed plan to proceed with execution.

---

### `rejectPlan() → void`

Decline the plan the agent proposed. The turn ends; nothing is applied.

Pairs with `approvePlan()`. One of the two must be called once `onPlan` has
fired, or the turn stays suspended.

---

### `setPlanFirst(enabled: boolean) → void` · `getPlanFirst() → boolean`

Ask for a plan before **every** turn, rather than only the ones the classifier
calls mashups. Persisted, so it survives a restart.

---

### `cancelLongRunningTool() → void`

Ask the running tool to stop. Cooperative: the tool checks between units of
work — `batch_apply` between files, `timer_record` between polls — so a call
lands at the next boundary rather than immediately, and a tool that does not
check is unaffected.

Resolves as soon as the request is recorded, not when the tool actually stops.
Watch `onToolProgress` for that.

---

## Selection and Markers

### `setSelectionContext(range?: { start_sec: number, end_sec: number }) → void`

Set the current selection range for context. Pass `null` / omit to clear the selection.

The selection is injected into the next agent turn's context (the agent can reference "the selected region").

```typescript
await bridge.setSelectionContext({ start_sec: 5.0, end_sec: 20.0 });
// Clear selection:
await bridge.setSelectionContext();
```

### `addMarker(timeSec: number, name: string) → string`

Add a named marker annotation at a specific time. Returns the marker's annotation ID.

```typescript
const markerId = await bridge.addMarker(30.5, "chorus starts");
```

### `removeMarker(id: string) → NodeId`

Remove a marker by annotation ID. Returns the new head node ID.

```typescript
const newHead = await bridge.removeMarker(markerId);
```

### `listMarkers() → Marker[]`

List all markers and regions for the current session.

**Returns:**
```typescript
interface Marker {
  id: string;
  name: string;
  kind: "marker" | "region";
  time_sec?: number;     // for kind = "marker"
  start_sec?: number;    // for kind = "region"
  end_sec?: number;      // for kind = "region"
}
```

---

### `updateMarker(id: string, patch: { name?, time?, start?, end? }) → NodeId`

Edit an existing marker in place. Only the fields present in `patch` change.

`time` moves a point marker; `start` and `end` move a region's bounds. Returns
the new head.

**Errors:** no marker with that id.

---

## Transcript

### `getTranscript() → TranscriptWord[]`

The transcript at the current head. Empty until `transcribe` has run.

**Returns:**
```typescript
interface TranscriptWord {
  text: string;
  start_sec: number;
  end_sec: number;
  confidence: number;
}
```

---

### `cutTranscriptWords(track: number, fromWord: number, toWord: number) → NodeId`

Cut the half-open word range `[fromWord, toWord)` **and the audio underneath
it**, closing the gap. Returns the new head.

Indices are into the array `getTranscript()` returned; re-read it afterwards,
since the words after the cut renumber.

**Errors:** track index out of range; no transcript at the head; the range is
empty or runs past the end.

---

## Tracks

### `listTracks() → TrackSummary[]`

List all tracks in the current session head.

**Returns:**
```typescript
interface TrackSummary {
  id: string;
  name: string;
  muted: boolean;
  gain_db: number;
  audio_path: string | null;  // Path to source audio (null if track is empty)
}
```

---

### `renameTrack(track: number, name: string) → NodeId`

Rename a track. Returns the new head.

---

### `removeTrack(track: number) → NodeId`

Delete a track and everything on it. Returns the new head.

Appends an ordinary session node, so it undoes like any other edit — which is
why the UI does not confirm first.

---

### `duplicateTrack(track: number) → NodeId`

Copy a track — clips, gain, pan, effects — as a new track at the end. Returns
the new head.

---

### `setTrackGain(track: number, gainDb: number) → NodeId`

Set a track's gain in dB. Absolute, not relative. Returns the new head.

---

### `setTrackPan(track: number, pan: number) → NodeId`

Set a track's pan: `-1` hard left, `0` centre, `1` hard right. Returns the new
head.

---

### `setTrackMuted(track: number, muted: boolean) → NodeId` · `setTrackSoloed(track: number, soloed: boolean) → NodeId`

Mute or solo a track. Both return the new head.

Solo is exclusive-by-effect rather than by state: soloing a track silences the
others at render time without changing their `muted` flags, so un-soloing puts
everything back as it was.

---

### `getSyncLock() → boolean` · `setSyncLock(enabled: boolean) → NodeId`

Whether an edit that shifts time on one track shifts them all.

Read separately from `listTracks()` because it belongs to the session, not to a
track — the toggle has to show the right state the moment a project opens,
rather than after the first edit. `setSyncLock` resolves to the new head, or to
the unchanged one when the value did not change.

---

## Clips

A track split by an interior cut is several clips. These address one clip at a
time; the whole-track equivalents live under [Tracks](#tracks).

### `moveClip(track: number, clip: number, startSec: number) → NodeId`

Move one clip to a new start, in seconds from the top of the timeline. The
other clips stay where they are — `time_shift` is the whole-track version.

Clips are re-sorted by start afterwards, **so a clip dragged past its neighbour
comes back at a different index**. Re-read `listTracks()` before addressing it
again.

---

### `removeClip(track: number, clip: number) → NodeId`

Remove one clip, leaving a silent gap where it was. The other clips do not
move.

---

### `setClipEnvelope(track: number, clip: number, points: EnvelopePoint[]) → NodeId`

Replace a clip's volume automation curve. An empty array clears it.

```typescript
interface EnvelopePoint {
  time_samples: number;  // Relative to the clip's own start
  gain_db: number;
}
```

Points need not be sorted — the tool sorts them, so dragging one past its
neighbour does not need the caller to reorder first.

---

## Recording

### `startRecording() → string`

Open the default input device and start capturing. Resolves to a status string.

**Errors:** no input device; permission denied; the device is in use. All three
are reachable and none of them are distinguishable from each other by the
caller, so surface the message rather than a generic failure.

---

### `stopRecording(outputPath: string) → RecordingResult`

Stop capturing and write the take to `outputPath`.

**Returns:**
```typescript
interface RecordingResult {
  path: string;
  sample_rate: number;
  channels: number;
}
```

The WAV is written but **not** added to the session — call `loadFiles([path])`
to import it. The two steps fail separately, and the distinction matters: a
failed write means the take is gone, while a failed import means it is on disk
at the path this returned.

---

### `timerRecord(outputPath: string, schedule: { startAfterSec?: number; durationSec?: number }) → { path?: string; cancelled: boolean }`

Record on a timer: wait `startAfterSec`, capture for `durationSec`.

Both fields are optional — omitting `startAfterSec` starts now, omitting
`durationSec` records until stopped. `cancelled` is true when
`cancelLongRunningTool()` ended it early, in which case `path` may be absent.

Emits `tool-progress` events throughout; see `onToolProgress`.

---

## Templates

### `listTemplates() → TemplateInfo[]`

**Returns:**
```typescript
interface TemplateInfo {
  name: string;
  description: string;
}
```

---

### `applyTemplate(name: string) → NodeId`

Apply a session template — a starting arrangement of tracks and settings.
Returns the new head.

**Errors:** no template by that name.

---

## Plugins

### `installPlugin(source: string) → PluginInstallResult`

Install a plugin from a path or a URL.

**Returns:**
```typescript
interface PluginInstallResult {
  name: string;
  version: string;
  skills_installed: number;
  agents_installed: number;
  /** Alias of `mcp_registered`, kept for older callers. */
  mcp_keys: string[];
  mcp_registered: string[];
}
```

Any MCP servers a plugin declares are registered **disabled**, and stay that
way until the user enables them.

---

### `installBundledSkills() → number`

Copy the pre-installed skill `.md` files out of the Tauri resource bundle into
`~/.edytlab/skills/` on first launch. Returns the number copied.

Returns `0` — not an error — when the skills directory already holds `.md`
files, so a user's edits are never overwritten, and when running in dev without
the bundled-skills resource dir. Always safe to call, and non-fatal: the app
calls it on startup and ignores the result.

---

## Capabilities

### `listCapabilities() → Capabilities`

Fetch all available capabilities for the `+` menu in the chat panel.

**Returns:**
```typescript
interface Capabilities {
  tools: CapabilityDescriptor[];
  skills: CapabilityDescriptor[];
  agents: CapabilityDescriptor[];
  mcp_servers: CapabilityDescriptor[];
}

interface CapabilityDescriptor {
  name: string;
  description: string;
}
```

---

## Skills (CRUD)

### `listSkills() → SkillSummary[]`

```typescript
interface SkillSummary {
  name: string;
  description: string;
  trigger: "always" | "keywords" | "regex";
  enabled: boolean;
}
```

### `readSkill(name: string) → SkillContent`

```typescript
interface SkillContent {
  name: string;
  description: string;
  trigger: "always" | "keywords" | "regex";
  keywords: string[];
  pattern: string;
  enabled: boolean;
  body: string;
}
```

### `upsertSkill(name: string, content: SkillContent) → void`

Create or update a skill. Persisted to `~/.edytlab/skills/<name>.md`.

### `deleteSkill(name: string) → void`

Delete a skill file.

---

## Agent Profiles (CRUD)

### `listAgentProfiles() → AgentProfileSummary[]`

```typescript
interface AgentProfileSummary {
  name: string;
  description: string;
}
```

### `readAgentProfile(name: string) → AgentProfileContent`

```typescript
interface AgentProfileContent {
  name: string;
  description: string;
  model: AgentProfileModel | null;
  tools: string[] | null;   // null = all tools available
  body: string;
}

interface AgentProfileModel {
  provider: string;
  id: string;
}
```

### `upsertAgentProfile(name: string, content: AgentProfileContent) → void`

Create or update a profile. Persisted to `~/.edytlab/agents/<name>.md`.

### `deleteAgentProfile(name: string) → void`

### `getActiveAgentProfile() → string | null`

Returns the name of the active profile, or null if none is selected.

### `setActiveAgentProfile(name: string | null) → void`

Set the active profile. Pass `null` to clear (use default model + all tools).

---

## Memory

### `readMemory(scope: "global" | "project") → string`

Read the memory file for the given scope. Returns empty string if no memory exists.

```typescript
const globalMemory = await bridge.readMemory("global");
const projectMemory = await bridge.readMemory("project");
```

### `writeMemory(scope: "global" | "project", contents: string) → void`

Overwrite the memory file atomically.

```typescript
await bridge.writeMemory("project", "BPM: 128\nKey: Am\nArtist: Tushar");
```

---

## MCP Servers

### `listMcpServers() → McpServerListEntry[]`

```typescript
interface McpServerListEntry {
  id: string;
  status: "stopped" | "running" | "error";
  enabled: boolean;
}
```

### `readMcpServer(id: string) → McpServerEntry`

```typescript
interface McpServerEntry {
  id: string;
  transport: "stdio" | "sse";
  command: string;
  args: string[];
  env: Record<string, string>;
  url: string;
  headers: Record<string, string>;
  enabled: boolean;
}
```

### `upsertMcpServer(id: string, entry: McpServerEntry) → void`

Create or update an MCP server entry.

### `deleteMcpServer(id: string) → void`

### `restartMcpServer(id: string) → void`

Stop and restart a running MCP server process.

---

## Events

Subscribe to events emitted by the Tauri backend. All handlers return an `UnlistenFn` — call it to unsubscribe.

### `onTextDelta(cb: (text: string) => void) → Promise<UnlistenFn>`

Emitted for each text chunk streamed from the LLM. Append to the current assistant message.

```typescript
const unlisten = await bridge.onTextDelta((chunk) => {
  setCurrentMessage(prev => prev + chunk);
});
```

### `onToolCall(cb: (name: string, id: string) => void) → Promise<UnlistenFn>`

Emitted when the agent starts executing a tool. Use to show tool badge in the UI.

### `onToolCallEnd(cb: (id: string, ok: boolean) => void) → Promise<UnlistenFn>`

Emitted when tool execution completes. `ok = false` if the tool returned an error.

### `onNodeCreated(cb: (nodeId: string) => void) → Promise<UnlistenFn>`

Emitted when a tool appends a new node to the session DAG. Refresh the graph view and track list.

### `onAgentDone(cb: () => void) → Promise<UnlistenFn>`

Emitted when the agent turn is fully complete (no more tool calls, no more text).

### `onPlan(cb: (steps: object[]) => void) → Promise<UnlistenFn>`

Emitted in mashup mode when the agent proposes a multi-step plan before execution.

### `onMarkerChanged(cb: () => void) → Promise<UnlistenFn>`

Emitted when a marker or region annotation is added or removed. Refresh the marker list.

---

### `onToolProgress(cb: (p: ToolProgress) => void) → Promise<UnlistenFn>`

Progress from a long-running tool.

```typescript
interface ToolProgress {
  kind: string;        // "batch_apply", "timer_record", "selection", …
  index?: number;      // Absent on the final event
  total: number;
  file?: string;
  succeeded: number;
  refused: number;
  done?: boolean;
  cancelled?: boolean;
  // `select_region` reports its match on this same channel.
  start_sec?: number;
  end_sec?: number;
  matched?: string;
}
```

A tool call is a single round trip, so without this a twelve-file batch is an
unexplained pause. `batch_apply` emits one event per file plus a final `done`.

**Not everything on this channel is progress.** `select_region` reports the
region it matched here too, with `kind: "selection"` — filter on `kind` against
an allow-list rather than treating every event as something to show a progress
bar for.

---

## TypeScript Types

Full type definitions are in `apps/desktop/src/lib/tauri-bridge.ts`.

```typescript
type NodeId = string;
type ProviderId = "anthropic" | "openrouter" | "openai";

interface ProjectInfo {
  path: string;
  head: NodeId | null;
}

interface SessionNode {
  id: NodeId;
  parent: NodeId | null;
  created_at: string;
  label: string | null;
  reasoning: string | null;
  state: SessionState;
}

interface SessionState {
  tracks: Track[];
  sample_rate: number;
  length_samples: number;
  transcript: Transcript | null;
}

interface Track {
  id: string;
  name: string;
  clips: Clip[];
  gain_db: number;
  muted: boolean;
}

interface Clip {
  source_path: string;
  start_sec: number;
  duration_sec: number;
}

interface GraphSummary {
  nodes: GraphNode[];
  head: NodeId | null;
}

interface GraphNode {
  id: NodeId;
  parent: NodeId | null;
  label: string | null;
  tool: string | null;
  created_at: string;
}

interface ModelInfo {
  id: string;
  display_name: string;
  context_length: number | null;
  provider_hint: string | null;
}

interface TrackSummary {
  id: string;
  name: string;
  muted: boolean;
  gain_db: number;
  audio_path: string | null;
}

interface Marker {
  id: string;
  name: string;
  kind: "marker" | "region";
  time_sec?: number;
  start_sec?: number;
  end_sec?: number;
}

interface SkillSummary {
  name: string;
  description: string;
  trigger: string;
  enabled: boolean;
}

interface SkillContent {
  name: string;
  description: string;
  trigger: "always" | "keywords" | "regex";
  keywords: string[];
  pattern: string;
  enabled: boolean;
  body: string;
}

interface AgentProfileModel {
  provider: string;
  id: string;
}

interface AgentProfileContent {
  name: string;
  description: string;
  model: AgentProfileModel | null;
  tools: string[] | null;
  body: string;
}

type McpTransport = "stdio" | "sse";
type McpServerStatus = "stopped" | "running" | "error";

interface McpServerEntry {
  id: string;
  transport: McpTransport;
  command: string;
  args: string[];
  env: Record<string, string>;
  url: string;
  headers: Record<string, string>;
  enabled: boolean;
}

interface Capabilities {
  tools: CapabilityDescriptor[];
  skills: CapabilityDescriptor[];
  agents: CapabilityDescriptor[];
  mcp_servers: CapabilityDescriptor[];
}

interface CapabilityDescriptor {
  name: string;
  description: string;
}
```

---

*Reflects edytlab v0.1.0-dev. Coverage of `tauri-bridge.ts` is enforced by
`apps/desktop/src/__tests__/apiReferenceCoverage.test.ts` — a new export with no
entry here fails CI.*
