# edytlab — API Reference

> Complete reference for all Tauri IPC commands and the TypeScript bridge.
> Commands are invoked from the frontend via `tauri-bridge.ts`.

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
- [Tracks](#tracks)
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

**Example:**
```typescript
const project = await bridge.openProject("/Users/alice/Music/my-session");
console.log(project.head); // null | "abc123..."
```

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

*Last updated: 2026-05-17. Reflects edytlab v0.1.0-dev.*
