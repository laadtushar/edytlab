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
- [ONNX Runtime](#onnx-runtime)
- [Speech Model](#speech-model)
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
first open, and moves the project to the top of the recents list. A project
that is a copy of another and has lost audio the other still has (a copy saved
by 0.6.0, whose open swept its `derived/`, #612) has that audio copied back
into it: in the background, so the open does not wait, adding files and never
replacing one, stopping while the disk still has 1 GiB free, and skipping (and
logging) a file it cannot copy. What it leaves, the next open tries again.

**Throws:** `path` is not absolute; or the folder is one that a private writer
has open, a `batch_apply` run writing its per-file project there (#566): two
writers on one project folder would overwrite each other's history, so the
open is refused until the batch is done. A path that now names a different
folder from the one a store is open on (the project was restored from a backup
over itself, or trashed and made again) is refused too while anything but the
window holds the old store, an assistant turn most likely: wait for the turn to
end, then open the project again. A folder that a store is already open on (the
window's, or the one an assistant turn is working in) is not an error: the open
reads the folder again into that store, whichever way the path is spelt.

**Example:**
```typescript
const project = await bridge.openProject("/Users/alice/Music/my-session");
console.log(project.head); // null | "abc123..."
```

### What a project is on disk

```
my-session/
  project.json            name, created, last opened, notes
  generated/              audio the tools generate (mixes, tones, noise)
  .audiograph/
    nodes/                the edit history
    head                  where you are in it
    trail.json            the path taken to it, which undo follows
    view.json             zoom, selection, playhead
    derived/              audio produced by edits
    clipboard/            blobs a paste depends on
    previews/             render cache — disposable
    auditions/            audition_effect excerpts — disposable
```

The split is by lifetime. `project.json` is the project's identity and
sits outside the store because the store is an implementation detail.
`view.json` is disposable — losing it costs a scroll. `trail.json` is the
path taken through that history; losing it makes undo skip steps, so it sits
beside `head` and is written the same way. `previews/` and `auditions/` are
caches: every entry in `previews/` re-derives byte-identically from the node it
is named for (a whole mix by the node's id, a ranged render by a hash of the
node's id and the range), and an audition is named by its content, so they are
the two directories a copy of a project can safely skip (Save As leaves them
behind, #576).

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

### `getHeadTrail() → SavedTrail` · `saveHeadTrail(trail: SavedTrail) → void`

The path the user took through the history, kept with the project
(`.audiograph/trail.json`) so that undo retraces it after a restart (#453).

```typescript
interface SavedTrail {
  head: string | null;   // the head the path ends at
  trail: string[];       // heads left behind, oldest first
  redo: string[];        // where undo came from, most recent last
}
```

Why it exists: a node's id is the hash of its state, so an edit that returns
to an earlier state (mute, then unmute) lands on a node that already exists
and keeps the parent it had the first time. Undo cannot follow `parent`; it
follows this path. The frontend decides where undo and redo go
(`src/lib/headTrail.ts`); the backend only keeps the file.

`getHeadTrail` never fails because of the file. No file, an unreadable or
corrupt one, and one whose head the project no longer has all answer
`{ head: null, trail: [], redo: [] }`. Entries naming nodes the project no
longer has (a compaction removed them) are dropped from the lists, and ids
that are not node ids are dropped too. The app uses the path only if its
`head` is the head the project opened on; a path that ends elsewhere (a save
lost to a crash, a store another client moved) is ignored and undo falls
back on the node's parent.

`saveHeadTrail` replaces the file, written to a temporary file in the same
directory and renamed over it, so a reader sees the old file or the new
one. Each list is cut to its last 500 entries. A path whose `head` is not a
node of the open project is dropped and answered as success: the app saves
without waiting, so a save for the project just left can arrive after the
next has opened. The app calls it after every move of the head, outside the
queue that orders edits, with at most one save in flight (the newest
replaces any waiting), so no edit waits for the disk.

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

Load several files in one call, each as its own track. Every file goes through
the same `load` tool the agent uses, one after another, so each appends its own
node: undo takes back one file at a time, not the whole batch.

**Returns:**
```typescript
interface BatchLoadResult {
  tracks_loaded: number;                          // Files that became tracks
  last_node_id: string | null;                    // The head after the last of them; null with no session yet
  failures: { path: string; error: string }[];    // Files the load tool refused, with its reason
}
```

A file the tool refuses (not audio, missing, unreadable) does not fail the call
and leaves no node behind — it lands in `failures` and the rest still load.

---

### `saveProjectAs(dest: string) → CopyReport`

Copy the current project to `dest` and continue working in the copy.

The copy is a snapshot of the head at the moment of the click: that head, the
history behind it and the audio they play, whatever the agent commits while
the copy is made (#571). An edit that holds the session when Save As is
clicked finishes first and is in the copy; one still running off the session's
lock, and anything after, is not. The session's lock is held only while the
history is copied; the audio is copied with it free, so other commands are not
held up by a large project. The caches, `.audiograph/previews/` and
`.audiograph/auditions/`, are left behind, since every entry in them rebuilds
on demand (#576); a folder of the same name anywhere else is copied. The copy
plays its own audio (`.audiograph/derived/`, `clipboard/` and `generated/`)
from the moment it opens, so deleting or moving the original afterwards costs
it none of it (#612). A file the project plays from where the user keeps it (a
take loaded in place) is still played from that path: one inside the
original's folder is copied with it, but the copy goes on naming the
original's.

**Returns:**
```typescript
interface CopyReport {
  files: number;                // Files copied
  bytes: number;                // Their total size
  skipped_cache_files: number;  // Cached previews and auditions left behind
}
```

**Errors:** no session open; `dest` is not absolute, or is the project's own
folder; `dest` exists and is not empty; the copy fails part way, or is not
whole when it is checked (the head's node, or audio the head plays, is missing
from it; the error names the file). In each of the last two `dest` is left for
inspection.

---

## API Key Management

### `setApiKey(key: string) → KeychainPersistence`

Set the API key for the active provider. Stored in the OS keychain. Resolves
as `setApiKeyFor` does.

```typescript
await bridge.setApiKey("sk-ant-api...");
```

### `setApiKeyFor(provider: ProviderId, key: string) → KeychainPersistence`

Set the API key for a specific provider, and make it the active one.

Resolves with where the save landed, in the shape `getKeychainPersistence`
answers with: `persistent: false` is a save that fell back to the kernel
keyring because no Secret Service could be reached (it went away after launch,
say), so it lasts only until the machine restarts, and `reason` is why. The key
is saved and used all the same. The key and the active provider are two writes;
the answer is `persistent: false` if either fell back, with the first reason.
Settings shows the launch banner's words for it after Save. The other `set*`
functions that write to the keychain (`setActiveProvider`, `setBaseUrlFor`,
`setEffortFor`, `setActiveModel`) answer the same way.

```typescript
const { persistent, reason } = await bridge.setApiKeyFor("openrouter", "sk-or-...");
```

### `hasApiKey() → boolean`

Check whether the active provider has a key configured.

```typescript
const ready = await bridge.hasApiKey();
```

**Throws:** Error string when the system keychain could not be read (a locked keyring, a dismissed unlock prompt, no service). That is not the same as resolving `false`: `false` means no key is stored and the app asks for one, while a rejection means a key may well be stored and could not be looked up, so the app says so instead.

**Pending** while the backend's launch-time keychain reads are still going. They run off the setup thread, so the window is up while a locked keyring's unlock prompt waits on the user, and this resolves or rejects when they finish. The app shows "Waiting for your keyring" if that takes more than a moment.

### `hasApiKeyFor(provider: ProviderId) → boolean`

Check whether a specific provider has a key.

**Throws:** Error string when the keychain could not be read, as `hasApiKey` does.

### `getKeychainPersistence() → KeychainPersistence`

Whether keys and settings saved now survive a reboot. Always `persistent: true` on macOS and Windows. On Linux they live in the Secret Service (GNOME Keyring, KWallet or KeePassXC), and `persistent: false` means none is running, so they are kept in the kernel keyring until the machine restarts. `reason` is why the Secret Service could not be used. It looks up an account nothing ever stores, so it never raises an unlock prompt.

```typescript
interface KeychainPersistence {
  persistent: boolean;
  reason: string | null; // why the Secret Service could not be used
}

const { persistent, reason } = await bridge.getKeychainPersistence();
```

This is where saves go *now*, and the app shows it once at launch. The `set*` functions answer the same shape for the save they just made.

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
  modelUnavailable: boolean; // the provider has no such model (retired?)
}
```

**Throws:** the endpoint is unreachable or the key was rejected —
`"<status> <body>"`, e.g. `"401 invalid x-api-key"`. A model that
connects but cannot call tools resolves with `toolsOk: false` instead of
throwing: the key is fine, the model is not. A model the provider no longer
has (retired, renamed, or never there) resolves with `modelUnavailable: true`,
`toolsOk: false` and no `detail`: it was never asked about tools.

---

## Provider Selection

### `listProviders() → ProviderId[]`

Returns every provider ID the app supports (`SUPPORTED_PROVIDER_IDS`), whether or not a key is stored for it.

```typescript
const providers = await bridge.listProviders();
// ["anthropic", "openrouter", "openai", "groq", "gemini", "ollama"]
```

### `getActiveProvider() → ProviderId`

Returns the currently active provider.

**Throws:** Error string (the reason alone) after a startup the system keychain could not be read in, until a save goes through. The default provider is then what was never read, not an answer: a caller that saved it would overwrite the stored choice. Pending, like `hasApiKey`, while the app's launch-time keychain reads are still going.

### `setActiveProvider(provider: ProviderId) → KeychainPersistence`

Switch the active provider. Rebuilds the agent with the new provider configuration. Resolves with where the choice was saved, as `setApiKeyFor` does.

**Throws:** If the switch worked but the new provider's saved key could not be read, it rejects with that reason instead of resolving. The choice is saved.

```typescript
await bridge.setActiveProvider("openrouter");
```

---

### `getBaseUrlFor(provider: ProviderId) → string | null`

The base URL this provider has been pointed at, or `null` when it is on its
default.

**Throws:** Error string (the reason alone) when the keychain could not be
read. That is not `null`: a caller that saved the blank it took for "no
override" would delete the one that is stored, which is what Settings did
after a startup the keychain refused.

After a startup the keychain refused (a locked keyring, a dismissed unlock
prompt) it rejects without reading, saying so: each read of a locked keyring
is another unlock prompt, and Settings asks for this the moment it opens. The
refusal lifts with the first save that goes through. Pending, like `hasApiKey`,
while the launch-time reads are still going.

---

### `defaultBaseUrlFor(provider: ProviderId) → string`

The URL the provider ships with. Shown as the field's placeholder, so the user
can see what "empty" means.

---

### `setBaseUrlFor(provider: ProviderId, baseUrl: string) → KeychainPersistence`

Point a provider somewhere else — a proxy, a gateway, a local server. **An
empty string restores the default**, which is how the field is cleared.

Resolves with where the URL was saved, as `setApiKeyFor` does. Clearing writes
nothing, so it always resolves `persistent: true`: there is no value to lose.

---

### `getEffortFor(provider: ProviderId) → ReasoningEffort | null`

The reasoning effort saved for this provider, or `null` when the model's own
default applies. Only Anthropic has the setting, so any other provider answers
`null`.

**Throws:** Error string (the reason alone) when the keychain could not be
read, as `getBaseUrlFor` does, and for the same reason. Like it, rejects
without reading after a startup the keychain refused.

---

### `setEffortFor(provider: ProviderId, effort: ReasoningEffort | null) → KeychainPersistence`

Set how hard the model works on a response: sent to the Anthropic API as
`output_config.effort`. Higher effort is slower and uses more tokens, and the
model may think before it answers. **`null` (or an empty value) restores the
model's default**, which is how the setting is cleared. Rebuilds the agent.

Resolves with where the level was saved, as `setBaseUrlFor` does (clearing it
is always `persistent: true`).

Rejects a level that is not one of the five in `ReasoningEffort`, and rejects
any level for a provider other than Anthropic (OpenRouter and the
chat-completions providers have no such field, so none is ever sent to them).

```typescript
await bridge.setEffortFor("anthropic", "xhigh");
await bridge.setEffortFor("anthropic", null); // back to the model's default
```

---

## Model Selection

### `listModelsFor(provider: ProviderId, apiKey?: string) → ModelCatalogue`

Fetch available models from the provider's API, and say which list they are.
Live results are cached for 10 minutes, per provider, endpoint and key.

**Parameters:**
- `provider` — provider to query
- `apiKey` — optional key override (uses stored key if omitted)

**Throws:** Error string after a startup the keychain refused, without
reading it: the stored key and base URL are both keychain reads, and a list
fetched with a guessed endpoint is not the list of the server chat talks to.
Pending, like `hasApiKey`, while the launch-time reads are still going.

**Returns:** `ModelCatalogue`
```typescript
interface ModelCatalogue {
  models: ModelInfo[];
  // "live": the provider's own list. "fallback": edytlab's built-in list,
  // which Anthropic gets with no key, or when its list cannot be fetched.
  source: "live" | "fallback";
  notice: string | null; // why the fallback is shown; null for a live list
}

interface RenderWarning {
  scope:
    | { type: "track"; index: number; name: string }  // index as the tools' `track` takes it
    | { type: "bus"; name: string }
    | { type: "master" };
  effect_index: number;     // in the stored chain, bypassed entries counted
  kind: string;
  reason: "not_yet_streamable" | "unknown";
}

interface ModelInfo {
  id: string;
  display_name: string;
  context_length: number | null;
  provider_hint: string | null;
}
```

**Example:**
```typescript
const { models, source } = await bridge.listModelsFor("anthropic");
// source === "live"
// models: [{ id: "claude-haiku-5-5", display_name: "Claude Haiku 5.5", ... }, ...]
```

### `getActiveModel(provider: ProviderId) → string`

Returns the model ID currently selected for the provider, or an empty string
when none has been chosen.

**Throws:** Error string when the app's launch-time keychain reads could not
restore the chosen models (a locked keyring, a dismissed unlock prompt) and
nothing is known for this provider. That is not the empty string: "none
chosen" would be what the map lacks, not what the keychain says, and a caller
that saved the model it showed would overwrite the stored one. A model chosen
since, or restored before the refusal, is still answered. Pending, like
`hasApiKey`, while the launch-time reads are still going.

### `setActiveModel(provider: ProviderId, model: string) → KeychainPersistence`

Set the active model for a provider. Persisted in the keychain as `<provider>_model` (e.g. `anthropic_model`). Resolves with where the choice was saved, as `setApiKeyFor` does.

**Throws:** If the keychain refused the write, `"model selected for this session, but could not be saved: …"`: the choice applies until the app closes.

```typescript
await bridge.setActiveModel("anthropic", "claude-opus-5-5");
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
byte-identically on demand. `storage_report` reports what it is holding. A mix
bigger than the cap cannot be kept, so the call fails (the cache is left as it
was) instead of returning a path to a file that is already gone. The entry it
returns is the one the window plays, so it is held in the cache, outside the
cap, until the window's next `renderPreview` or a project change (#607): the
transport loads the file again whenever the timeline mounts (a switch to the
Graph or Transcript tab and back), and a render made meanwhile (the agent's,
an audition, a graph preview) evicts only other entries to fit. A call that
fails changes nothing, and the previous hold stays. While an A/B compare is
open its two sides are held in the same cache too (`prepareCompare`).

```typescript
const wavPath = await bridge.renderPreview(nodeId);
// Use path with WaveSurfer or audio element
```

A render never fails because of a stored effect: one the renderer cannot run in
a chain (reverb, echo, eq and the other not-yet-streamable kinds, or a kind it
has never heard of) is left out of the mix, and `getRenderWarnings` says which.
The path is all this command answers, and a cached preview has no report, so the
warnings are asked for by node rather than returned here.

### `getRenderWarnings(node: NodeId) → RenderWarning[]`

What a render of `node` leaves out: every switched-on effect in a stored chain
(a track's, a bus's, the master's) that the renderer cannot run. Read from the
node's chains alone, so it renders nothing and takes no audio or engine lock.
Empty when nothing is left out.

A node id hashes the state, so the answer for an id never changes. After the
effect is removed or bypassed the head is a different node, whose answer is
empty. The window asks for the node it shows whenever that changes (an edit,
undo, a click in the graph) and shows each warning above the timeline, naming
the track (by the index the tools take, and its name), bus or master and the
effect: "Reverb on track 0 "Vocals" is left out: this build can't run it in a
chain yet. Remove or bypass it to clear this."

**Throws:** `NoSession` with no project open; an invalid or unknown node id.

```typescript
const warnings = await bridge.getRenderWarnings(head);
// [{ scope: { type: "track", index: 0, name: "Vocals" },
//    effect_index: 1, kind: "reverb", reason: "not_yet_streamable" }]
```

### `renderRange(node: NodeId, startSec: number, endSec: number, outPath: string) → { warnings: RenderWarning[] }`

Render a time range from the session at `node` to a specific output file.

**Parameters:**
- `node` — node to render
- `startSec` — start of range in seconds
- `endSec` — end of range in seconds
- `outPath` — absolute path for the output WAV

**Returns:** `warnings`, what the file was written without: the same list
`getRenderWarnings(node)` gives, `[]` when nothing was left out. The file is
written either way; the window says so after the export.

**Errors:** rejects, and writes nothing, when `outPath` is audio the open
project reads: a recording it plays from, in its current state or in any step of
its history (#653). The message names the file and asks for another path; the
window shows it as the export's error.

```typescript
const { warnings } = await bridge.renderRange(nodeId, 10.0, 45.5, "/Users/alice/export.wav");
```

### `prepareCompare(a: NodeId, b: NodeId) → PreparedCompare`

Pre-render two nodes for A/B comparison. Returns paths to both rendered WAV
files, what each side's render leaves out (`getRenderWarnings`' list for that
node), and the id of the compare. A side that stores an effect the renderer
cannot run sounds the same with or without it, which is what a comparison
cannot afford, so the window lists both sides' warnings under the compare bar.

**Where the files are.** Each side is the preview cache's entry for its node,
`<project>/.audiograph/previews/<node>.wav`: the file `renderPreview` returns
for that node, rendered only if it is not there already (#575). A side the
window has just played is not rendered again, and neither is a pair compared
before. Nothing is written outside the project, and a cache folder that cannot
be written is an error.

**Held while compared.** The bar loads a side from its file on every switch, so
both entries are kept out of the cache's eviction until `endCompare(compare_id)`,
or until the next `prepareCompare`, which ends the compare before it. Held
entries are outside the cap, so a long session whose two sides do not fit in the
cache together (an hour-long mix) can still be compared, and a `renderPreview`
made while the compare is open evicts only other entries. The cache is over its
cap by at most the pair, and back under it once the compare ends. A side bigger
than the cap on its own is an error, as it is for `renderPreview`.

```typescript
interface PreparedCompare {
  a_path: string;
  b_path: string;
  a_warnings: RenderWarning[];
  b_warnings: RenderWarning[];
  compare_id: number;
}

const { a_path, b_path, a_warnings, b_warnings, compare_id } =
  await bridge.prepareCompare(nodeA, nodeB);
```

### `endCompare(compareId: number) → void`

The compare bar for `compareId` has closed: its two renders are ordinary
preview-cache entries again. They count against the cap once more, and the cache
is evicted back under it at once, least recently used first (usually the pair).
Ending a compare that is no longer the open one (a newer `prepareCompare` has
started since) changes nothing. The A/B bar calls it when it closes or moves to
another pair.

```typescript
await bridge.endCompare(compare_id);
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

**Side effects:** Emits `agent://text-delta`, `agent://tool-call`, `agent://tool-call-end`, `agent://node-created`, and `agent://done` events during processing (plus `agent://plan*` when a plan, or the first edit, needs approval: see `setPlanFirst`). The promise settles when the turn does, so it stays pending while the turn waits on a plan card.

**Throws:** If no API key is configured or no project is open. After a startup the system keychain refused (a locked keyring, a dismissed unlock prompt) there is no agent either, and the message says why: `no agent configured: the keychain refused access at launch (…), so it was not asked again. Press Save in Settings to try the keychain again, or unlock your keyring and restart edytlab.` The agent is not built from guesses, since building it reads the stored base URL and reasoning level; the first key or provider save that goes through builds it.

```typescript
await bridge.sendMessage("cut the silence at the start and normalize to -14 LUFS");
```

### `approvePlan(steps?: { tool: string; description: string }[]) → void`

Approve what `onPlan` showed, so the suspended turn runs it: the agent's
proposed plan, or, when there was no plan, the first edit it is about to make.

Pass `steps` as edited, each with its `tool` (which is not editable) and its
`description`, to change it instead of approving it as shown. The agent receives
each step as `tool — description`, so an edited `track: 1` still says what it is
to run with. For a plan, the agent follows the revised steps. For a held edit,
**nothing runs**: the agent is told what you changed and proposes again, and that
proposal is held in turn, so every edit that runs is one you saw as shown.

A call while no card is waiting is ignored.

---

### `rejectPlan() → void`

Decline what `onPlan` showed. The turn ends; nothing is applied.

For a held edit, the agent is told you declined it, nothing it proposed runs,
and `onPlanRejected` fires with no `onAgentDone` after it.

Pairs with `approvePlan()`. One of the two must be called once `onPlan` has
fired, or the turn stays suspended.

---

### `setPlanFirst(enabled: boolean) → void` · `getPlanFirst() → boolean`

Ask for a plan before **every** turn, rather than only the ones the classifier
calls mashups. It survives the agent rebuilds that a key, model or base-URL
change causes, and it survives a restart: `setPlanFirst()` writes it to
`~/.edytlab/settings.json` (replacing the file in one step), and the app reads
it back before the window can ask for anything. A fresh install has no file, and
a missing, unreadable or corrupt one reads as off; none of them stops the app
starting. The chat box reads it with `getPlanFirst()` when it opens, so its
toggle shows what the next turn will do.

It is an approval gate, so a save that fails is not silent. The choice still
applies until the app quits, and `setPlanFirst()` rejects with a message that
begins `could not save the Plan first setting`. The chat box then asks
`getPlanFirst()` which way it is in force and shows that message.

**With it on, no edit runs without your approval.** If the model writes a plan,
you approve the plan. If it does not (it chose not to, or planning failed for any
reason), the first step that would change the session is shown for approval
before it runs, as the exact tool calls and arguments. A turn whose tool calls
only read the session (tempo, loudness, spectrum, listing), or only audition an
effect or render a preview, is never held: those write nothing but the app's
bounded preview caches.

With it off, only a request the classifier calls a mashup is planned, and if no
plan arrives that turn proceeds with a notice and no gate.

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

### `addMarker(timeSec: number, name: string) → NodeId`

Add a named marker annotation at a specific time. Appends one node and returns
the new head. The label's own ID is in `listMarkers()`.

```typescript
const newHead = await bridge.addMarker(30.5, "chorus starts");
```

### `removeMarker(id: string) → NodeId`

Remove a marker by annotation ID. Returns the new head node ID.

```typescript
const [{ id }] = await bridge.listMarkers();
const newHead = await bridge.removeMarker(id);
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

`time` moves the label to that time: a point marker sits there, and a region
starts there and keeps its length, so dragging a region in the label lane does
not turn it into a point. `start` and `end` set a region's bounds outright and
are sent together; they are not mixed with `time`. A change that changes nothing
(a label dragged back to where it was, renamed to the name it has) appends no
node and returns the head unchanged. Otherwise one node, and the new head.

**Errors:** `time` below 0 or not finite; `start`/`end` not both given, or
`end <= start`; `time` together with `start`/`end`. An id that matches no label
changes nothing.

---

## Transcript

### `getTranscript() → Transcript`

The transcript at the current head, and where it came from. No words until
`transcribe` has run (the Transcript tab re-reads it on every head move, so a
transcription the assistant makes appears as its node arrives).

A word's time is seconds of the audio it was made from, so the Transcript tab
cuts a track by it only if the transcript is of that track (`source.kind` is
`"track"`). It was always track 0 before `transcribe` could be asked about
another track (#384).

**Returns:**
```typescript
interface TranscriptWord {
  text: string;
  start_sec: number;
  end_sec: number;
  confidence: number;
}

type TranscriptSource =
  | { kind: "track"; track: number } // the track's index at the head (found by its id, so it follows the track)
  | { kind: "file" }                 // made from a file: the times are the file's, nothing cuts by them
  | { kind: "gone" }                 // the track it was made from has been removed
  | { kind: "unknown" };             // it does not say (written before it was recorded), or there is no transcript

interface Transcript {
  words: TranscriptWord[];
  source: TranscriptSource;
}
```

---

### `cutTranscriptWords(track: number, fromWord: number, toWord: number) → NodeId`

Cut the half-open word range `[fromWord, toWord)` **and the audio underneath
it**, closing the gap. Returns the new head.

Indices are into `getTranscript().words`; re-read it afterwards, since the words
after the cut renumber. `track` is the transcript's own track
(`getTranscript().source.track`): the `cut_words` tool the command runs refuses
any other track, because the word times describe that track's audio.

**Errors:** track index out of range; no transcript at the head; the track is
not the one the transcript is of, or that track has been removed, or the
transcript is of a file; the range is empty or runs past the end.

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
  audio_path: string | null;  // The track's audio from session 0 (null if track is empty)
}
```

A project with no history yet answers `NoSession`. That is not a fault — the
project simply has no tracks — so the app treats it as an empty list (#341);
see `isNoSession` below.

---

### `isNoSession(err: unknown) → boolean`

Whether a command failed with `CommandError::NoSession` — the project has no
history yet. Matches the error's `Display` text, which is what a
`CmdResult` carries to the frontend.

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
  time_sec: number;      // Seconds from the clip's own start
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

The WAV is written but **not** added to the session — call `batchLoad([path])`
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

Keep the skill `.md` files that ship in the Tauri resource bundle current in
`~/.edytlab/skills/`. The app calls it at every launch. Returns the number of
skills written: copied in, or updated.

The folder is the user's, so the app records what it installed in
`~/.edytlab/skills/.bundled.json` and replaces a file only while it is still
the text it installed:

- a skill that is missing and was never installed is copied in;
- one that is unedited, when the bundled text has changed, is replaced (a copy
  from before the record counts as unedited if its text is a version that shipped
  before the record existed);
- one the user has edited is left alone, and the new text is written beside it
  as `<name>.bundled.md`, which neither the agent nor the Skills editor lists;
- one the user deleted is not brought back.

Every write goes through a temporary file renamed into place, and nothing is
deleted. A skill that cannot be updated is logged and skipped. When anything
was written the skill library is reloaded, so the agent sees the new text
without a restart.

Returns `0` — not an error — when nothing was due, and when running in dev
without the bundled-skills resource dir. Always safe to call, and non-fatal: the
app calls it on startup and ignores the result.

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

## ONNX Runtime

### `mlRuntimeStatus() → MlRuntimeStatus`

Which ONNX Runtime library the app found, and its version. Settings shows it as
one line: "ONNX Runtime: 1.23.2 (bundled)", "ONNX Runtime: not installed in this
build", or the reason the library it found cannot be used.

It loads the library if it has not been loaded: it opens the file by absolute
path, makes the checks `ort` makes, and hands it to `ort` once. That is file and
library work, so the command runs it on the blocking pool, and a library that
is missing, broken, or cannot be used because this macOS is too old for it is a
status, never a panic or a hang. It looks in
`ORT_DYLIB_PATH`, then the directory the installer puts the library in
(`Contents/Frameworks` on macOS, Tauri's resource directory on Linux), then next
to the executable (where the Windows installer puts it).

**Returns:**
```typescript
type MlRuntimeStatus =
  | { state: "found"; version: string; path: string;
      source: "bundled" | "environment" | "executable" }
  | { state: "missing"; searched: string[] }            // every place it looked
  | { state: "unsupportedOs"; needs: string; running: string } // macOS older than 13.4
  | { state: "loadFailed"; path: string | null; reason: string };

const status = await bridge.mlRuntimeStatus();
```

`source` says where the library came from: `bundled` is the installer's copy,
`environment` the file `ORT_DYLIB_PATH` names, `executable` one left beside the
executable on macOS or Linux (a development build).

The release installers carry ONNX Runtime 1.23.2 from v0.7.0 on (#383), so an
installed build answers `found` with `source: "bundled"`; v0.6.1 and earlier,
and a build from source, have none and answer `missing`.
The app binary prints the same answer as one line of JSON, with no window,
when started with `--ml-runtime-status` (`edytlab-desktop --ml-runtime-status`),
which is how the release workflows check every installer. A library that loads
is not a model loaded: `transcribe` loads the speech model when it is asked, and
stem separation is not shipped (#384, #385).

---

## Speech Model

The Settings → Account "Speech model" row drives these four commands and the
`ml-model-progress` event (#383). They all take `model`, the id of a model in
`crates/ml-pipeline/models.json`: today only `"whisper-base"`, Whisper base
(about 139 MB, three files pinned by commit, byte size and SHA-256, from
huggingface.co). An id that is not listed is an error naming it; nothing is
made into a path from it.

**The only way a model is downloaded is `downloadMlModel`, which the user's
click on Download calls.** Nothing downloads at launch, when Settings opens, or
from a tool or the agent, and `crates/ml-pipeline/tests/only_the_user_downloads.rs`
and `speechModelOnlyOnClick.test.ts` fail if another path could. None of these
commands takes the store, engine, dispatcher or agent lock: a download runs on a
thread of its own and never waits for, or stalls, a turn. The request goes to
huggingface.co and carries no audio. A model on disk is what the `transcribe`
tool uses (#384): it finds the files with `ml_pipeline::models::cached`, never
downloads, and answers that the model is not downloaded, with a card that sends
the user to this row, when they are not there.

```typescript
type MlModelStatus = { model: string; bytes: number; size: number } & (
  | { state: "not_downloaded" }                          // bytes > 0: a partial file a click resumes
  | { state: "downloading"; received: number; total: number }
  | { state: "ready" }                                   // every file on disk, checked against its SHA-256
  | { state: "failed"; reason: string }                  // the partial file is kept for a retry
);
```

`bytes` is how much of the model is on disk now and `size` its whole pinned
size. Every command below answers with this object, and so does the event.

### `mlModelStatus(model: string) → MlModelStatus`

Where the model is. Reads the disk, and hashes the files when they are all
there, so it runs on the blocking pool; a file that was replaced since it was
downloaded is not `ready`. It makes no network request and creates nothing.

### `downloadMlModel(model: string) → MlModelStatus`

Start downloading on a thread of its own and answer at once with `downloading`.
Progress and the end arrive on `onMlModelProgress`. A partial file from an
earlier attempt is resumed with an HTTP `Range` request, a download already
running is left alone, and a model that is already complete is verified rather
than fetched. Every file is written to a `.part` file and renamed to its final
name only once its size and SHA-256 match, so a half file never has one. Models
are kept in `<app local data>/models/<id>/` (the local, non-roaming data folder: `%LOCALAPPDATA%` on Windows).

### `cancelMlModelDownload(model: string) → MlModelStatus`

Ask a running download to stop. It stops at its next chunk and keeps the partial
file; `onMlModelProgress` says `not_downloaded` once it has. The answer is the
state at the time of the call, so it still says `downloading`. A no-op when
nothing is running.

### `removeMlModel(model: string) → MlModelStatus`

Delete the model's own files from the models folder: the files `models.json`
lists, their `.part` files, and the folders that leaves empty. Anything else in
the folder stays, and so does every other model. Refused ("being downloaded
right now") while the download runs; cancel it first. Clears a recorded failure.

### `onMlModelProgress(cb: (status: MlModelStatus) => void) → Promise<UnlistenFn>`

Emitted as `ml-model-progress` on every change of a model's state: the start,
progress (at most one every 100 ms), the end (`ready`, `failed`, or
`not_downloaded` after a cancel) and a removal. The payload is the same object
the commands answer with.

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

### `onToolCallEnd(cb: (id: string, ok: boolean, view: ToolView | undefined, notRun: boolean) => void) → Promise<UnlistenFn>`

Emitted when tool execution completes. `ok = false` if the tool returned an error. `view` is set for the few tools that return something to draw or play, such as `plot_spectrum`'s chart or `audition_effect`'s excerpt (`type: "audition"`, with `path`, `kind` and a `target` caption such as `track 1`, `the master bus` or `bus "Drums"`; `track` is there only for a track). A failed `transcribe` whose problem the user fixes carries `{ type: "speech_setup", problem }` (`"model_not_downloaded"`, `"runtime_unavailable"`, `"model_damaged"` or `"model_path_invalid"`): the window is never sent a tool's error text, so the backend makes this from the marker the error begins with, and the chat draws a card under the call's badge (Open Settings, for the two that Settings fixes).

`notRun` is `true` (with `ok = false`) when the call was announced but never
dispatched: the user declined or reworded the held step, did not answer within
five minutes, or the step would have gone past the tool budget. Nothing ran, so
show it as "not run", not as a failure.

### `onNodeCreated(cb: (nodeId: string) => void) → Promise<UnlistenFn>`

Emitted when an agent tool call moves the session's head: to a new node, or to one already in the graph (`revert_to`, `fork_node`). `nodeId` is the head the store is on after the call, not a `node_id` from the tool's result, and it is sent once per call that moved it, even one that then failed. A call that leaves the head where it was (`name_node`, a read) sends nothing. Refresh the graph view and track list.

### `onAgentDone(cb: () => void) → Promise<UnlistenFn>`

Emitted when the agent turn is fully complete (no more tool calls, no more text).

A turn can change what a graph view draws without moving the head: `name_node` sets a label and sends no `onNodeCreated`. A client that shows the graph reads it again here as well as on a head move.

### `onPlan(cb: (steps: object[]) => void) → Promise<UnlistenFn>`

Emitted when something needs approval before it runs, and the turn is suspended
until `approvePlan()` or `rejectPlan()`. Each step is
`{ step: number, tool: string, description: string }`, plus
`unavailable: true` on a step whose tool does nothing in this build
(`separate_stems`, `transcribe`: their inference is not shipped).

The agent's own plan is checked before it is shown. A step with neither a tool
nor a description is dropped, and so is a step naming `none` or a tool that is
not registered or not permitted this turn. If nothing is left there is no plan,
and the turn takes the `onPlanUnavailable` path (the reason reads "a plan with no
steps"). A step that names a tool which does nothing in this build is kept and
marked `unavailable`, so the card can say it will fail; a plan whose every step
is marked cannot be run. A step that passes is sent exactly as the agent wrote
it. The plan request names the permitted tools and states the step shape, so a
plan that follows it is not changed.

The card draws a step as the tool's badge label and the description
(`Add marker — Add marker 'Drop' at 16.0 s`). A step with no description draws
the label and any arguments the agent wrote beside it, never a bare dash.

Either the agent's own multi-step plan (mashup mode, or Plan first on), or, with
Plan first on and no plan from the agent, the first step that would change the
session. For that held edit the steps are the step's concrete tool calls, in
order, and `description` is the arguments each will run with
(`track: 0, db: 3`), not a paraphrase. Read-only calls in the same step are listed
too: approving runs all of them.

### `onMarkerChanged(cb: () => void) → Promise<UnlistenFn>`

Emitted when a marker or region annotation is added, renamed, moved or removed (`addMarker`, `updateMarker`, `removeMarker`). Refresh the marker list.

---

### `onPlanUnavailable(cb: (reason: string, firstEditHeld: boolean) => void) → Promise<UnlistenFn>`

A plan was asked for and none arrived.

`reason` names the failure class — a transport error, an HTTP status, an
unparseable body, a response with no text, a response with no `<plan>` block, or
a plan with no steps. The last two are the model choosing not to plan; the rest
are faults.

`firstEditHeld` says what happens next. `true`: Plan first is on, and the turn
will show its first edit through `onPlan` and wait for you before running it.
`false`: a request classified as a mashup with Plan first **off** proceeds
without a gate.

**Distinct from receiving no `onPlan` at all**, which means no plan was
requested for this turn. Without this event the two are indistinguishable, and a
user who turned Plan First on would watch the agent act while assuming the model
had decided no plan was needed.

---

### `onPlanRejected(cb: () => void) → Promise<UnlistenFn>`

The user declined a plan or a held edit (`rejectPlan()`), and the turn has ended
with no `onAgentDone`.

For a held edit, the agent's sentence has already streamed and its tool calls
already announced; each call's `onToolCallEnd` (`ok = false`, `notRun = true`) comes first. This
is the event that tells a transcript to settle the half-streamed message rather
than leave it pending.

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
type ProviderId = "anthropic" | "openrouter" | "openai" | "groq" | "gemini" | "ollama";
type ReasoningEffort = "low" | "medium" | "high" | "xhigh" | "max";

interface KeychainPersistence {
  persistent: boolean;      // false: Linux with no Secret Service, kept until reboot only
  reason: string | null;    // why the Secret Service could not be used
}

type MlRuntimeStatus =
  | { state: "found"; version: string; path: string; source: "bundled" | "environment" | "executable" }
  | { state: "missing"; searched: string[] }
  | { state: "unsupportedOs"; needs: string; running: string }
  | { state: "loadFailed"; path: string | null; reason: string };

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
  length_samples: number; // in session frames (sample_rate), whatever rate each track's files are at
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

interface ModelCatalogue {
  models: ModelInfo[];
  source: "live" | "fallback";
  notice: string | null;
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

*Kept current by the staleness sweep in `CLAUDE.md`; it describes `main`, not a
release. Coverage of `tauri-bridge.ts` is enforced by
`apps/desktop/src/__tests__/apiReferenceCoverage.test.ts` — a new export with no
entry here fails CI.*
