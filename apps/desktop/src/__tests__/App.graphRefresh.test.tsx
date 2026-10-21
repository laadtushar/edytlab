/**
 * The history graph, while it is open, shows the session as it is (#602).
 *
 * `GraphView` read the store on mount and again only when the app bumped
 * its `refreshKey`, which it did on `agent://node-created` and nothing
 * else. So an open graph went stale whenever the session changed some
 * other way:
 *
 * - `compact_session` deletes nodes and makes none, so the nodes it
 *   removed stayed drawn, and a click on one asked the store for a node it
 *   no longer had;
 * - the agent's `name_node` sets a label without moving the head (#601),
 *   so no `node-created` followed and the label never showed;
 * - the user's own edits (a file loaded from the header), another project
 *   opened, and undo, redo and "Set as head" moved the head without the
 *   graph hearing of it.
 *
 * The real `App` runs with only the IPC boundary replaced, as
 * `App.graphPreview.test.tsx` runs it. The backend here keeps a node list
 * that tests change the way the store would, and counts the graph reads.
 */

import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("wavesurfer.js", () => ({
  default: {
    create: () => ({
      on: vi.fn(),
      un: vi.fn(),
      load: vi.fn(),
      zoom: vi.fn(),
      play: vi.fn(),
      pause: vi.fn(),
      seekTo: vi.fn(),
      setTime: vi.fn(),
      setVolume: vi.fn(),
      setOptions: vi.fn(),
      isPlaying: vi.fn(() => false),
      destroy: vi.fn(),
      getDuration: () => 10,
      getCurrentTime: () => 0,
    }),
  },
}));

import App from "../App";

type Args = Record<string, unknown>;

interface Stored {
  id: string;
  parent: string | null;
  label: string | null;
  tool: string | null;
  created_at: string;
}

/** A node id; the status bar shows the first seven characters. */
const node = (n: number): string => n.toString(16).padStart(7, "0").padEnd(64, "0");

const BEFORE_LOAD = node(1);
const START = node(2);
const MUTED = node(3);
/** The node a file loaded from the header adds. */
const LOADED = node(4);
/** Another project's nodes. */
const OTHER_ROOT = node(5);
const OTHER_HEAD = node(6);

const at = (n: number) => `2026-10-21T00:0${n}:00Z`;
const stored = (
  id: string,
  parent: string | null,
  label: string | null,
  tool: string,
  n: number,
): Stored => ({ id, parent, label, tool, created_at: at(n) });

const headLabel = (id: string): string => `head ${id.slice(0, 7)}`;

/** The project the app opens on: a load, the project's state, and a mute. */
const firstProject = (): Stored[] => [
  stored(BEFORE_LOAD, null, null, "load", 0),
  stored(START, BEFORE_LOAD, null, "load", 1),
  stored(MUTED, START, null, "set_track_muted", 2),
];

const otherProject = (): Stored[] => [
  stored(OTHER_ROOT, null, "second project", "load", 0),
  stored(OTHER_HEAD, OTHER_ROOT, "second project, edited", "gain", 1),
];

class Backend {
  head = START;
  muted = false;
  /** The store's nodes. A test changes this the way the store would change. */
  nodes: Stored[] = firstProject();
  readonly sent: { cmd: string; args: Args }[] = [];
  calls = 0;

  handle = (cmd: string, args: Args = {}): unknown => {
    this.calls++;
    this.sent.push({ cmd, args });
    switch (cmd) {
      case "set_track_muted":
        this.muted = args.muted as boolean;
        this.head = this.muted ? MUTED : START;
        return this.head;
      case "set_head_to":
        this.head = args.nodeId as string;
        this.muted = this.head === MUTED;
        return this.head;
      case "batch_load":
        // The header's open-file button: a `load` node on the head.
        this.nodes = [...this.nodes, stored(LOADED, this.head, null, "load", 3)];
        this.head = LOADED;
        return { tracks_loaded: 1, last_node_id: LOADED, failures: [] };
      case "open_project":
        this.nodes = otherProject();
        this.head = OTHER_HEAD;
        return { path: args.path, head: this.head };
      case "get_node":
        return { id: args.id, parent: null };
      case "get_session_head":
        return this.head;
      case "get_head_trail":
        return { head: null, trail: [], redo: [] };
      case "save_head_trail":
        return null;
      case "get_graph":
        return { nodes: this.nodes.map((n) => ({ ...n })), head: this.head };
      case "render_preview":
        return `/tmp/edytlab-preview-${args.node as string}.wav`;
      case "has_api_key":
        return true;
      case "get_keychain_persistence":
        return { persistent: true, reason: null };
      case "install_bundled_skills":
        return 0;
      case "list_templates":
      case "list_recent_projects":
      case "list_markers":
      case "get_transcript":
        return [];
      case "get_view_state":
        return {};
      case "get_plan_first":
      case "get_sync_lock":
        return false;
      case "save_view_state":
      case "set_selection_context":
        return null;
      case "list_tracks":
        return [this.track()];
      case "plugin:dialog|open":
        return "/tmp/picked";
      default:
        return undefined;
    }
  };

  private track() {
    return {
      id: "3f2b8c1e-7d4a-4e9b-9c6f-1a2b3c4d5e6f",
      name: "Voice",
      muted: this.muted,
      gain_db: 0,
      pan: 0,
      soloed: false,
      audio_path: "/tmp/voice.wav",
      clips: [
        { start_sec: 0, length_sec: 10, source_path: "/tmp/voice.wav", volume_envelope: [] },
      ],
    };
  }

  /** How many times the app has read the graph. */
  get graphReads(): number {
    return this.sent.filter((s) => s.cmd === "get_graph").length;
  }

  get movedTo(): string[] {
    return this.sent.filter((s) => s.cmd === "set_head_to").map((s) => s.args.nodeId as string);
  }

  /** Name a node, as the agent's `name_node` does: no new node, no head move. */
  label(id: string, label: string) {
    this.nodes = this.nodes.map((n) => (n.id === id ? { ...n, label } : n));
  }

  /** The store loses `ids`, as `compact_session` with `apply` does. */
  compact(ids: string[]) {
    this.nodes = this.nodes.filter((n) => !ids.includes(n.id));
  }
}

/** Let what the app does next happen. */
const settle = () =>
  act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });

/** Wait until the app has stopped asking for things. */
async function quiet(backend: Backend): Promise<void> {
  for (let guard = 0; guard < 20; guard++) {
    const before = backend.calls;
    await settle();
    if (backend.calls === before) return;
  }
  throw new Error("the app never stopped asking");
}

/** Open the app on `backend`, and wait until it is at rest on its head. */
async function open(backend: Backend) {
  mockIPC((cmd, args) => backend.handle(cmd, args as Args | undefined), {
    shouldMockEvents: true,
  });
  render(<App />);
  await screen.findByTestId("timeline-lane-mute");
  await waitFor(() =>
    expect(screen.getByTestId("status-bar-head")).toHaveTextContent(headLabel(backend.head)),
  );
  await quiet(backend);
}

const undo = () => fireEvent.keyDown(window, { key: "z", ctrlKey: true });

const bubbles = (): HTMLElement[] => screen.queryAllByTestId("graph-bubble");
const bubbleFor = (id: string): HTMLElement | undefined =>
  bubbles().find((b) => b.getAttribute("data-node-id") === id);

/** The bubble for `id`, once the graph has drawn it. */
async function bubble(id: string): Promise<HTMLElement> {
  return waitFor(() => {
    const found = bubbleFor(id);
    if (!found) throw new Error(`no bubble for ${id}`);
    return found;
  });
}

/** Show the history graph, and wait until it has drawn the first project. */
async function openGraph(backend: Backend) {
  fireEvent.click(screen.getByTestId("tab-graph"));
  await bubble(START);
  await quiet(backend);
}

/** Tell the window about an agent tool call that starts and ends. */
async function toolCall(name: string, id: string) {
  await act(async () => {
    await emit("agent://tool-call", { name, id });
  });
  return () =>
    act(async () => {
      await emit("agent://tool-call-end", { id, ok: true, not_run: false });
    });
}

/** The agent's turn ends. */
const agentDone = () =>
  act(async () => {
    await emit("agent://done", {});
  });

beforeEach(() => {
  // React Flow measures what it draws; jsdom has neither of these.
  if (!("ResizeObserver" in window)) {
    class RO {
      observe(): void {}
      unobserve(): void {}
      disconnect(): void {}
    }
    (window as unknown as { ResizeObserver: typeof RO }).ResizeObserver = RO;
  }
  if (!("DOMMatrixReadOnly" in window)) {
    class M {
      m22 = 1;
      constructor(_init?: string | number[]) {}
    }
    (window as unknown as { DOMMatrixReadOnly: typeof M }).DOMMatrixReadOnly = M;
  }
});

afterEach(() => {
  cleanup();
  clearMocks();
  vi.restoreAllMocks();
});

describe("an open graph after the agent compacts the session", () => {
  it("no longer draws the nodes the compaction removed", async () => {
    const backend = new Backend();
    await open(backend);
    await openGraph(backend);
    expect(await bubble(MUTED)).toBeInTheDocument();

    // `compact_session` with `apply`: the store loses MUTED and gains no
    // node, so there is no `node-created`, only the call starting and
    // ending.
    const end = await toolCall("compact_session", "toolu_compact");
    backend.compact([MUTED]);
    await end();

    await waitFor(() => expect(bubbleFor(MUTED)).toBeUndefined());
    expect(bubbleFor(START)).toBeDefined();
    expect(bubbleFor(BEFORE_LOAD)).toBeDefined();
  });

  it("reads the graph once for it", async () => {
    const backend = new Backend();
    await open(backend);
    await openGraph(backend);
    // Nothing has been undone, so the path names no node and the window
    // has no reason to ask the store what it holds: the one read is the
    // view's.
    const reads = backend.graphReads;

    const end = await toolCall("compact_session", "toolu_compact");
    backend.compact([MUTED]);
    await end();
    await waitFor(() => expect(bubbleFor(MUTED)).toBeUndefined());
    await quiet(backend);

    expect(backend.graphReads).toBe(reads + 1);
  });

  it("leaves the graph alone for another tool", async () => {
    const backend = new Backend();
    await open(backend);
    await openGraph(backend);
    const reads = backend.graphReads;

    const end = await toolCall("storage_report", "toolu_report");
    await end();
    await quiet(backend);

    expect(backend.graphReads).toBe(reads);
  });
});

describe("an open graph after the agent names a node", () => {
  it("shows the label when the agent's turn ends", async () => {
    const backend = new Backend();
    await open(backend);
    await openGraph(backend);
    expect(await bubble(START)).not.toHaveTextContent("chorus");

    // `name_node` (#601) sets the label and leaves the head where it was:
    // no node is created, so no `node-created` follows.
    const end = await toolCall("name_node", "toolu_name");
    backend.label(START, "chorus");
    await end();
    await agentDone();

    await waitFor(() => expect(bubbleFor(START)).toHaveTextContent("chorus"));
    // It was a label and not a move.
    expect(screen.getByTestId("status-bar-head")).toHaveTextContent(headLabel(START));
  });
});

describe("an open graph after the user's own changes", () => {
  it("draws the node a file loaded from the header adds", async () => {
    const backend = new Backend();
    await open(backend);
    await openGraph(backend);
    expect(bubbleFor(LOADED)).toBeUndefined();

    fireEvent.click(screen.getByTestId("open-audio-button"));

    expect(await bubble(LOADED)).toHaveAttribute("data-is-head", "true");
    expect(await bubble(START)).toHaveAttribute("data-is-head", "false");
  });

  it("draws another project's nodes when it is opened", async () => {
    const backend = new Backend();
    await open(backend);
    await openGraph(backend);

    fireEvent.click(screen.getByTestId("open-project-button"));

    expect(await bubble(OTHER_HEAD)).toHaveTextContent("second project, edited");
    await waitFor(() => expect(bubbleFor(MUTED)).toBeUndefined());
    expect(bubbleFor(START)).toBeUndefined();
    expect(bubbles()).toHaveLength(2);
  });

  it("re-reads when undo moves the head", async () => {
    const backend = new Backend();
    await open(backend);
    // A mute, so there is a step to undo; then to the graph.
    fireEvent.click(screen.getByTestId("timeline-lane-mute"));
    await waitFor(() =>
      expect(screen.getByTestId("status-bar-head")).toHaveTextContent(headLabel(MUTED)),
    );
    await quiet(backend);
    fireEvent.click(screen.getByTestId("tab-graph"));
    await bubble(MUTED);
    await quiet(backend);
    const reads = backend.graphReads;

    undo();

    await waitFor(() => expect(backend.movedTo).toEqual([START]));
    await waitFor(() => expect(backend.graphReads).toBe(reads + 1));
    await quiet(backend);
    expect(backend.graphReads).toBe(reads + 1);
    expect(await bubble(START)).toHaveAttribute("data-is-head", "true");
  });

  it("re-reads once for Set as head, not once for the move and again for the click", async () => {
    const backend = new Backend();
    await open(backend);
    await openGraph(backend);
    const reads = backend.graphReads;

    fireEvent.contextMenu(await bubble(BEFORE_LOAD));
    fireEvent.click(
      within(screen.getByTestId("graph-context-menu")).getByRole("menuitem", {
        name: "Set as head",
      }),
    );
    await waitFor(() => expect(backend.movedTo).toEqual([BEFORE_LOAD]));
    await waitFor(() => expect(backend.graphReads).toBeGreaterThan(reads));
    await quiet(backend);

    expect(backend.graphReads).toBe(reads + 1);
    expect(await bubble(BEFORE_LOAD)).toHaveAttribute("data-is-head", "true");
  });
});

describe("a preview click in an open graph", () => {
  it("is not a reason to read the graph again, and moves nothing", async () => {
    const backend = new Backend();
    await open(backend);
    await openGraph(backend);
    const reads = backend.graphReads;

    fireEvent.click(await bubble(BEFORE_LOAD));
    await waitFor(() =>
      expect(backend.sent.filter((s) => s.cmd === "render_preview")).toHaveLength(1),
    );
    await quiet(backend);

    expect(backend.graphReads).toBe(reads);
    expect(backend.movedTo).toEqual([]);
    expect(screen.getByTestId("status-bar-head")).toHaveTextContent(headLabel(START));
    expect(await bubble(BEFORE_LOAD)).toHaveAttribute("data-is-preview", "true");
  });
});
