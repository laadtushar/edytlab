import type { ReferenceArea } from "./reference";

/**
 * The Tauri commands, events and bridge types, as the API Reference lists
 * them.
 *
 * This is the one place a command is documented. `/docs/api-reference`
 * lists the areas and every name; `/docs/api-reference/[area]` renders one
 * area from `apiAreas` (`generateStaticParams` reads it), so a new command
 * is one row here and a new area is one more object: no page file to add.
 *
 * `lib/apiReferencePage.test.ts` holds the commands to the
 * `generate_handler!` list in `lib.rs`, one row each. A command row is
 * `name: "setTrackGain(track, db)"` (or, when the arguments contain a
 * double quote, `name: 'readMemory("global" | "project")'`).
 */
export interface ApiCommand {
  /** The bridge function with its arguments: `setTrackGain(track, db)`. */
  name: string;
  returns: string;
  desc: string;
}

export interface ApiEvent {
  /** The bridge subscriber with its callback: `onTextDelta(cb)`. */
  name: string;
  payload: string;
  desc: string;
}

export type ApiSection =
  | { kind: "commands"; title: string; items: ApiCommand[] }
  | { kind: "events"; title: string; items: ApiEvent[] }
  | { kind: "types"; title: string; code: string };

export type ApiArea = ReferenceArea<ApiSection>;

const events: ApiEvent[] = [
  {
    name: "onTextDelta(cb)",
    payload: "string (text chunk)",
    desc: "Each streamed text chunk from the LLM. Append to the current assistant message.",
  },
  {
    name: "onToolCall(cb)",
    payload: "{ name, id }",
    desc: "Tool execution started. Use to show a tool badge in the chat UI.",
  },
  {
    name: "onToolCallEnd(cb)",
    payload: "{ id, ok: boolean, not_run: boolean, view? }",
    desc: "Tool execution completed. ok = false if the tool returned an error. not_run = true (with ok = false) when the call was announced but never dispatched (declined, reworded, unanswered, or over the tool budget): show it as not run, not as a failure. view is set for the few tools that return something to draw or play: plot_spectrum's chart, and audition_effect's excerpt with a target caption (track 1, the master bus, or bus \"Drums\").",
  },
  {
    name: "onNodeCreated(cb)",
    payload: "nodeId: string",
    desc: "An agent tool call moved the session's head: to a new node, or to one already in the graph (revert_to, fork_node). nodeId is the head the session is on after the call, not a node_id from the tool's result, and it is sent once per call that moved it, even one that then failed. A call that leaves the head where it was (name_node, a read) sends nothing. Refresh the graph view and track list.",
  },
  {
    name: "onAgentDone(cb)",
    payload: "none",
    desc: "Agent turn fully complete — no more text or tool calls.",
  },
  {
    name: "onPlan(cb)",
    payload: "steps: { step, tool, description }[]",
    desc: "Something needs approval before it runs: the agent's plan, or with Plan first on and no plan, its first edit shown as the exact tool calls and arguments. The turn waits for approvePlan() or rejectPlan().",
  },
  {
    name: "onPlanUnavailable(cb)",
    payload: "reason: string, firstEditHeld: boolean",
    desc: "A plan was asked for and none arrived. firstEditHeld says whether the first edit will be held for approval (Plan first on) or the turn proceeds with no gate.",
  },
  {
    name: "onPlanRejected(cb)",
    payload: "none",
    desc: "The user declined a plan or a held edit. The turn ends with no onAgentDone.",
  },
  {
    name: "onMarkerChanged(cb)",
    payload: "none",
    desc: "Marker or region annotation added, renamed, moved or removed. Refresh the marker list.",
  },
  {
    name: "onChatGptStatus(cb)",
    payload: "status: ChatGptStatus",
    desc: "The ChatGPT sign-in changed (event chatgpt://status), sent when the status differs from the last one sent: a sign-in begins, ends or is cancelled, a sign-out, a session that ended inside a turn or a refresh, a message kept or taken. Lets a card follow a sign-in that is happening in the browser. The payload is the object chatgptStatus answers with.",
  },
  {
    name: "onMlModelProgress(cb)",
    payload: "status: MlModelStatus",
    desc: "A downloadable model changed state (event ml-model-progress): the start, progress at most every 100 ms, the end (ready, failed, or not_downloaded after a cancel) and a removal. The payload is the object the model commands answer with.",
  },
];

const keyTypes = `type NodeId = string;
type ProviderId = "anthropic" | "openrouter" | "openai" | "groq" | "gemini" | "ollama";

interface ProjectInfo  { path: string; head: NodeId | null }
interface SessionNode  { id: NodeId; parent: NodeId | null; created_at: string; label: string | null; reasoning: string | null; state: SessionState }
interface GraphSummary { nodes: GraphNode[]; head: NodeId | null }
interface GraphNode    { id: NodeId; parent: NodeId | null; label: string | null; tool: string | null; created_at: string }
interface TrackSummary { id: string; name: string; muted: boolean; gain_db: number; pan: number; soloed: boolean; audio_path: string | null; clips: ClipSummary[] }
interface ClipSummary  { start_sec: number; length_sec: number; source_path: string; volume_envelope: { time_sec: number; gain_db: number }[] }
interface RenderWarning { scope: { type: "track"; index: number; name: string } | { type: "bus"; name: string } | { type: "master" }; effect_index: number; kind: string; reason: "not_yet_streamable" | "unknown" }
interface ModelInfo    { id: string; display_name: string; context_length: number | null; provider_hint: string | null }
interface ModelCatalogue { models: ModelInfo[]; source: "live" | "fallback"; notice: string | null }
type MlRuntimeStatus =
  | { state: "found"; version: string; path: string; source: "bundled" | "environment" | "executable" }
  | { state: "missing"; searched: string[] }
  | { state: "unsupportedOs"; needs: string; running: string }
  | { state: "loadFailed"; path: string | null; reason: string };
type Marker           = { id: string; name: string; kind: "marker"; time_sec: number }
                      | { id: string; name: string; kind: "region"; start_sec: number; end_sec: number }`;

export const apiAreas: ApiArea[] = [
  {
    slug: "project",
    title: "Projects and Templates",
    summary: "Open, save and rename projects, keep the view and the history path, and apply templates.",
    sections: [
      {
        kind: "commands",
        title: "Project",
        items: [
          {
            name: "openProject(path)",
            returns: "ProjectInfo",
            desc: "Open or create a project directory. Returns the current head node ID (null if empty), stamps last-opened into project.json, and moves the project to the top of the recents list. Refused while a batch_apply run is writing its per-file project to the folder, and while the folder has been replaced at the same path (restored from a backup, or trashed and made again) and the assistant is still working in the old one; wait for it to finish and open the project again.",
          },
          {
            name: "saveProjectAs(dest)",
            returns: "CopyReport",
            desc: "Copy the open project to dest and carry on in the copy. The copy is the project at the moment of the click, history and audio together, whatever the assistant commits while it is made, and the audio is copied without holding up other commands. Cached previews and auditions are left behind, since they rebuild. Refuses a dest that exists and is not empty; a copy that fails part way, or is missing what its head needs, is an error and leaves dest for inspection. Resolves with { files, bytes, skipped_cache_files }.",
          },
          {
            name: "getProjectMeta()",
            returns: "ProjectMeta",
            desc: "The open project's name, when it was created, and its notes. A missing or corrupt project.json returns a folder-named default rather than failing — the audio and its history are not in that file.",
          },
          {
            name: "setProjectMeta(name, notes?)",
            returns: "ProjectMeta",
            desc: "Rename the open project. An empty name is refused, and the recents row is updated in step.",
          },
          {
            name: "getViewState()",
            returns: "ViewState",
            desc: "Where you were: head, zoom, selection, playhead. Every field is independently optional — absent means 'was not recorded', not 'reset this'.",
          },
          {
            name: "saveViewState(view)",
            returns: "void",
            desc: "Remember where you are. Disposable by design: losing it costs a scroll, not work.",
          },
          {
            name: "getHeadTrail()",
            returns: "SavedTrail",
            desc: "The path you took through the history, saved with the project so undo retraces it after a restart. A missing or corrupt file answers an empty path rather than failing. Entries for nodes the project no longer has are dropped.",
          },
          {
            name: "saveHeadTrail(trail)",
            returns: "void",
            desc: "Keep the path with the project, replacing the last. Written atomically and bounded to the last 500 steps each way. A path that does not end at a node of the open project is ignored.",
          },
          {
            name: "listRecentProjects()",
            returns: "RecentProject[]",
            desc: "Projects this machine has opened, most recent first, one row each, capped at 10. Folders that have gone are pruned on read and the pruning is written back.",
          },
          {
            name: "forgetRecentProject(path)",
            returns: "RecentProject[]",
            desc: "Remove the row. Does not touch the project.",
          },
        ],
      },
      {
        kind: "commands",
        title: "Templates",
        items: [
          {
            name: "listTemplates()",
            returns: "TemplateInfo[]",
            desc: "Session templates that ship with the app.",
          },
          {
            name: "applyTemplate(name)",
            returns: "NodeId",
            desc: "Apply a template to the current session.",
          },
        ],
      },
    ],
  },
  {
    slug: "session",
    title: "Session, Preview and Markers",
    summary: "Read the session graph, move the head, render previews and A/B compares, set the selection and markers, and cut by transcript.",
    sections: [
      {
        kind: "commands",
        title: "Session Graph",
        items: [
          {
            name: "getSessionHead()",
            returns: "NodeId",
            desc: "Returns the current head node ID.",
          },
          {
            name: "getNode(id)",
            returns: "SessionNode",
            desc: "Fetch a full node (including SessionState) by ID.",
          },
          {
            name: "getGraph()",
            returns: "GraphSummary",
            desc: "Fetch all nodes (without full state) and the current head.",
          },
          {
            name: "setHeadTo(nodeId)",
            returns: "NodeId",
            desc: "Move the head pointer to any existing node (non-destructive revert).",
          },
          {
            name: "renameNode(nodeId, label)",
            returns: "void",
            desc: "Set a human-readable label on a node.",
          },
        ],
      },
      {
        kind: "commands",
        title: "Rendering and A/B",
        items: [
          {
            name: "renderPreview(node)",
            returns: "string (path)",
            desc: "Render the session to a WAV for playback. Cached by node id under .audiograph/previews/, so repeating a head — undo, redo — costs nothing. The cache is capped; a mix bigger than the cap is an error. The file it returns is the mix the window plays, so it is held out of eviction, and outside the cap, until the next renderPreview replaces it or the project changes: a render made meanwhile (the agent's, an audition, a graph preview) cannot evict it, and the timeline can read it again when it remounts. If the file is gone anyway the window renders the node again, once.",
          },
          {
            name: "getRenderWarnings(node)",
            returns: "RenderWarning[]",
            desc: "What a render of the node leaves out: every switched-on effect in a stored chain (a track's, a bus's, the master's) that the renderer cannot run in a chain yet. Read from the stored chains, so nothing is rendered. Empty when nothing is left out; the window shows each one above the timeline.",
          },
          {
            name: "renderRange(node, startSec, endSec, outPath)",
            returns: "{ warnings }",
            desc: "Render a time range to a specific output file. warnings is what the file was written without (getRenderWarnings' list for the node), [] when nothing was left out. Rejects, writing nothing, when outPath is audio the project reads (a recording it plays from, in any step of its history): choose another path.",
          },
          {
            name: "prepareCompare(a, b)",
            returns: "{ a_path, b_path, a_warnings, b_warnings, compare_id }",
            desc: "Pre-render two nodes for A/B comparison, with what each side's render leaves out. Each side is the preview cache's entry for its node (the file renderPreview returns for it), rendered only if it is not there already, and nothing is written outside the project. Both are kept from the cache's eviction, and outside its cap, until endCompare(compare_id) or the next compare, so two sides too long to fit in the cache together can still be compared. A side bigger than the cap on its own is an error.",
          },
          {
            name: "endCompare(compareId)",
            returns: "void",
            desc: "The compare bar has closed: its two renders are ordinary cache entries again, and the cache is evicted back under its cap at once. Ending a compare that a newer one has replaced changes nothing.",
          },
          {
            name: "acceptB(b)",
            returns: "NodeId",
            desc: "Accept node B as the new session head.",
          },
        ],
      },
      {
        kind: "commands",
        title: "Selection and Markers",
        items: [
          {
            name: "setSelectionContext(range?)",
            returns: "void",
            desc: "Set or clear the selection range. Injected as agent context on next turn.",
          },
          {
            name: "addMarker(timeSec, name)",
            returns: "NodeId",
            desc: "Add a named point marker. Appends one node and returns the new head; the label's own ID is in listMarkers().",
          },
          {
            name: "updateMarker(id, patch)",
            returns: "NodeId",
            desc: "Rename and/or move a label in one call. patch is { name?, time?, start?, end? }: time moves a point marker there, and a region starts there and keeps its length; start and end set a region's bounds outright and are sent together, never with time. A change that changes nothing appends no node and returns the head unchanged. Refuses a time below 0 or not finite, a start without an end, and end <= start.",
          },
          {
            name: "removeMarker(id)",
            returns: "NodeId",
            desc: "Remove a marker by annotation ID. Returns the new head.",
          },
          {
            name: "listMarkers()",
            returns: "Marker[]",
            desc: "List all markers and regions for the current session.",
          },
        ],
      },
      {
        kind: "commands",
        title: "Transcript",
        items: [
          {
            name: "getTranscript()",
            returns: "Transcript",
            desc: "The transcript at the current head, and where it came from: { words, source }, with words as { text, start_sec, end_sec, confidence } and source one of { kind: \"track\", track }, { kind: \"file\" }, { kind: \"gone\" } or { kind: \"unknown\" }. No words until transcribe has run (the Transcript tab re-reads it on every head move, so a transcription the assistant makes appears as its node arrives). A word's time is seconds of the audio it was made from, so the Transcript tab cuts a track by it only if the transcript is of that track (source.kind is \"track\").",
          },
          {
            name: "cutTranscriptWords(track, fromWord, toWord)",
            returns: "NodeId",
            desc: "Cut the half-open word range [fromWord, toWord) and the audio underneath it, closing the gap. Indices are into the array getTranscript() returned; re-read it afterwards, since the words after the cut renumber. track is the transcript's own track (getTranscript().source.track): the cut_words tool the command runs refuses any other track. Refuses a track out of range, no transcript, a track that is not the one the transcript is of (or that has been removed, or a transcript of a file), and an empty range or one past the end.",
          },
        ],
      },
    ],
  },
  {
    slug: "tracks",
    title: "Tracks and Recording",
    summary: "List and edit tracks and clips, sync-lock and envelopes, and record from the microphone.",
    sections: [
      {
        kind: "commands",
        title: "Tracks",
        items: [
          {
            name: "batchLoad(paths)",
            returns: "BatchLoadResult",
            desc: "Load several audio files in one call, each as its own track. Each file is its own node, so undo takes back one at a time. Resolves with { tracks_loaded, last_node_id, failures }: a file the load tool refuses lands in failures with its reason, leaves no node behind, and does not stop the rest.",
          },
          {
            name: "listTracks()",
            returns: "TrackSummary[]",
            desc: "List all tracks in the current session head. A project with no history yet rejects with NoSession, which is not a fault: the app (isNoSession) reads it as an empty list.",
          },
          {
            name: "renameTrack(track, name)",
            returns: "NodeId",
            desc: "Rename a track. Returns the new head.",
          },
          {
            name: "removeTrack(track)",
            returns: "NodeId",
            desc: "Delete a track and everything on it. An ordinary session node, so it undoes like any other edit, which is why the window does not confirm first.",
          },
          {
            name: "duplicateTrack(track)",
            returns: "NodeId",
            desc: "Copy a track, with its clips, gain, pan and effects, as a new track at the end.",
          },
          {
            name: "setTrackGain(track, gainDb)",
            returns: "NodeId",
            desc: "Set a track's gain in dB. Absolute, not relative.",
          },
          {
            name: "setTrackPan(track, pan)",
            returns: "NodeId",
            desc: "Set a track's pan: -1 hard left, 0 centre, 1 hard right.",
          },
          {
            name: "setTrackMuted(track, muted)",
            returns: "NodeId",
            desc: "Mute or unmute a track. Returns the new head.",
          },
          {
            name: "setTrackSoloed(track, soloed)",
            returns: "NodeId",
            desc: "Solo or unsolo a track. Soloing silences the others at render time without changing their muted flags, so un-soloing puts everything back as it was.",
          },
          {
            name: "getSyncLock()",
            returns: "boolean",
            desc: "Whether an edit that shifts time on one track shifts them all. It belongs to the session, not to a track, so the toggle can show the right state the moment a project opens.",
          },
          {
            name: "setSyncLock(enabled)",
            returns: "NodeId",
            desc: "Turn sync lock on or off. Resolves to the new head, or to the unchanged one when the value did not change.",
          },
          {
            name: "moveClip(track, clip, startSec)",
            returns: "NodeId",
            desc: "Move one clip to a new start, in seconds from the top of the timeline; the other clips stay put. Clips are re-sorted by start afterwards, so a clip dragged past its neighbour comes back at a different index: re-read listTracks() before addressing it again.",
          },
          {
            name: "removeClip(track, clip)",
            returns: "NodeId",
            desc: "Remove one clip, leaving a silent gap where it was. The other clips do not move.",
          },
          {
            name: "setClipEnvelope(track, clip, points)",
            returns: "NodeId",
            desc: "Replace a clip's volume automation curve with points of { time_sec, gain_db }, the time in seconds from the clip's own start. An empty array clears it; the points need not be sorted.",
          },
        ],
      },
      {
        kind: "commands",
        title: "Recording",
        items: [
          {
            name: "startRecording()",
            returns: "string",
            desc: "Begin capturing from the default input device.",
          },
          {
            name: "stopRecording(outputPath)",
            returns: "RecordingResult",
            desc: "Stop capture and write the take to a WAV file at outputPath. Resolves with { path, sample_rate, channels }. The take is not added to the session: load it with batchLoad([path]). A failed write means the take is gone; a failed import means it is on disk at the path this returned.",
          },
          {
            name: "timerRecord(outputPath, schedule)",
            returns: "{ path?, cancelled }",
            desc: "Record on a timer: wait startAfterSec, then capture for durationSec. Both are optional; omitting startAfterSec starts now, and omitting durationSec records until stopped. cancelled is true when cancelLongRunningTool() ended it early, and path may then be absent. Emits tool-progress events throughout.",
          },
        ],
      },
    ],
  },
  {
    slug: "keys-and-providers",
    title: "API Keys and Providers",
    summary: "Store and test provider keys in the OS keychain, and choose the provider, model, address and effort.",
    sections: [
      {
        kind: "commands",
        title: "API Keys",
        items: [
          {
            name: "setApiKey(key)",
            returns: "KeychainPersistence",
            desc: "Store the API key for the active provider in the OS keychain. Resolves with where it landed, as setApiKeyFor does.",
          },
          {
            name: "setApiKeyFor(provider, key)",
            returns: "KeychainPersistence",
            desc: "Store the API key for a specific provider and make it the active one. Resolves with where the save landed: persistent is false when it fell back to the kernel keyring because no Secret Service could be reached, so it lasts only until restart. The key is saved and used either way. The other setters that write to the keychain answer the same way.",
          },
          {
            name: "hasApiKey()",
            returns: "boolean",
            desc: "Check whether the active provider has a key configured. Rejects when the keychain could not be read (a locked keyring, say), which is not the same as false: a key may be stored and could not be looked up.",
          },
          {
            name: "hasApiKeyFor(provider)",
            returns: "boolean",
            desc: "Check whether a specific provider has a key. For chatgpt there is no key: it is true while the ChatGPT sign-in is signed in. Rejects when the keychain could not be read, as hasApiKey does.",
          },
          {
            name: "getKeychainPersistence()",
            returns: "KeychainPersistence",
            desc: "Whether a key saved now survives a reboot (settings are in a file and always do). persistent is false on Linux with no Secret Service running, where keys are kept in the kernel keyring until restart; reason says why. The setters answer the same shape for the save they just made.",
          },
          {
            name: "clearApiKeyFor(provider)",
            returns: "void",
            desc: "Remove a specific provider's key from the keychain.",
          },
          {
            name: "clearApiKey()",
            returns: "void",
            desc: "Remove the API key for the active provider from the keychain.",
          },
          {
            name: "testApiKeyFor(provider, key, baseUrl?, model?)",
            returns: "ProbeReport",
            desc: "Probe a provider at the endpoint and model currently on screen. Reports three outcomes: ready, connected but the model cannot call tools (so editing will fail), or unreachable. For chatgpt the key and address are ignored: it is tested with the signed-in session, on a provider of its own so a turn that is running is not affected, and rejects when not signed in.",
          },
          {
            name: "testApiKey(key)",
            returns: "void",
            desc: "Validate a key with a 1-token probe request. Throws on failure.",
          },
        ],
      },
      {
        kind: "commands",
        title: "Provider and Model",
        items: [
          {
            name: "listProviders()",
            returns: 'ProviderId[]',
            desc: 'Returns ["anthropic", "openrouter", "openai", "groq", "gemini", "ollama"].',
          },
          {
            name: "getActiveProvider()",
            returns: "ProviderId",
            desc: "Returns the currently active provider ID.",
          },
          {
            name: "setActiveProvider(provider)",
            returns: "KeychainPersistence",
            desc: "Switch the active provider. Rebuilds the agent. Resolves with where the choice was saved, as setApiKeyFor does. chatgpt can be made active only while the ChatGPT sign-in is signed in.",
          },
          {
            name: "listModelsFor(provider, apiKey?)",
            returns: "ModelCatalogue",
            desc: "Live list; Anthropic falls back to a built-in list without a key or when the fetch fails. Cached 10 minutes. Each model carries reasoning_levels, which only chatgpt fills in: the levels the signed-in account lists for it. For chatgpt it is the account's own list, read with its session, and rejects when not signed in.",
          },
          {
            name: "getActiveModel(provider)",
            returns: "string",
            desc: "Returns the model ID selected for the provider.",
          },
          {
            name: "setActiveModel(provider, model)",
            returns: "KeychainPersistence",
            desc: "Set the active model for a provider. Resolves with where the choice was saved, as setApiKeyFor does.",
          },
          {
            name: "getBaseUrlFor(provider)",
            returns: "string | null",
            desc: "The base URL the provider has been pointed at, or null when it is on its default. Rejects when the keychain could not be read, which is not the same as null, and rejects without reading after a startup the keychain refused.",
          },
          {
            name: "defaultBaseUrlFor(provider)",
            returns: "string",
            desc: "The URL the provider ships with. Shown as the field's placeholder.",
          },
          {
            name: "setBaseUrlFor(provider, baseUrl)",
            returns: "KeychainPersistence",
            desc: "Point a provider at a proxy, a gateway or a local server. An empty string restores the default. Resolves with where the URL was saved, as setApiKeyFor does; clearing writes nothing and is always persistent.",
          },
          {
            name: "getEffortFor(provider)",
            returns: "ReasoningEffort | null",
            desc: "The reasoning effort saved for the provider, or null when the model's own default applies. Only Anthropic and the ChatGPT plan have the setting; any other provider answers null. Rejects when the keychain could not be read, as getBaseUrlFor does.",
          },
          {
            name: "setEffortFor(provider, effort)",
            returns: "KeychainPersistence",
            desc: "Set how hard the model works on a response: low, medium, high, xhigh or max, sent as output_config.effort. null restores the model's default. Anthropic and the ChatGPT plan only; any other provider is refused. For chatgpt the level is sent as reasoning.effort, and only when the model's list names it. Rebuilds the agent, and resolves with where the level was saved, as setBaseUrlFor does.",
          },
        ],
      },
    ],
  },
  {
    slug: "chatgpt-plan",
    title: "ChatGPT Plan",
    summary: "Sign in with ChatGPT, read the sign-in status, sign out, and open the usage page.",
    sections: [
      {
        kind: "commands",
        title: "ChatGPT Plan",
        items: [
          {
            name: "chatgptStatus()",
            returns: "ChatGptStatus",
            desc: "Where Sign in with ChatGPT (preview) stands: state is signed_out, signing_in, signed_in, plan_disabled, session_ended (reason.code says why), unread (a saved sign-in the keychain was not asked for) or unavailable; plus the account's email when there is one, where the refresh token is kept (tokenStorage: the keychain, the Linux kernel keyring until reboot, or memory until quit), and whether a message is kept to send again. Holds no token. Reads nothing from the keychain and makes no request.",
          },
          {
            name: "chatgptSignIn(intent)",
            returns: "ChatGptStatus",
            desc: "Sign in through the system browser, which the backend opens: the window never sees the URL. intent is sign_in, enable_plan_usage (ask for consent again after declining it) or another_account. Resolves when the sign-in is over, which can be minutes; a cancel resolves with the status as it then stands, and a failure rejects with a sentence. Requests go to auth.openai.com, and later to api.openai.com. No token is returned.",
          },
          {
            name: "chatgptCancelSignIn()",
            returns: "void",
            desc: "End the sign-in in progress, if any. chatgptSignIn then resolves with the status as it stands.",
          },
          {
            name: "chatgptSignOut()",
            returns: "ChatGptEnd",
            desc: "Sign out: the refresh token is revoked at OpenAI and removed from the keychain. The registered client is kept, so signing in again adds no new entry in ChatGPT's settings. notice says what could not be confirmed, if anything. An agent running on the plan is dropped.",
          },
          {
            name: "chatgptForgetRegistration()",
            returns: "ChatGptEnd",
            desc: "Start over: sign out, then forget the registered client and its account, so the next sign-in registers edytlab again. For a registration OpenAI no longer accepts. The old entry may remain in ChatGPT's settings, which notice says. Rejects while a sign-in is in progress.",
          },
          {
            name: "chatgptTakePendingMessage()",
            returns: "PendingChatGptMessage | null",
            desc: "The message a turn could not send because the ChatGPT session ended, taken and forgotten. After signing in, send resumeText with disabledTools. Kept only when the session ended (not on a sign-out) and the account has not changed since the turn began. In memory only: it is gone at quit, at the next message, at a sign-out or start over and at this call. null when none is kept.",
          },
          {
            name: "openChatgptUsage()",
            returns: "void",
            desc: "Open ChatGPT's usage page (chatgpt.com/settings/usage) in the system browser, for the Manage usage buttons. The window has no opener, so the backend opens it, and it takes no argument: the address is fixed and the webview cannot ask for another page. Rejects if no browser could be opened.",
          },
        ],
      },
    ],
  },
  {
    slug: "agent",
    title: "Agent and Profiles",
    summary: "Send messages, approve or reject plans, cancel a long-running tool, and manage agent profiles.",
    sections: [
      {
        kind: "commands",
        title: "Agent",
        items: [
          {
            name: "sendMessage(text)",
            returns: "void",
            desc: "Send a user message. Agent runs async; emits events during processing. A failure on the ChatGPT plan rejects with text that starts [plan-usage code=usage_limit] (or not_eligible, plan_disabled, session_ended, unavailable, unsupported_request, refused), then the sentence to show; when the session ended the message is kept for chatgptTakePendingMessage.",
          },
          {
            name: "approvePlan(steps?)",
            returns: "void",
            desc: "Approve what onPlan showed (the agent's plan, or its first edit when Plan first is on and it wrote no plan). Pass the edited steps, each with its tool, to change it instead: a plan follows the revision, a held edit is not run and the agent proposes again.",
          },
          {
            name: "rejectPlan()",
            returns: "void",
            desc: "Decline what onPlan showed. The turn ends and nothing is applied; for a held edit, the agent is told you declined.",
          },
          {
            name: "setPlanFirst(enabled)",
            returns: "void",
            desc: "With Plan first on, no edit runs without your approval: the plan is shown, or, with no plan, the first step that would change the session. Calls that only read the session, audition an effect or render a preview are never held. The setting is kept through the agent rebuilds a key, model or base-URL change causes, and across a restart: it is saved to ~/.edytlab/settings.json and read back when the app starts. If it cannot be saved the call rejects with \"could not save the Plan first setting\", the choice still applies until the app quits, and the next launch starts from what was saved before.",
          },
          {
            name: "getPlanFirst()",
            returns: "boolean",
            desc: "Whether Plan first is on: what was saved when the app last quit, then whatever setPlanFirst has set since. Off on a fresh install, or when the settings file is missing or corrupt. The chat box reads it when it opens, so its toggle shows the right state.",
          },
          {
            name: "cancelLongRunningTool()",
            returns: "void",
            desc: "Ask the running tool to stop. Cooperative: batch_apply checks between files and timer_record between polls, so it lands at the next boundary, and a tool that does not check is unaffected. Resolves when the request is recorded, not when the tool stops; watch the tool-progress event for that.",
          },
        ],
      },
      {
        kind: "commands",
        title: "Agent Profiles",
        items: [
          {
            name: "listAgentProfiles()",
            returns: "AgentProfileSummary[]",
            desc: "List saved agent profiles, re-reading them from disk first.",
          },
          {
            name: "readAgentProfile(name)",
            returns: "AgentProfileContent",
            desc: "Read one profile's model override, tool whitelist and system-prompt addition.",
          },
          {
            name: "upsertAgentProfile(name, content)",
            returns: "void",
            desc: "Create or overwrite an agent profile.",
          },
          {
            name: "deleteAgentProfile(name)",
            returns: "void",
            desc: "Delete an agent profile.",
          },
          {
            name: "getActiveAgentProfile()",
            returns: "string | null",
            desc: "The profile currently in effect, or null when none is selected.",
          },
          {
            name: "setActiveAgentProfile(name)",
            returns: "void",
            desc: "Switch the active profile. Pass null to clear it.",
          },
        ],
      },
    ],
  },
  {
    slug: "skills-and-extensions",
    title: "Skills, Memory and Extensions",
    summary: "Skills, memory, MCP servers, plugins and capabilities.",
    sections: [
      {
        kind: "commands",
        title: "Skills",
        items: [
          {
            name: "listSkills()",
            returns: "SkillSummary[]",
            desc: "List every skill in the library, re-reading the skills directory first so files added or removed since startup are picked up.",
          },
          {
            name: "readSkill(name)",
            returns: "SkillContent",
            desc: "Read one skill's front matter and markdown body.",
          },
          {
            name: "upsertSkill(name, content)",
            returns: "void",
            desc: "Create or overwrite a skill. The name is validated before it becomes a filename.",
          },
          {
            name: "deleteSkill(name)",
            returns: "void",
            desc: "Delete a skill from the library.",
          },
          {
            name: "installBundledSkills()",
            returns: "number",
            desc: "Keep the skills that ship with the app current in the user's library: copy in missing ones, update those the user has not edited (the new text of an edited one goes beside it as <name>.bundled.md), and never bring back one the user deleted. Returns how many were written.",
          },
        ],
      },
      {
        kind: "commands",
        title: "Memory",
        items: [
          {
            name: 'readMemory("global" | "project")',
            returns: "string",
            desc: "Read the memory markdown for the given scope. Returns empty string if none.",
          },
          {
            name: 'writeMemory("global" | "project", contents)',
            returns: "void",
            desc: "Overwrite the memory file atomically.",
          },
        ],
      },
      {
        kind: "commands",
        title: "MCP Servers",
        items: [
          {
            name: "listMcpServers()",
            returns: "McpServerListEntry[]",
            desc: "Every registered MCP server with its transport, enabled flag and current connection state.",
          },
          {
            name: "readMcpServer(id)",
            returns: "McpServerEntry",
            desc: "Full configuration for one server — command, args, env, headers.",
          },
          {
            name: "upsertMcpServer(id, entry)",
            returns: "void",
            desc: "Register or update a server. A server installed by a plugin always arrives disabled.",
          },
          {
            name: "deleteMcpServer(id)",
            returns: "void",
            desc: "Stop the server and remove it from the configuration.",
          },
          {
            name: "restartMcpServer(id)",
            returns: "void",
            desc: "Stop and re-launch one server, picking up an edited configuration.",
          },
        ],
      },
      {
        kind: "commands",
        title: "Plugins and Capabilities",
        items: [
          {
            name: "installPlugin(source)",
            returns: "PluginInstallResult",
            desc: "Install skills and agent profiles from github:org/repo or a local path. Existing files are never overwritten, and any MCP servers the plugin declares are registered disabled.",
          },
          {
            name: "listCapabilities()",
            returns: "Capabilities",
            desc: "A snapshot of everything the agent can currently reach: tools, skills, MCP servers and profiles, with their enabled state.",
          },
        ],
      },
    ],
  },
  {
    slug: "speech-model",
    title: "Speech Model and Runtime",
    summary: "Check the ONNX Runtime, and download, cancel or remove the speech model.",
    sections: [
      {
        kind: "commands",
        title: "ONNX Runtime",
        items: [
          {
            name: "mlRuntimeStatus()",
            returns: "MlRuntimeStatus",
            desc: "Which ONNX Runtime library the app found and its version, for the line in Settings. It loads the library if it has not been, off the main thread, and a library that is missing, broken, or unusable because this macOS is too old for it is a status, never a hang. state is found (version, path, and source: bundled, environment or executable), missing (the places searched), unsupportedOs (macOS older than the library's 13.4) or loadFailed (path, reason). The installers carry ONNX Runtime 1.23.2, so an installed build answers found with source bundled; a build from source answers missing. A loaded library runs nothing by itself: no model ships with it, transcribe uses the speech model only once it is downloaded (see Speech Model), and stem separation has not shipped. The same answer is printed, as one line of JSON with no window, by edytlab-desktop --ml-runtime-status, which the release workflows run against each installer.",
          },
        ],
      },
      {
        kind: "commands",
        title: "Speech Model",
        items: [
          {
            name: "mlModelStatus(model)",
            returns: "MlModelStatus",
            desc: "Where a downloadable model is, for the Speech model row in Settings. model is an id from crates/ml-pipeline/models.json (today whisper-base, about 139 MB from huggingface.co). The answer is a flat object tagged by state: not_downloaded, downloading (received and total bytes), ready (every file on disk and checked against its SHA-256) or failed (reason), with bytes on disk and the pinned size. It reads the disk off the main thread and makes no network request.",
          },
          {
            name: "downloadMlModel(model)",
            returns: "MlModelStatus",
            desc: "Start downloading a model on a thread of its own and answer at once with downloading. The only way a model is downloaded, called by the user's click on Download: nothing downloads at launch, when Settings opens, or from a tool or the agent. Progress and the end arrive on onMlModelProgress. A partial file from an earlier attempt is resumed with a Range request, and a file is renamed to its final name only once its size and SHA-256 match. It takes no store, engine, dispatcher or agent lock. The request goes to huggingface.co and carries no audio. A model on disk is what the transcribe tool uses: it finds the files, never downloads, and says the model is not downloaded when they are not there.",
          },
          {
            name: "cancelMlModelDownload(model)",
            returns: "MlModelStatus",
            desc: "Ask a running download to stop. It stops at its next chunk and keeps the partial file for a resume; onMlModelProgress says not_downloaded once it has. Does nothing when no download is running.",
          },
          {
            name: "removeMlModel(model)",
            returns: "MlModelStatus",
            desc: "Delete the model's own files (and its partial files) from the models folder, and nothing else in it. Refused while the model is downloading: cancel it first.",
          },
        ],
      },
    ],
  },
  {
    slug: "events-and-types",
    title: "Events and Types",
    summary: "The events the backend emits and the TypeScript types the bridge uses.",
    sections: [
      { kind: "events", title: "Events", items: events },
      { kind: "types", title: "Key types", code: keyTypes },
    ],
  },
];
