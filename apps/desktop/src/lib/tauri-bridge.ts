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
 * Persist the Anthropic API key to the OS keychain and (re)build the
 * agent. Rejects if the key is empty or if the keyring backend errors.
 */
export const setApiKey = (key: string): Promise<void> =>
  invoke<void>("set_api_key", { key });

/**
 * Whether the OS keychain currently holds an Anthropic API key.
 *
 * Settings.tsx calls this on mount to decide whether to render the
 * blocking first-launch modal. Reads through to the keychain on each
 * call so the answer reflects the latest state (e.g. just after
 * `clearApiKey`).
 */
export const hasApiKey = (): Promise<boolean> =>
  invoke<boolean>("has_api_key");

/**
 * Remove the stored API key, drop the in-memory cache, and tear down
 * the agent. After this resolves, `hasApiKey()` returns `false` and the
 * UI should re-render the blocking first-launch modal — no app restart
 * required.
 */
export const clearApiKey = (): Promise<void> =>
  invoke<void>("clear_api_key");

/**
 * Probe `key` against the Anthropic Messages API with a 1-token request.
 *
 * Resolves on HTTP 200 and rejects with the `"<status> <body>"` string
 * (e.g. `"401 invalid x-api-key"`) on any non-2xx or transport error.
 * The key is *not* persisted; the Settings panel calls `setApiKey`
 * separately if the test passes.
 */
export const testApiKey = (key: string): Promise<void> =>
  invoke<void>("test_api_key", { key });

/** Current session head as hex; rejects if no project is loaded. */
export const getSessionHead = (): Promise<NodeId> =>
  invoke<NodeId>("get_session_head");

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
