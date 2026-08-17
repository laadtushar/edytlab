import type { Metadata } from "next";
import { siteConfig } from "@/lib/site";
import { DocShell } from "@/components/docs/doc-shell";

export const metadata: Metadata = {
  title: "API Reference",
  description:
    "Complete API reference for edytlab — all Tauri commands, TypeScript bridge types, and SSE events.",
  alternates: { canonical: "/docs/api-reference" },
  openGraph: {
    title: "API Reference — edytlab Docs",
    description: "All Tauri commands, TypeScript types, and events for edytlab.",
    url: `${siteConfig.url}/docs/api-reference`,
  },
};

const commands = [
  {
    group: "Project",
    items: [
      {
        name: "openProject(path)",
        returns: "ProjectInfo",
        desc: "Open or create a project directory. Returns the current head node ID (null if empty).",
      },
    ],
  },
  {
    group: "API Keys",
    items: [
      {
        name: "setApiKey(key)",
        returns: "void",
        desc: "Store the API key for the active provider in the OS keychain.",
      },
      {
        name: "setApiKeyFor(provider, key)",
        returns: "void",
        desc: "Store the API key for a specific provider.",
      },
      {
        name: "hasApiKey()",
        returns: "boolean",
        desc: "Check whether the active provider has a key configured.",
      },
      {
        name: "hasApiKeyFor(provider)",
        returns: "boolean",
        desc: "Check whether a specific provider has a key.",
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
        desc: 'Returns ["anthropic", "openrouter", "openai"].',
      },
      {
        name: "getActiveProvider()",
        returns: "ProviderId",
        desc: "Returns the currently active provider ID.",
      },
      {
        name: "setActiveProvider(provider)",
        returns: "void",
        desc: "Switch the active provider. Rebuilds the agent.",
      },
      {
        name: "listModelsFor(provider, apiKey?)",
        returns: "ModelInfo[]",
        desc: "Fetch available models. Results cached 10 minutes.",
      },
      {
        name: "getActiveModel(provider)",
        returns: "string",
        desc: "Returns the model ID selected for the provider.",
      },
      {
        name: "setActiveModel(provider, model)",
        returns: "void",
        desc: "Set the active model for a provider.",
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
        desc: "Render the session to a WAV for playback. Cached by node id under .audiograph/previews/, so repeating a head — undo, redo — costs nothing.",
      },
      {
        name: "renderRange(node, startSec, endSec, outPath)",
        returns: "void",
        desc: "Render a time range to a specific output file.",
      },
      {
        name: "prepareCompare(a, b)",
        returns: "{ a_path, b_path }",
        desc: "Pre-render two nodes for A/B comparison.",
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
        name: "approvePlan()",
        returns: "void",
        desc: "In mashup mode: approve the agent's proposed plan to proceed.",
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
        returns: "string (annotation_id)",
        desc: "Add a named point marker.",
      },
      {
        name: "removeMarker(id)",
        returns: "NodeId",
        desc: "Remove a marker by annotation ID.",
      },
      {
        name: "listMarkers()",
        returns: "Marker[]",
        desc: "List all markers and regions for the current session.",
      },
    ],
  },
  {
    group: "Tracks",
    items: [
      {
        name: "batchLoad(paths)",
        returns: "LoadReport",
        desc: "Load several audio files in one call, each as its own track.",
      },
      {
        name: "listTracks()",
        returns: "TrackSummary[]",
        desc: "List all tracks in the current session head.",
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
        desc: "Copy the skills that ship with the app into the user's library. Returns how many were written.",
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
        returns: "PluginInstallReport",
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
    group: "Templates",
    items: [
      {
        name: "listTemplates()",
        returns: "TemplateInfo[]",
        desc: "Session templates that ship with the app.",
      },
      {
        name: "applyTemplate(name)",
        returns: "nodeId",
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
        returns: "{ path, duration_sec }",
        desc: "Stop capture and write the take to a WAV file.",
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
    payload: "{ id, ok: boolean }",
    desc: "Tool execution completed. ok = false if the tool returned an error.",
  },
  {
    name: "onNodeCreated(cb)",
    payload: "nodeId: string",
    desc: "New DAG node appended. Refresh the graph view and track list.",
  },
  {
    name: "onAgentDone(cb)",
    payload: "none",
    desc: "Agent turn fully complete — no more text or tool calls.",
  },
  {
    name: "onPlan(cb)",
    payload: "steps: object[]",
    desc: "Mashup mode: agent proposed a multi-step plan before execution.",
  },
  {
    name: "onMarkerChanged(cb)",
    payload: "none",
    desc: "Marker or region annotation added or removed. Refresh the marker list.",
  },
];

export default function ApiReferencePage() {
  return (
    <DocShell
      title="API Reference"
      description="All Tauri commands, TypeScript bridge types, and SSE events."
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
type ProviderId = "anthropic" | "openrouter" | "openai";

interface ProjectInfo  { path: string; head: NodeId | null }
interface SessionNode  { id: NodeId; parent: NodeId | null; label: string | null; state: SessionState }
interface GraphSummary { nodes: GraphNode[]; head: NodeId | null }
interface GraphNode    { id: NodeId; parent: NodeId | null; label: string | null; tool: string | null }
interface TrackSummary { id: string; name: string; muted: boolean; gain_db: number; audio_path: string | null }
interface ModelInfo    { id: string; display_name: string; context_length: number | null }
interface Marker       { id: string; name: string; kind: "marker" | "region"; time_sec?: number; start_sec?: number; end_sec?: number }`}</code>
      </pre>
    </DocShell>
  );
}
