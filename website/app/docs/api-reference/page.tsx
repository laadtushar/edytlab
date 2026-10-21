import type { Metadata } from "next";
import { siteConfig } from "@/lib/site";
import { pageSocial } from "@/lib/seo-core";
import { DocShell } from "@/components/docs/doc-shell";

const title = "API Reference: Tauri commands, types and events";
const description =
  "The API reference for edytlab: every Tauri command the frontend can call, the TypeScript bridge types and the events the backend emits, with examples.";

export const metadata: Metadata = {
  title,
  description,
  alternates: { canonical: "/docs/api-reference" },
  ...pageSocial("/docs/api-reference", `${title} · edytlab`, description),
};

const commands = [
  {
    group: "Project",
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
    group: "API Keys",
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
        desc: "Check whether a specific provider has a key. Rejects when the keychain could not be read, as hasApiKey does.",
      },
      {
        name: "getKeychainPersistence()",
        returns: "KeychainPersistence",
        desc: "Whether keys and settings saved now survive a reboot. persistent is false on Linux with no Secret Service running, where they are kept in the kernel keyring until restart; reason says why. The setters answer the same shape for the save they just made.",
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
        desc: "Probe a provider at the endpoint and model currently on screen. Reports three outcomes: ready, connected but the model cannot call tools (so editing will fail), or unreachable.",
      },
      {
        name: "testApiKey(key)",
        returns: "void",
        desc: "Validate a key with a 1-token probe request. Throws on failure.",
      },
    ],
  },
  {
    group: "Provider and Model",
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
        desc: "Switch the active provider. Rebuilds the agent. Resolves with where the choice was saved, as setApiKeyFor does.",
      },
      {
        name: "listModelsFor(provider, apiKey?)",
        returns: "ModelCatalogue",
        desc: "Live list; Anthropic falls back to a built-in list without a key or when the fetch fails. Cached 10 minutes.",
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
        desc: "The reasoning effort saved for the provider, or null when the model's own default applies. Only Anthropic has the setting; any other provider answers null. Rejects when the keychain could not be read, as getBaseUrlFor does.",
      },
      {
        name: "setEffortFor(provider, effort)",
        returns: "KeychainPersistence",
        desc: "Set how hard the model works on a response: low, medium, high, xhigh or max, sent as output_config.effort. null restores the model's default. Anthropic only; any other provider is refused. Rebuilds the agent, and resolves with where the level was saved, as setBaseUrlFor does.",
      },
    ],
  },
  {
    group: "Session Graph",
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
    group: "Rendering and A/B",
    items: [
      {
        name: "renderPreview(node)",
        returns: "string (path)",
        desc: "Render the session to a WAV for playback. Cached by node id under .audiograph/previews/, so repeating a head — undo, redo — costs nothing. The cache is capped; a mix bigger than the cap is an error.",
      },
      {
        name: "getRenderWarnings(node)",
        returns: "RenderWarning[]",
        desc: "What a render of the node leaves out: every switched-on effect in a stored chain (a track's, a bus's, the master's) that the renderer cannot run in a chain yet. Read from the stored chains, so nothing is rendered. Empty when nothing is left out; the window shows each one above the timeline.",
      },
      {
        name: "renderRange(node, startSec, endSec, outPath)",
        returns: "{ warnings }",
        desc: "Render a time range to a specific output file. warnings is what the file was written without (getRenderWarnings' list for the node), [] when nothing was left out.",
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
    group: "Agent",
    items: [
      {
        name: "sendMessage(text)",
        returns: "void",
        desc: "Send a user message. Agent runs async; emits events during processing.",
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
    group: "Selection and Markers",
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
    group: "Transcript",
    items: [
      {
        name: "getTranscript()",
        returns: "TranscriptWord[]",
        desc: "The transcript at the current head, as { text, start_sec, end_sec, confidence } words. Empty until transcribe has run, which it cannot in this build (its inference is not shipped), so the list is empty today.",
      },
      {
        name: "cutTranscriptWords(track, fromWord, toWord)",
        returns: "NodeId",
        desc: "Cut the half-open word range [fromWord, toWord) and the audio underneath it, closing the gap. Indices are into the array getTranscript() returned; re-read it afterwards, since the words after the cut renumber. Refuses a track out of range, no transcript, and an empty range or one past the end.",
      },
    ],
  },
  {
    group: "Tracks",
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
        desc: "Replace a clip's volume automation curve with points of { time_samples, gain_db }, the time relative to the clip's own start. An empty array clears it; the points need not be sorted.",
      },
    ],
  },
  {
    group: "Skills",
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
    group: "Agent Profiles",
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
  {
    group: "MCP Servers",
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
    group: "Plugins and Capabilities",
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
  {
    group: "ONNX Runtime",
    items: [
      {
        name: "mlRuntimeStatus()",
        returns: "MlRuntimeStatus",
        desc: "Which ONNX Runtime library the app found and its version, for the line in Settings. It loads the library if it has not been, off the main thread, and a library that is missing, broken, or unusable because this macOS is too old for it is a status, never a hang. state is found (version, path, and source: bundled, environment or executable), missing (the places searched), unsupportedOs (macOS older than the library's 13.4) or loadFailed (path, reason). The installers of every release after v0.6.1 carry ONNX Runtime 1.23.2, so an installed build answers found with source bundled; a build from source answers missing. A loaded library does not mean stem separation or transcription run.",
      },
    ],
  },
  {
    group: "Templates",
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
  {
    group: "Recording",
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
  {
    group: "Memory",
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
];

const events = [
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
];

export default function ApiReferencePage() {
  return (
    <DocShell
      title="API Reference"
      description="All Tauri commands, TypeScript bridge types, and events."
    >
      <blockquote>
        This page covers the public API consumed by the edytlab frontend. For
        the full Rust implementation, see{" "}
        <a
          href={`${siteConfig.github}/blob/main/apps/desktop/src-tauri/src/commands.rs`}
          target="_blank"
          rel="noopener noreferrer"
        >
          commands.rs
        </a>{" "}
        and{" "}
        <a
          href={`${siteConfig.github}/blob/main/apps/desktop/src/lib/tauri-bridge.ts`}
          target="_blank"
          rel="noopener noreferrer"
        >
          tauri-bridge.ts
        </a>
        . The complete reference is in{" "}
        <a
          href={`${siteConfig.github}/blob/main/docs/api-reference.md`}
          target="_blank"
          rel="noopener noreferrer"
        >
          docs/api-reference.md
        </a>
        .
      </blockquote>

      <h2>Commands</h2>
      <p>
        All commands are invoked via <code>tauri-bridge.ts</code>. They return{" "}
        <code>Promise&lt;T&gt;</code> and throw a string on Rust{" "}
        <code>Err(_)</code>.
      </p>

      {commands.map((group) => (
        <div key={group.group}>
          <h3>{group.group}</h3>
          <table>
            <thead>
              <tr>
                <th>Function</th>
                <th>Returns</th>
                <th>Description</th>
              </tr>
            </thead>
            <tbody>
              {group.items.map((cmd) => (
                <tr key={cmd.name}>
                  <td>
                    <code>{cmd.name}</code>
                  </td>
                  <td>
                    <code>{cmd.returns}</code>
                  </td>
                  <td>{cmd.desc}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ))}

      <h2>Events</h2>
      <p>
        Subscribe with the unlisten pattern. Each handler returns an{" "}
        <code>UnlistenFn</code> — call it in <code>useEffect</code> cleanup.
      </p>
      <pre>
        <code>{`const unlisten = await bridge.onTextDelta((chunk) => {
  appendText(chunk);
});
// cleanup:
return () => { unlisten(); };`}</code>
      </pre>
      <table>
        <thead>
          <tr>
            <th>Event</th>
            <th>Payload</th>
            <th>Description</th>
          </tr>
        </thead>
        <tbody>
          {events.map((ev) => (
            <tr key={ev.name}>
              <td>
                <code>{ev.name}</code>
              </td>
              <td>
                <code>{ev.payload}</code>
              </td>
              <td>{ev.desc}</td>
            </tr>
          ))}
        </tbody>
      </table>

      <h2>Key types</h2>
      <pre>
        <code>{`type NodeId = string;
type ProviderId = "anthropic" | "openrouter" | "openai" | "groq" | "gemini" | "ollama";

interface ProjectInfo  { path: string; head: NodeId | null }
interface SessionNode  { id: NodeId; parent: NodeId | null; label: string | null; state: SessionState }
interface GraphSummary { nodes: GraphNode[]; head: NodeId | null }
interface GraphNode    { id: NodeId; parent: NodeId | null; label: string | null; tool: string | null }
interface TrackSummary { id: string; name: string; muted: boolean; gain_db: number; audio_path: string | null }
interface RenderWarning { scope: { type: "track"; index: number; name: string } | { type: "bus"; name: string } | { type: "master" }; effect_index: number; kind: string; reason: "not_yet_streamable" | "unknown" }
interface ModelInfo    { id: string; display_name: string; context_length: number | null }
interface ModelCatalogue { models: ModelInfo[]; source: "live" | "fallback"; notice: string | null }
type MlRuntimeStatus =
  | { state: "found"; version: string; path: string; source: "bundled" | "environment" | "executable" }
  | { state: "missing"; searched: string[] }
  | { state: "unsupportedOs"; needs: string; running: string }
  | { state: "loadFailed"; path: string | null; reason: string };
interface Marker       { id: string; name: string; kind: "marker" | "region"; time_sec?: number; start_sec?: number; end_sec?: number }`}</code>
      </pre>
    </DocShell>
  );
}
