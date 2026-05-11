/**
 * Type-safe wrappers around the Tauri command + event surface exposed
 * by the Rust backend in `apps/desktop/src-tauri/src/commands.rs` and
 * `events.rs`.
 *
 * Field names and shapes are HAND-ALIGNED with the Rust types. The
 * Rust integration test in `tests/commands_mock.rs` pins the
 * serialised JSON shape of `ProjectInfo`; if you change the Rust struct
 * without updating this file, that test (or the snapshot tests in
 * `commands.rs`) will fail.
 *
 * Phase 1 keeps the surface minimal — the chat panel (M12) and the
 * settings panel (M13) consume this module directly without any
 * higher-level abstraction.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// -----------------------------------------------------------------------------
// Types — these MUST match the corresponding Rust types in
// `apps/desktop/src-tauri/src/commands.rs` and the `session` crate.
// -----------------------------------------------------------------------------

/**
 * Hex-encoded session node id (64 lowercase hex chars, blake3 of the
 * canonicalised `SessionState`).
 */
export type NodeId = string;

/**
 * Mirrors `commands::ProjectInfo`. `head` is `null` when the project
 * has no nodes yet.
 */
export interface ProjectInfo {
  path: string;
  head: NodeId | null;
}

/**
 * Mirrors `session::SessionNode`. The full `SessionState` payload is
 * deliberately untyped (`unknown`) on the frontend for now — Phase 1's
 * UI only needs the node metadata; the canvas reads waveforms via the
 * render-preview path, not by walking the state graph in JS.
 */
export interface SessionNode {
  id: NodeId;
  parent: NodeId | null;
  created_at: string;
  label: string | null;
  reasoning: string | null;
  state: SessionState;
}

/**
 * Mirrors `session::SessionState`. Kept loose on the TS side because
 * the canvas / chat UI does not interpret these fields directly; the
 * render pipeline runs in Rust. Tighten as concrete UI needs surface.
 */
export interface SessionState {
  tracks: unknown[];
  bus_routing: unknown;
  master_chain: unknown[];
  tempo_map: unknown;
  key_map: unknown | null;
  transcript: unknown | null;
  sample_rate: number;
  length_samples: number;
}

// -----------------------------------------------------------------------------
// Commands
// -----------------------------------------------------------------------------

/** Open / create a project at the given absolute path. */
export const openProject = (path: string): Promise<ProjectInfo> =>
  invoke<ProjectInfo>("open_project", { path });

/**
 * Send a chat message to the agent. The response streams via the
 * `agent://*` events; this promise resolves once the turn finishes
 * (or rejects with an error string).
 */
export const sendMessage = (text: string): Promise<void> =>
  invoke<void>("send_message", { text });

/**
 * Stable provider ids the Rust backend supports. The Settings picker
 * mirrors this list, and the keychain stores a separate key per id.
 */
export type ProviderId = "anthropic" | "openrouter" | "openai";

/**
 * One model catalogue entry returned by {@link listModelsFor}. Mirrors
 * the Rust `ai::ModelInfo` struct (re-exposed via the
 * `commands::ModelInfoDto` IPC type).
 */
export interface ModelInfo {
  id: string;
  display_name: string;
  context_length: number | null;
  provider_hint: string | null;
}

/**
 * Fetch the model catalogue for `provider`. `apiKey` is required for
 * OpenAI (the `/v1/models` endpoint is auth-gated); optional for
 * Anthropic (static list) and OpenRouter (public catalogue).
 *
 * The Rust layer caches results for 10 minutes; repeat calls within
 * that window do not re-hit the network.
 */
export const listModelsFor = (
  provider: ProviderId,
  apiKey?: string,
): Promise<ModelInfo[]> =>
  invoke<ModelInfo[]>("list_models_for", {
    provider,
    apiKey: apiKey ?? null,
  });

/**
 * Persist the chosen model id for `provider`. The next agent rebuild
 * uses this model id when constructing the LlmConfig.
 */
export const setActiveModel = (
  provider: ProviderId,
  model: string,
): Promise<void> =>
  invoke<void>("set_active_model", { provider, model });

/** Read the model id currently selected for `provider` (empty string when unset). */
export const getActiveModel = (provider: ProviderId): Promise<string> =>
  invoke<string>("get_active_model", { provider });

/**
 * Persist the API key for the active provider to the OS keychain and
 * (re)build the agent. Rejects if the key is empty or if the keyring
 * backend errors. To save against a *specific* provider (without first
 * switching to it), use {@link setApiKeyFor}.
 */
export const setApiKey = (key: string): Promise<void> =>
  invoke<void>("set_api_key", { key });

/**
 * Persist `key` for `provider`, mark `provider` as active, and rebuild
 * the agent. Used by the Settings picker when the user picks a provider
 * and saves a key against it in the same submit.
 */
export const setApiKeyFor = (
  provider: ProviderId,
  key: string,
): Promise<void> => invoke<void>("set_api_key_for", { provider, key });

/**
 * Whether the OS keychain currently holds an API key for the active
 * provider.
 *
 * Settings.tsx calls this on mount to decide whether to render the
 * blocking first-launch modal. Reads through to the keychain on each
 * call so the answer reflects the latest state (e.g. just after
 * `clearApiKey`).
 */
export const hasApiKey = (): Promise<boolean> =>
  invoke<boolean>("has_api_key");

/** Whether a key is stored for `provider` (without changing the active provider). */
export const hasApiKeyFor = (provider: ProviderId): Promise<boolean> =>
  invoke<boolean>("has_api_key_for", { provider });

/**
 * Remove the stored API key for the active provider, drop the in-memory
 * cache, and tear down the agent. After this resolves, `hasApiKey()`
 * returns `false` and the UI should re-render the blocking
 * first-launch modal — no app restart required.
 */
export const clearApiKey = (): Promise<void> =>
  invoke<void>("clear_api_key");

/** Remove the stored API key for `provider`. */
export const clearApiKeyFor = (provider: ProviderId): Promise<void> =>
  invoke<void>("clear_api_key_for", { provider });

/**
 * Probe `key` against the active provider's Messages endpoint with a
 * 1-token request.
 *
 * Resolves on HTTP 200 and rejects with the `"<status> <body>"` string
 * (e.g. `"401 invalid x-api-key"`) on any non-2xx or transport error.
 * The key is *not* persisted; the Settings panel calls `setApiKey`
 * separately if the test passes.
 */
export const testApiKey = (key: string): Promise<void> =>
  invoke<void>("test_api_key", { key });

/** Probe `key` against `provider`'s endpoint specifically. */
export const testApiKeyFor = (
  provider: ProviderId,
  key: string,
): Promise<void> => invoke<void>("test_api_key_for", { provider, key });

/** List the provider ids the Rust backend supports. */
export const listProviders = (): Promise<ProviderId[]> =>
  invoke<ProviderId[]>("list_providers");

/** Currently active provider id. Defaults to `"anthropic"`. */
export const getActiveProvider = (): Promise<ProviderId> =>
  invoke<ProviderId>("get_active_provider");

/** Switch the active provider. Persists the choice and rebuilds the agent. */
export const setActiveProvider = (provider: ProviderId): Promise<void> =>
  invoke<void>("set_active_provider", { provider });

/** Current session head as hex; rejects if no project is loaded. */
export const getSessionHead = (): Promise<NodeId> =>
  invoke<NodeId>("get_session_head");

// -----------------------------------------------------------------------------
// memory: user-editable system-prompt fragments
// -----------------------------------------------------------------------------

/** Two memory files, mirroring `memory::Scope` on the Rust side. */
export type MemoryScope = "global" | "project";

/**
 * Read the contents of the memory file for `scope`. Returns `""` if
 * the file is missing. Rejects with `"project scope requested but no
 * project is open"` when `scope === "project"` and no project is open.
 */
export const readMemory = (scope: MemoryScope): Promise<string> =>
  invoke<string>("read_memory", { scope });

/**
 * Replace the memory file for `scope`. Creates the parent directory
 * if missing; writes atomically. Empty `contents` truncates the file.
 */
export const writeMemory = (
  scope: MemoryScope,
  contents: string,
): Promise<void> => invoke<void>("write_memory", { scope, contents });

// -----------------------------------------------------------------------------
// skills: one markdown file per skill at ~/.edytlab/skills/*.md
// -----------------------------------------------------------------------------

/** Mirrors `commands::SkillSummary` on the Rust side. `trigger` is a
 *  short, human-readable summary like `"always"` /
 *  `"keywords: mix, sidechain"` / `"regex"`. */
export interface SkillSummary {
  name: string;
  description: string;
  trigger: string;
  enabled: boolean;
}

/**
 * Rescan `~/.edytlab/skills/` and return the current set of skills.
 * The Rust side reloads on every call so newly-dropped files show up
 * without an app restart.
 */
export const listSkills = (): Promise<SkillSummary[]> =>
  invoke<SkillSummary[]>("list_skills");

/** Round-trip shape for the skill editor. Mirrors `commands::SkillContent`. */
export interface SkillContent {
  name: string;
  description: string;
  trigger: "always" | "keywords" | "regex";
  keywords: string[];
  pattern: string;
  enabled: boolean;
  body: string;
}

/** Read one skill from disk into the editor shape. */
export const readSkill = (name: string): Promise<SkillContent> =>
  invoke<SkillContent>("read_skill", { name });

/**
 * Create or replace a skill. `name` must equal `content.name`. Writes
 * atomically and triggers an in-process library reload so the next
 * agent turn sees the change.
 */
export const upsertSkill = (
  name: string,
  content: SkillContent,
): Promise<void> => invoke<void>("upsert_skill", { name, content });

/** Delete the skill file. Missing files are a no-op. */
export const deleteSkill = (name: string): Promise<void> =>
  invoke<void>("delete_skill", { name });

// -----------------------------------------------------------------------------
// agent profiles — `~/.edytlab/agents/*.md`
// -----------------------------------------------------------------------------

/** Mirrors `commands::AgentProfileModel`. */
export interface AgentProfileModel {
  provider: string;
  id: string;
}

/** Mirrors `commands::AgentProfileSummary`. */
export interface AgentProfileSummary {
  name: string;
  description: string;
  model: AgentProfileModel | null;
  tool_count: number | null;
}

/** Mirrors `commands::AgentProfileContent` — round-trip for the editor. */
export interface AgentProfileContent {
  name: string;
  description: string;
  /** null = use global default model. */
  model: AgentProfileModel | null;
  /** null = all tools; [] = no tools. */
  tools: string[] | null;
  body: string;
}

export const listAgentProfiles = (): Promise<AgentProfileSummary[]> =>
  invoke<AgentProfileSummary[]>("list_agent_profiles");

export const readAgentProfile = (name: string): Promise<AgentProfileContent> =>
  invoke<AgentProfileContent>("read_agent_profile", { name });

export const upsertAgentProfile = (
  name: string,
  content: AgentProfileContent,
): Promise<void> => invoke<void>("upsert_agent_profile", { name, content });

export const deleteAgentProfile = (name: string): Promise<void> =>
  invoke<void>("delete_agent_profile", { name });

export const getActiveAgentProfile = (): Promise<string | null> =>
  invoke<string | null>("get_active_agent_profile");

/** Pass `null` to clear the active profile. */
export const setActiveAgentProfile = (name: string | null): Promise<void> =>
  invoke<void>("set_active_agent_profile", { name });

/** Look up a single session node by id. */
export const getNode = (id: NodeId): Promise<SessionNode> =>
  invoke<SessionNode>("get_node", { id });

/**
 * One node entry returned by {@link getGraph}. Mirrors the Rust
 * `commands::GraphNode` struct. `tool` is best-effort — it's the first
 * whitespace-delimited token of `label`, so callers should treat it as
 * a display hint and fall back to `label` (or `id`) when it's null.
 */
export interface GraphNode {
  id: NodeId;
  parent: NodeId | null;
  label: string | null;
  tool: string | null;
  /** RFC 3339 / ISO-8601 timestamp. Parse with `new Date(...)`. */
  created_at: string;
}

/**
 * The shape returned by {@link getGraph}. `head` is null when the
 * project has no nodes yet.
 */
export interface GraphSummary {
  nodes: GraphNode[];
  head: NodeId | null;
}

/**
 * Fetch every node in the current project's session store plus the
 * current head pointer. Used by the M25 graph view to render the DAG.
 *
 * Rejects with `"no session loaded"` if no project is open.
 */
export const getGraph = (): Promise<GraphSummary> =>
  invoke<GraphSummary>("get_graph");

/**
 * Render `node` to a temporary WAV and return the absolute path.
 * Phase 1 callers play this back via the audio engine; Phase 2 will
 * cache renders per project.
 */
export const renderPreview = (node: NodeId): Promise<string> =>
  invoke<string>("render_preview", { node });

/**
 * Pre-render both sides of an A/B compare to stable temp WAV paths so
 * the toggle is gapless. Returns `{ a_path, b_path }`.
 *
 * The paths are stable (`compare_a.wav` / `compare_b.wav` in the OS
 * temp dir) so repeated calls overwrite the previous render — callers
 * should treat the returned paths as valid only until the next
 * `prepareCompare` call.
 */
export const prepareCompare = (
  a: NodeId,
  b: NodeId,
): Promise<{ a_path: string; b_path: string }> =>
  invoke<{ a_path: string; b_path: string }>("prepare_compare", { a, b });

/**
 * Accept the B side of an A/B compare: promote node `b` to the session
 * head. Returns the new head hex so the caller can update local state
 * without a separate `getSessionHead` round-trip.
 */
export const acceptB = (b: NodeId): Promise<string> =>
  invoke<string>("accept_b", { b });

// -----------------------------------------------------------------------------
// Events
// -----------------------------------------------------------------------------

/**
 * Subscribe to streamed assistant text deltas. The promise resolves
 * with an `unlisten` function the caller MUST invoke on unmount to
 * avoid duplicate listeners.
 */
export const onTextDelta = (
  cb: (text: string) => void,
): Promise<UnlistenFn> =>
  listen<{ text: string }>("agent://text-delta", (e) => cb(e.payload.text));

/** Subscribe to tool-call start events. */
export const onToolCall = (
  cb: (name: string, id: string) => void,
): Promise<UnlistenFn> =>
  listen<{ name: string; id: string }>("agent://tool-call", (e) =>
    cb(e.payload.name, e.payload.id),
  );

/**
 * Subscribe to tool-call end events. The `id` matches the one carried
 * by the matching `agent://tool-call` event. `ok` is `false` for schema
 * validation errors and tool-level errors alike.
 */
export const onToolCallEnd = (
  cb: (id: string, ok: boolean) => void,
): Promise<UnlistenFn> =>
  listen<{ id: string; ok: boolean }>("agent://tool-call-end", (e) =>
    cb(e.payload.id, e.payload.ok),
  );

/** Subscribe to "a new session node was created by the agent" events. */
export const onNodeCreated = (
  cb: (nodeId: NodeId) => void,
): Promise<UnlistenFn> =>
  listen<{ node_id: string }>("agent://node-created", (e) =>
    cb(e.payload.node_id),
  );

/** Subscribe to "agent turn finished" events. */
export const onAgentDone = (cb: () => void): Promise<UnlistenFn> =>
  listen<Record<string, never>>("agent://done", () => cb());

/**
 * Approve the pending mashup plan, unblocking the agent loop.
 *
 * Call this when the user clicks "Run" on the plan approval card.
 * Rejects if no agent is configured (no API key / no open project).
 */
export const approvePlan = (): Promise<void> =>
  invoke<void>("approve_plan");

/**
 * Subscribe to "mashup plan ready" events. The callback receives the
 * ordered plan steps; the frontend should render an approval card and
 * call {@link approvePlan} when the user clicks Run.
 *
 * The promise resolves with an `unlisten` function the caller MUST
 * invoke on unmount to avoid duplicate listeners.
 */
export const onPlan = (
  cb: (steps: Record<string, unknown>[]) => void,
): Promise<UnlistenFn> =>
  listen<{ steps: Record<string, unknown>[] }>("agent://plan", (e) =>
    cb(e.payload.steps),
  );

// ---- Marker / selection IPC (Phase audacity-surface) ----

/**
 * A point marker annotation. `kind` is the serde tag value.
 * Matches `AnnotationKind::Marker` serialised with
 * `#[serde(tag = "kind", rename_all = "snake_case")]`.
 */
export interface MarkerAnnotation {
  id: string;
  name: string;
  kind: "marker";
  time_sec: number;
}

/**
 * A region annotation spanning `start_sec`–`end_sec`.
 * Matches `AnnotationKind::Region`.
 */
export interface RegionAnnotation {
  id: string;
  name: string;
  kind: "region";
  start_sec: number;
  end_sec: number;
}

/** Union of all annotation shapes. Discriminated by `kind`. */
export type Marker = MarkerAnnotation | RegionAnnotation;

/** Push the current timeline selection to Rust so the next send_message includes it in SessionContext. */
export const setSelectionContext = (
  range: { start_sec: number; end_sec: number } | null,
): Promise<void> => invoke("set_selection_context", { range });

/** Place a named marker at `time` seconds and append a new session node. */
export const addMarker = (time: number, name: string): Promise<string> =>
  invoke<string>("add_marker", { time, name });

/** Remove a marker by its annotation id (UUID string). Returns the new head hex. */
export const removeMarker = (id: string): Promise<string> =>
  invoke<string>("remove_marker", { id });

/** List all annotations at the current head. */
export const listMarkers = (): Promise<Marker[]> =>
  invoke<Marker[]>("list_markers");

/** Subscribe to "marker-changed" events. Returns unlisten fn. */
export const onMarkerChanged = (cb: () => void): Promise<UnlistenFn> =>
  listen("marker-changed", () => cb());

// -----------------------------------------------------------------------------
// Capabilities — drives the `+` menu in the composer.
// -----------------------------------------------------------------------------

/**
 * One capability advertised by the backend. `category` is a coarse
 * grouping ("session", "analysis", "audio", "history", "annotation").
 * Mirrors `commands::CapabilityDescriptor`.
 */
export interface CapabilityDescriptor {
  name: string;
  description: string;
  category: string;
}

/**
 * The shape returned by {@link listCapabilities}. `skills`, `agents`,
 * and `mcp_servers` are placeholders for future surfaces (see
 * `docs/specs/agentic-chat-ui.md`); the backend currently returns empty
 * arrays for them but the menu still renders the group with a
 * "coming soon" affordance.
 */
export interface Capabilities {
  tools: CapabilityDescriptor[];
  skills: CapabilityDescriptor[];
  agents: CapabilityDescriptor[];
  mcp_servers: CapabilityDescriptor[];
}

/** List every capability the agent can currently invoke. */
export const listCapabilities = (): Promise<Capabilities> =>
  invoke<Capabilities>("list_capabilities");
