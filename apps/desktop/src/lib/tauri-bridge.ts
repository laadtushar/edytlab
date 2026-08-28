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

export type NodeId = string;

export interface ProjectInfo {
  path: string;
  head: NodeId | null;
}

export interface SessionNode {
  id: NodeId;
  parent: NodeId | null;
  created_at: string;
  label: string | null;
  reasoning: string | null;
  state: SessionState;
}

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

export const openProject = (path: string): Promise<ProjectInfo> =>
  invoke<ProjectInfo>("open_project", { path });

export const sendMessage = (text: string, disabledTools?: string[]): Promise<void> =>
  invoke<void>("send_message", { text, disabledTools: disabledTools ?? [] });

export type ProviderId =
  | "anthropic"
  | "openrouter"
  | "openai"
  | "groq"
  | "gemini"
  | "ollama";

export interface ModelInfo {
  id: string;
  display_name: string;
  context_length: number | null;
  provider_hint: string | null;
}

export const listModelsFor = (
  provider: ProviderId,
  apiKey?: string,
): Promise<ModelInfo[]> =>
  invoke<ModelInfo[]>("list_models_for", {
    provider,
    apiKey: apiKey ?? null,
  });

export const setActiveModel = (
  provider: ProviderId,
  model: string,
): Promise<void> =>
  invoke<void>("set_active_model", { provider, model });

export const getActiveModel = (provider: ProviderId): Promise<string> =>
  invoke<string>("get_active_model", { provider });

export const setApiKey = (key: string): Promise<void> =>
  invoke<void>("set_api_key", { key });

export const setApiKeyFor = (
  provider: ProviderId,
  key: string,
): Promise<void> => invoke<void>("set_api_key_for", { provider, key });

export const hasApiKey = (): Promise<boolean> =>
  invoke<boolean>("has_api_key");

export const hasApiKeyFor = (provider: ProviderId): Promise<boolean> =>
  invoke<boolean>("has_api_key_for", { provider });

export const clearApiKey = (): Promise<void> =>
  invoke<void>("clear_api_key");

export const clearApiKeyFor = (provider: ProviderId): Promise<void> =>
  invoke<void>("clear_api_key_for", { provider });

export const testApiKey = (key: string): Promise<void> =>
  invoke<void>("test_api_key", { key });

/**
 * What a TEST press found out. A rejection still throws — this is the
 * success side, and it has two halves: `toolsOk` false means the
 * endpoint answered but the model ignored the tool it was offered, so
 * every edit would fail. `detail` is what it said instead.
 */
export interface ProbeReport {
  model: string;
  toolsOk: boolean;
  detail: string | null;
}

/**
 * `baseUrl` and `model` are the values currently typed into Settings,
 * not the saved ones — testing an endpoint before saving it is the
 * point of the button.
 */
export const testApiKeyFor = (
  provider: ProviderId,
  key: string,
  baseUrl?: string,
  model?: string,
): Promise<ProbeReport> =>
  invoke<ProbeReport>("test_api_key_for", {
    provider,
    key,
    baseUrl: baseUrl ?? null,
    model: model ?? null,
  });

export const listProviders = (): Promise<ProviderId[]> =>
  invoke<ProviderId[]>("list_providers");

export const getActiveProvider = (): Promise<ProviderId> =>
  invoke<ProviderId>("get_active_provider");

export const setActiveProvider = (provider: ProviderId): Promise<void> =>
  invoke<void>("set_active_provider", { provider });

/** A provider's base-URL override, or null when it uses the built-in one. */
export const getBaseUrlFor = (provider: ProviderId): Promise<string | null> =>
  invoke<string | null>("get_base_url_for", { provider });

/** The URL a provider ships with — shown as the placeholder. */
export const defaultBaseUrlFor = (provider: ProviderId): Promise<string> =>
  invoke<string>("default_base_url_for", { provider });

/** Point a provider elsewhere. An empty string restores the default. */
export const setBaseUrlFor = (
  provider: ProviderId,
  baseUrl: string,
): Promise<void> => invoke<void>("set_base_url_for", { provider, baseUrl });

export const getSessionHead = (): Promise<NodeId> =>
  invoke<NodeId>("get_session_head");

// -----------------------------------------------------------------------------
// memory
// -----------------------------------------------------------------------------

export type MemoryScope = "global" | "project";

export const readMemory = (scope: MemoryScope): Promise<string> =>
  invoke<string>("read_memory", { scope });

export const writeMemory = (
  scope: MemoryScope,
  contents: string,
): Promise<void> => invoke<void>("write_memory", { scope, contents });

// -----------------------------------------------------------------------------
// skills
// -----------------------------------------------------------------------------

export interface SkillSummary {
  name: string;
  description: string;
  trigger: string;
  enabled: boolean;
}

export const listSkills = (): Promise<SkillSummary[]> =>
  invoke<SkillSummary[]>("list_skills");

export interface SkillContent {
  name: string;
  description: string;
  trigger: "always" | "keywords" | "regex";
  keywords: string[];
  pattern: string;
  enabled: boolean;
  body: string;
}

export const readSkill = (name: string): Promise<SkillContent> =>
  invoke<SkillContent>("read_skill", { name });

export const upsertSkill = (
  name: string,
  content: SkillContent,
): Promise<void> => invoke<void>("upsert_skill", { name, content });

export const deleteSkill = (name: string): Promise<void> =>
  invoke<void>("delete_skill", { name });

// -----------------------------------------------------------------------------
// agent profiles
// -----------------------------------------------------------------------------

export interface AgentProfileModel {
  provider: string;
  id: string;
}

export interface AgentProfileSummary {
  name: string;
  description: string;
  model: AgentProfileModel | null;
  tool_count: number | null;
}

export interface AgentProfileContent {
  name: string;
  description: string;
  model: AgentProfileModel | null;
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

export const setActiveAgentProfile = (name: string | null): Promise<void> =>
  invoke<void>("set_active_agent_profile", { name });

// -----------------------------------------------------------------------------
// MCP servers — `~/.edytlab/mcp.json`
// -----------------------------------------------------------------------------

export type McpTransport = "stdio" | "sse";
export type McpServerStatus = "stopped" | "running" | "error";

export interface McpServerEntry {
  id: string;
  transport: McpTransport;
  command: string;
  args: string[];
  env: Record<string, string>;
  url: string;
  headers: Record<string, string>;
  enabled: boolean;
}

export interface McpServerListEntry {
  id: string;
  transport: McpTransport;
  enabled: boolean;
  status: McpServerStatus;
  tools_count: number;
  last_error: string | null;
}

export const listMcpServers = (): Promise<McpServerListEntry[]> =>
  invoke<McpServerListEntry[]>("list_mcp_servers");

export const readMcpServer = (id: string): Promise<McpServerEntry> =>
  invoke<McpServerEntry>("read_mcp_server", { id });

export const upsertMcpServer = (
  id: string,
  entry: McpServerEntry,
): Promise<void> => invoke<void>("upsert_mcp_server", { id, entry });

export const deleteMcpServer = (id: string): Promise<void> =>
  invoke<void>("delete_mcp_server", { id });

export const restartMcpServer = (id: string): Promise<void> =>
  invoke<void>("restart_mcp_server", { id });

// -----------------------------------------------------------------------------
// session graph
// -----------------------------------------------------------------------------

export const getNode = (id: NodeId): Promise<SessionNode> =>
  invoke<SessionNode>("get_node", { id });

export interface GraphNode {
  id: NodeId;
  parent: NodeId | null;
  label: string | null;
  tool: string | null;
  created_at: string;
}

export interface GraphSummary {
  nodes: GraphNode[];
  head: NodeId | null;
}

export const getGraph = (): Promise<GraphSummary> =>
  invoke<GraphSummary>("get_graph");

export const renderPreview = (node: NodeId): Promise<string> =>
  invoke<string>("render_preview", { node });

export async function renderRange(
  nodeId: string,
  startSec: number,
  endSec: number,
  outPath: string,
): Promise<void> {
  await invoke("render_range", {
    nodeId,
    startSec,
    endSec,
    outPath,
  });
}

export const prepareCompare = (
  a: NodeId,
  b: NodeId,
): Promise<{ a_path: string; b_path: string }> =>
  invoke<{ a_path: string; b_path: string }>("prepare_compare", { a, b });

export const acceptB = (b: NodeId): Promise<string> =>
  invoke<string>("accept_b", { b });

// -----------------------------------------------------------------------------
// Events
// -----------------------------------------------------------------------------

export const onTextDelta = (
  cb: (text: string) => void,
): Promise<UnlistenFn> =>
  listen<{ text: string }>("agent://text-delta", (e) => cb(e.payload.text));

export const onToolCall = (
  cb: (name: string, id: string) => void,
): Promise<UnlistenFn> =>
  listen<{ name: string; id: string }>("agent://tool-call", (e) =>
    cb(e.payload.name, e.payload.id),
  );

export interface SpectrumPoint {
  hz: number;
  db: number;
}

/**
 * The drawable projection of a tool result — mirrors `ai::ToolView`.
 * Tagged on `type`, so adding a second chart kind here means adding a
 * variant there.
 */
export type ToolView = {
  type: "spectrum";
  points: SpectrumPoint[];
  summary?: string | null;
};

export const onToolCallEnd = (
  cb: (id: string, ok: boolean, view?: ToolView) => void,
): Promise<UnlistenFn> =>
  listen<{ id: string; ok: boolean; view?: ToolView }>(
    "agent://tool-call-end",
    (e) => cb(e.payload.id, e.payload.ok, e.payload.view),
  );

export const onNodeCreated = (
  cb: (nodeId: NodeId) => void,
): Promise<UnlistenFn> =>
  listen<{ node_id: string }>("agent://node-created", (e) =>
    cb(e.payload.node_id),
  );

export const onAgentDone = (cb: () => void): Promise<UnlistenFn> =>
  listen<Record<string, never>>("agent://done", () => cb());

export const approvePlan = (steps?: string[]): Promise<void> =>
  invoke<void>("approve_plan", { steps: steps && steps.length > 0 ? steps : null });

/**
 * Decline a plan. The waiting turn ends having run no tools and
 * appended no node — previously the only exits were approving it or
 * waiting out a five-minute timeout.
 */
export const rejectPlan = (): Promise<void> => invoke<void>("reject_plan");

/** Ask for a plan before every turn, not only mashup-classified ones. */
export const setPlanFirst = (enabled: boolean): Promise<void> =>
  invoke<void>("set_plan_first", { enabled });

export const getPlanFirst = (): Promise<boolean> =>
  invoke<boolean>("get_plan_first");

export const onPlan = (
  cb: (steps: Record<string, unknown>[]) => void,
): Promise<UnlistenFn> =>
  listen<{ steps: Record<string, unknown>[] }>("agent://plan", (e) =>
    cb(e.payload.steps),
  );

// ---- Marker / selection IPC ----

export interface MarkerAnnotation {
  id: string;
  name: string;
  kind: "marker";
  time_sec: number;
}

export interface RegionAnnotation {
  id: string;
  name: string;
  kind: "region";
  start_sec: number;
  end_sec: number;
}

export type Marker = MarkerAnnotation | RegionAnnotation;

export const setSelectionContext = (
  range: { start_sec: number; end_sec: number } | null,
): Promise<void> => invoke("set_selection_context", { range });

export const addMarker = (time: number, name: string): Promise<string> =>
  invoke<string>("add_marker", { time, name });

export const removeMarker = (id: string): Promise<string> =>
  invoke<string>("remove_marker", { id });

/**
 * Rename and/or move a label (#203 §1).
 *
 * One call rather than a rename and a move, so a drag-and-rename is one
 * undo step rather than two, and so a caller changing only the name does
 * not have to read the position back to leave it alone. Omitted fields
 * are left as they are; a change that changes nothing appends no node
 * and returns the head unchanged.
 */
export const updateMarker = (
  id: string,
  patch: { name?: string; time?: number; start?: number; end?: number },
): Promise<string> => invoke<string>("update_marker", { id, ...patch });

// ---- Transcript IPC (#157) ----

export interface TranscriptWord {
  text: string;
  start_sec: number;
  end_sec: number;
  confidence: number;
}

/**
 * The transcript at the current head.
 *
 * An empty list is the honest answer for "not transcribed yet" — that
 * is an ordinary state, not a fault, and the pane says so from the
 * empty list rather than from an error.
 */
export const getTranscript = (): Promise<TranscriptWord[]> =>
  invoke<TranscriptWord[]>("get_transcript");

/** Cut `[fromWord, toWord)` and the audio underneath. Returns the new head. */
export const cutTranscriptWords = (
  track: number,
  fromWord: number,
  toWord: number,
): Promise<string> =>
  invoke<string>("cut_transcript_words", { track, fromWord, toWord });

export const listMarkers = (): Promise<Marker[]> =>
  invoke<Marker[]>("list_markers");

/**
 * Progress from a long-running tool (#169 §1).
 *
 * `batch_apply` emits one event per file plus a final `done`. A tool
 * call is a single round trip, so without this a twelve-file batch is
 * an unexplained pause.
 */
export interface ToolProgress {
  kind: string;
  /** Absent on the final event. */
  index?: number;
  total: number;
  file?: string;
  succeeded: number;
  refused: number;
  done?: boolean;
  cancelled?: boolean;
  /**
   * `select_region` reports the region it matched on this same channel
   * (`kind: "selection"`). It is not progress — see `ToolProgressBar`'s
   * allow-list — and these are the fields that make it useful (#252).
   */
  start_sec?: number;
  end_sec?: number;
  matched?: string;
}

export const onToolProgress = (
  cb: (p: ToolProgress) => void,
): Promise<UnlistenFn> =>
  listen<ToolProgress>("tool-progress", (e) => cb(e.payload));

/**
 * Record unattended: begin after a delay, stop after a duration
 * (#203 §2). Either half alone is useful — "start in ten minutes" and
 * "record for thirty seconds" are different requests.
 *
 * Resolves when the take is saved. Progress and the countdown arrive on
 * the same channel as batch progress, and `cancelLongRunningTool`
 * stops it — during the countdown as well as during the take.
 */
export const timerRecord = (
  outputPath: string,
  schedule: { startAfterSec?: number; durationSec?: number },
): Promise<{ path?: string; cancelled: boolean }> =>
  invoke("timer_record", {
    outputPath,
    startAfterSec: schedule.startAfterSec ?? null,
    durationSec: schedule.durationSec ?? null,
  });

/** Ask the running tool to stop at its next checkpoint. */
export const cancelLongRunningTool = (): Promise<void> =>
  invoke("cancel_long_running_tool");

export const onMarkerChanged = (cb: () => void): Promise<UnlistenFn> =>
  listen("marker-changed", () => cb());

// -----------------------------------------------------------------------------
// Tracks
// -----------------------------------------------------------------------------

/** One automation point, measured from the clip's own start. */
export interface EnvelopePoint {
  time_sec: number;
  gain_db: number;
}

export interface ClipSummary {
  start_sec: number;
  length_sec: number;
  source_path: string;
  volume_envelope: EnvelopePoint[];
}

export interface TrackSummary {
  id: string;
  name: string;
  muted: boolean;
  gain_db: number;
  /** -1 hard left, 0 centre, 1 hard right. */
  pan: number;
  soloed: boolean;
  /** `null` when the track has zero or multiple clips. */
  audio_path: string | null;
  clips: ClipSummary[];
}

export const listTracks = (): Promise<TrackSummary[]> =>
  invoke<TrackSummary[]>("list_tracks");

// Mixer controls. Each appends one undoable session node and resolves
// to the new head, so the caller refreshes with `listTracks` after.
// The backend validates ranges too — these are not the only guard.

export const setTrackGain = (track: number, gainDb: number): Promise<string> =>
  invoke<string>("set_track_gain", { track, gainDb });

export const setTrackPan = (track: number, pan: number): Promise<string> =>
  invoke<string>("set_track_pan", { track, pan });

export const setTrackMuted = (track: number, muted: boolean): Promise<string> =>
  invoke<string>("set_track_muted", { track, muted });

/**
 * Sync-lock: whether an edit that shifts time on one track shifts them
 * all (#170 §3).
 *
 * Read separately from `listTracks` because it belongs to the session,
 * not to a track — the toggle has to show the right state the moment a
 * project opens rather than after the first edit.
 */
export const getSyncLock = (): Promise<boolean> =>
  invoke<boolean>("get_sync_lock");

/** Resolves to the new head, or the unchanged one if nothing changed. */
export const setSyncLock = (enabled: boolean): Promise<string> =>
  invoke<string>("set_sync_lock", { enabled });

export const setTrackSoloed = (
  track: number,
  soloed: boolean,
): Promise<string> => invoke<string>("set_track_soloed", { track, soloed });

/**
 * Track-head actions (#161). Each appends an ordinary session node and
 * returns the new head, so they undo like anything else — which is why
 * `removeTrack` does not need a confirmation down here.
 */
/**
 * A project is a thing with a name, not only a folder path (#156).
 */
export interface ProjectMeta {
  name: string;
  created_at?: string | null;
  last_opened_at?: string | null;
  notes?: string;
}

/** Where the user was, so reopening resumes instead of restarting. */
export interface ViewState {
  head?: string | null;
  zoom_px_per_sec?: number | null;
  /** `[start, end]` in session seconds. */
  selection?: [number, number] | null;
  playhead_sec?: number | null;
}

export interface RecentProject {
  path: string;
  name: string;
  last_opened_at?: string | null;
}

export const getProjectMeta = (): Promise<ProjectMeta> =>
  invoke<ProjectMeta>("get_project_meta");

export const setProjectMeta = (
  name: string,
  notes?: string,
): Promise<ProjectMeta> =>
  invoke<ProjectMeta>("set_project_meta", { name, notes: notes ?? null });

export const getViewState = (): Promise<ViewState> =>
  invoke<ViewState>("get_view_state");

export const saveViewState = (view: ViewState): Promise<void> =>
  invoke<void>("save_view_state", { view });

/** Most recent first; folders that have gone are pruned on read. */
export const listRecentProjects = (): Promise<RecentProject[]> =>
  invoke<RecentProject[]>("list_recent_projects");

/** What a Save As actually moved. */
export interface CopyReport {
  files: number;
  bytes: number;
  /** Preview-cache entries deliberately left behind; they re-derive. */
  skipped_previews: number;
}

/**
 * Copy the open project to `dest` and continue in the copy — Save As in
 * the sense a DAW means it, with the original left as it was.
 */
export const saveProjectAs = (dest: string): Promise<CopyReport> =>
  invoke<CopyReport>("save_project_as", { dest });

export const forgetRecentProject = (path: string): Promise<RecentProject[]> =>
  invoke<RecentProject[]>("forget_recent_project", { path });

export const renameTrack = (track: number, name: string): Promise<string> =>
  invoke<string>("rename_track", { track, name });

export const removeTrack = (track: number): Promise<string> =>
  invoke<string>("remove_track", { track });

export const duplicateTrack = (track: number): Promise<string> =>
  invoke<string>("duplicate_track", { track });

/**
 * Replace a clip's volume automation curve. An empty array clears it.
 *
 * Points need not be sorted — the tool sorts, so dragging one past its
 * neighbour does not need the caller to reorder first.
 */
/**
 * Move one clip to a new start, in seconds from the top of the
 * timeline. The other clips stay put — `time_shift` is the whole-track
 * version.
 *
 * Clips are re-sorted by start afterwards, so a clip dragged past its
 * neighbour comes back at a different index. Re-read `listTracks`
 * rather than reusing the index.
 */
export const moveClip = (
  track: number,
  clip: number,
  startSec: number,
): Promise<string> =>
  invoke<string>("move_clip", { track, clip, startSec });

/** Remove one clip, leaving a silent gap where it was. */
export const removeClip = (track: number, clip: number): Promise<string> =>
  invoke<string>("remove_clip", { track, clip });

export const setClipEnvelope = (
  track: number,
  clip: number,
  points: EnvelopePoint[],
): Promise<string> =>
  invoke<string>("set_clip_envelope", { track, clip, points });

// -----------------------------------------------------------------------------
// DAG ops (M24): set head + rename node
// -----------------------------------------------------------------------------

/** Move the session head to `nodeId`. Returns the new head as hex. */
export const setHeadTo = (nodeId: string): Promise<string> =>
  invoke<string>("set_head_to", { nodeId });

/** Set (or clear, when `label` is empty) a human-readable label on a node. */
export const renameNode = (nodeId: string, label: string): Promise<void> =>
  invoke<void>("rename_node", { nodeId, label });

// -----------------------------------------------------------------------------
// Capabilities
// -----------------------------------------------------------------------------

export interface CapabilityDescriptor {
  /**
   * The identifier the backend matches a disabled entry against.
   *
   * Differs from `name` for MCP tools: `name` is the readable
   * `<server>::<tool>`, `id` is the dispatcher's `<server>__<tool>`.
   * Persisting the wrong one is why the menu's checkboxes did nothing.
   */
  id: string;
  name: string;
  description: string;
  category: string;
}

export interface Capabilities {
  tools: CapabilityDescriptor[];
  skills: CapabilityDescriptor[];
  agents: CapabilityDescriptor[];
  mcp_servers: CapabilityDescriptor[];
}

export const listCapabilities = (): Promise<Capabilities> =>
  invoke<Capabilities>("list_capabilities");

// -----------------------------------------------------------------------------
// Batch import (Task 7)
// -----------------------------------------------------------------------------

export interface BatchLoadResult {
  tracks_loaded: number;
  last_node_id: string | null;
}

export const batchLoad = (paths: string[]): Promise<BatchLoadResult> =>
  invoke<BatchLoadResult>("batch_load", { paths });

// -----------------------------------------------------------------------------
// Templates (Task 8)
// -----------------------------------------------------------------------------

export interface TemplateInfo {
  name: string;
  description: string;
}

export const listTemplates = (): Promise<TemplateInfo[]> =>
  invoke<TemplateInfo[]>("list_templates");

export const applyTemplate = (name: string): Promise<string> =>
  invoke<string>("apply_template", { name });

// -----------------------------------------------------------------------------
// Microphone recording (Task 5)
// -----------------------------------------------------------------------------

export interface RecordingResult {
  path: string;
  sample_rate: number;
  channels: number;
}

export const startRecording = (): Promise<string> =>
  invoke<string>("start_recording");

export const stopRecording = (outputPath: string): Promise<RecordingResult> =>
  invoke<RecordingResult>("stop_recording", { outputPath });

// -----------------------------------------------------------------------------
// Bundled skills install (Task 2)
// -----------------------------------------------------------------------------

/** Copy the 8 pre-installed skill .md files from the Tauri resource bundle
 *  into ~/.edytlab/skills/ on first launch.  Returns the number of files
 *  copied, or 0 if the skills directory already contains .md files (so user
 *  customisations are never overwritten) or if running in dev mode without
 *  the bundled-skills resource dir.  Always safe to call; non-fatal. */
export async function installBundledSkills(): Promise<number> {
  return invoke<number>("install_bundled_skills");
}

// -----------------------------------------------------------------------------
// Plugin install (Task 4)
// -----------------------------------------------------------------------------

export interface PluginInstallResult {
  name: string;
  version: string;
  skills_installed: number;
  agents_installed: number;
  /** Alias of `mcp_registered`, kept for older callers. */
  mcp_keys: string[];
  /**
   * Server ids written into `~/.edytlab/mcp.json`. Always registered
   * **disabled**: a server config is a command line, and enabled servers
   * are spawned at launch, so installing a plugin must never amount to
   * running what it names. The user enables each one deliberately, with
   * the command visible in Settings → MCP.
   */
  mcp_registered: string[];
  /** Ids skipped because the user already has a server by that name. */
  mcp_skipped: string[];
  summary: string;
}

/** Install a plugin from a GitHub repo or a local directory.
 *
 *  @param source  Either `"github:org/repo"` (downloads the main-branch zip
 *                 from GitHub) or `"local:/abs/path/to/plugin-dir"`.
 *  @returns       A summary object with the plugin name/version and counts of
 *                 skills and agent profiles installed, plus any MCP server keys
 *                 declared in the manifest. */
export async function installPlugin(source: string): Promise<PluginInstallResult> {
  return invoke<PluginInstallResult>("install_plugin", { source });
}
