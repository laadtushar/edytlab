/**
 * A click on a graph bubble previews that node and leaves the head alone
 * (#522).
 *
 * The saved undo path (`.audiograph/trail.json`, #453) follows the head the
 * window holds. A click used to move that head without moving the store's,
 * so the file ended at a node the store was not on, the next launch
 * ignored it, and undo went back to each node's first parent (#398) and
 * skipped steps the user took. Inside one session the previewed node also
 * sat on the path: an edit made after a click was undone to the node that
 * was only looked at.
 *
 * The real `App` runs with only the IPC boundary replaced, the way
 * `App.trail.test.tsx` runs it: the backend here keeps the path file as
 * the real one does, and a restart is the app unmounted and mounted again
 * against the same backend.
 */

import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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
import type { SavedTrail } from "../lib/headTrail";

type Args = Record<string, unknown>;

/** A node id; the status bar shows the first seven characters. */
const node = (n: number): string => n.toString(16).padStart(7, "0").padEnd(64, "0");

/** The load before the project's state, the state, and the state muted. */
const BEFORE_LOAD = node(1);
const START = node(2);
const MUTED = node(3);

/** What the status bar says for a head. */
const headLabel = (id: string): string => `head ${id.slice(0, 7)}`;

const NO_PATH: SavedTrail = { head: null, trail: [], redo: [] };

/**
 * One project's store, as the commands the app uses see it: a head that
 * mute and unmute move between two nodes (the unmute lands on the node
 * that already exists, whose stored parent is the load before it), the
 * graph of the three nodes, and the path file.
 */
class Backend {
  head = START;
  muted = false;
  /** `.audiograph/trail.json`: what `save_head_trail` last wrote. */
  file: SavedTrail = NO_PATH;
  /** Every command the app sent, in order. */
  readonly sent: { cmd: string; args: Args }[] = [];
  /** How many commands of any kind have been received. */
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
      case "get_node":
        return { id: args.id, parent: args.id === START ? BEFORE_LOAD : null };
      case "get_session_head":
        return this.head;
      case "get_head_trail":
        return this.file;
      case "save_head_trail":
        this.file = args.trail as SavedTrail;
        return null;
      case "get_graph":
        return {
          nodes: [
            { id: BEFORE_LOAD, parent: null, label: "load", tool: "load", created_at: at(0) },
            { id: START, parent: BEFORE_LOAD, label: "load", tool: "load", created_at: at(1) },
            { id: MUTED, parent: START, label: "mute", tool: "set_track_muted", created_at: at(2) },
          ],
          head: this.head,
        };
      case "render_preview":
        // Named after the node, as the real one is.
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
        return [];
      case "get_transcript":
        return { words: [], source: { kind: "unknown" } };
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
      case "open_project":
        return { path: args.path, head: this.head };
      case "plugin:dialog|open":
        return "/tmp/other-project";
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

  /** The args of every `cmd` the app sent. */
  of(cmd: string): Args[] {
    return this.sent.filter((s) => s.cmd === cmd).map((s) => s.args);
  }

  get undoneTo(): string[] {
    return this.of("set_head_to").map((a) => a.nodeId as string);
  }

  get askedForParents(): number {
    return this.of("get_node").length;
  }

  get saves(): SavedTrail[] {
    return this.of("save_head_trail").map((a) => a.trail as SavedTrail);
  }
}

const at = (n: number) => `2026-10-09T00:0${n}:00Z`;

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

/**
 * Wait until the status bar says the head is `id`.
 *
 * The head is adopted when a command's answer arrives, outside any `act`,
 * so React draws it on its own schedule. `quiet` counts the backend's
 * calls and says nothing about what has been drawn, so a read of the bar
 * right after it can still see the head the app has just left.
 */
const headShown = (id: string) =>
  waitFor(() =>
    expect(screen.getByTestId("status-bar-head")).toHaveTextContent(headLabel(id)),
  );

/** Open the app on `backend`, and wait until it is at rest on its head. */
async function open(backend: Backend) {
  mockIPC((cmd, args) => backend.handle(cmd, args as Args | undefined), {
    shouldMockEvents: true,
  });
  render(<App />);
  await screen.findByTestId("timeline-lane-mute");
  await headShown(backend.head);
  await quiet(backend);
}

/** Close the app: nothing the webview held survives. */
function close() {
  cleanup();
  clearMocks();
}

/**
 * Click the mute button, and wait until the lane shows the track as
 * `pressed` and the status bar shows the head the edit made (`MUTED`, and
 * `START` again for an unmute). The button toggles what it was last drawn
 * with, so a second click made before the first is drawn would send the
 * same mute again. The lane follows the track list and the bar follows the
 * head: they are two answers, and React may draw one before the other.
 */
async function mute(pressed: boolean) {
  fireEvent.click(screen.getByTestId("timeline-lane-mute"));
  await waitFor(() =>
    expect(screen.getByTestId("timeline-lane-mute")).toHaveAttribute(
      "aria-pressed",
      String(pressed),
    ),
  );
  await headShown(pressed ? MUTED : START);
}

const undo = () => fireEvent.keyDown(window, { key: "z", ctrlKey: true });

/** The history graph's bubble for `id`, if the graph has drawn it. */
function findBubble(id: string): HTMLElement {
  const found = screen
    .getAllByTestId("graph-bubble")
    .find((b) => b.getAttribute("data-node-id") === id);
  if (!found) throw new Error(`no bubble for ${id}`);
  return found;
}

/** The history graph's bubble for `id`, once the graph has drawn. */
async function bubble(id: string): Promise<HTMLElement> {
  return waitFor(() => findBubble(id));
}

/** Wait until the bubble for `id` has its `data-is-head` and `data-is-preview` marks. */
const bubbleMarks = (id: string, isHead: boolean, isPreview: boolean) =>
  waitFor(() => {
    const found = findBubble(id);
    expect(found).toHaveAttribute("data-is-head", String(isHead));
    expect(found).toHaveAttribute("data-is-preview", String(isPreview));
  });

/** Show the history graph. */
async function openGraph() {
  fireEvent.click(screen.getByTestId("tab-graph"));
  await bubble(START);
}

/** Click the bubble for `id` and wait until its preview has been rendered. */
async function preview(backend: Backend, id: string) {
  const before = backend.of("render_preview").length;
  fireEvent.click(await bubble(id));
  await waitFor(() => expect(backend.of("render_preview").length).toBe(before + 1));
  expect(backend.of("render_preview")[before]).toEqual({ node: id });
  await quiet(backend);
}

/**
 * "Set as head" from the right-click menu of the bubble for `id`, and wait
 * until the store has been asked and the status bar shows the new head.
 */
async function setAsHead(backend: Backend, id: string) {
  fireEvent.contextMenu(await bubble(id));
  fireEvent.click(
    within(screen.getByTestId("graph-context-menu")).getByRole("menuitem", {
      name: "Set as head",
    }),
  );
  await waitFor(() => expect(backend.undoneTo[backend.undoneTo.length - 1]).toBe(id));
  await headShown(id);
  await quiet(backend);
}

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
  close();
  vi.restoreAllMocks();
});

describe("a preview click in the graph", () => {
  it("leaves the head and the saved path where they were", async () => {
    const backend = new Backend();
    await open(backend);
    await mute(true); // MUTED, with START on the path
    await quiet(backend);
    const saved = backend.file;
    const saves = backend.saves.length;
    expect(saved).toEqual({ head: MUTED, trail: [START], redo: [] });

    await openGraph();
    await preview(backend, BEFORE_LOAD);

    // The window is still on the head, and so is the file.
    expect(screen.getByTestId("status-bar-head")).toHaveTextContent(headLabel(MUTED));
    expect(backend.saves).toHaveLength(saves);
    expect(backend.file).toEqual(saved);
    // And the store was not asked to go anywhere.
    expect(backend.of("set_head_to")).toEqual([]);
  });

  it("keeps the head ring on the head, and gives the previewed node its own mark", async () => {
    const backend = new Backend();
    await open(backend);
    await mute(true);
    await quiet(backend);

    await openGraph();
    await preview(backend, BEFORE_LOAD);

    const head = await bubble(MUTED);
    const previewed = await bubble(BEFORE_LOAD);
    expect(head).toHaveAttribute("data-is-head", "true");
    expect(head).toHaveAttribute("data-is-preview", "false");
    expect(previewed).toHaveAttribute("data-is-head", "false");
    expect(previewed).toHaveAttribute("data-is-preview", "true");
    // Nothing else is marked.
    expect(await bubble(START)).toHaveAttribute("data-is-head", "false");
    expect(await bubble(START)).toHaveAttribute("data-is-preview", "false");
  });

  it("is said in the status bar, beside the head and not in place of it", async () => {
    const backend = new Backend();
    await open(backend);
    expect(screen.queryByTestId("status-bar-preview")).not.toBeInTheDocument();
    await openGraph();

    await preview(backend, BEFORE_LOAD);

    expect(screen.getByTestId("status-bar-head")).toHaveTextContent(headLabel(START));
    expect(screen.getByTestId("status-bar-preview")).toHaveTextContent(
      `previewing ${BEFORE_LOAD.slice(0, 7)}`,
    );
    // What is loaded is a render of what the bar says is being previewed,
    // so it is not reported as out of date.
    expect(screen.queryByTestId("status-bar-mix-stale")).not.toBeInTheDocument();
  });

  it("ends when the head moves: the preview is not the new head's mix", async () => {
    const backend = new Backend();
    await open(backend);
    await openGraph();
    await preview(backend, BEFORE_LOAD);
    expect(screen.getByTestId("status-bar-preview")).toBeInTheDocument();

    // Back to the timeline, where the mute is.
    fireEvent.click(screen.getByTestId("tab-timeline"));
    await mute(true);
    await quiet(backend);

    // The head is drawn when React gets to it, which can be after `quiet`.
    await headShown(MUTED);
    // The preview is derived from the head it was made under (see
    // `useGraphPreview`), so the render that shows the new head is not
    // one that still shows the preview: no wait for this one.
    expect(screen.queryByTestId("status-bar-preview")).not.toBeInTheDocument();
  });

  it("is not an edit's parent: undo after an edit made following a preview goes back to the head it left", async () => {
    // The click used to put the previewed node on the path, so the next
    // edit's undo went to a node the edit never came from.
    const backend = new Backend();
    await open(backend);
    await openGraph();
    await preview(backend, BEFORE_LOAD);

    fireEvent.click(screen.getByTestId("tab-timeline"));
    await mute(true); // MUTED, made from START
    await quiet(backend);
    undo();

    await waitFor(() => expect(backend.undoneTo).toEqual([START]));
  });

  it("clicking the head itself previews nothing apart from it", async () => {
    const backend = new Backend();
    await open(backend);
    await openGraph();

    await preview(backend, START);

    expect(screen.queryByTestId("status-bar-preview")).not.toBeInTheDocument();
    expect(await bubble(START)).toHaveAttribute("data-is-preview", "false");
    expect(backend.saves).toEqual([]);
  });
});

describe("after a preview click and a restart", () => {
  it("undo still retraces the path the user took", async () => {
    const backend = new Backend();
    await open(backend);
    await mute(true); // MUTED
    await quiet(backend);
    await mute(false); // unmuted: START again, the node it began on
    await quiet(backend);
    expect(backend.file).toEqual({ head: START, trail: [START, MUTED], redo: [] });

    // The last thing the user did was look at another node.
    await openGraph();
    await preview(backend, MUTED);
    expect(backend.head).toBe(START);

    // The app is closed and opened again, on the same project.
    close();
    await open(backend);

    // The path was kept, so the first undo is the mute, which the stored
    // parent of START (the load) does not know about.
    undo();
    await waitFor(() => expect(backend.undoneTo).toEqual([MUTED]));
    await quiet(backend);
    undo();
    await waitFor(() => expect(backend.undoneTo).toEqual([MUTED, START]));
    expect(backend.askedForParents).toBe(0);
  });

  it("redo too", async () => {
    const backend = new Backend();
    await open(backend);
    await mute(true);
    await quiet(backend);
    undo(); // back to START, with the mute to redo
    await waitFor(() => expect(backend.undoneTo).toEqual([START]));
    await quiet(backend);

    await openGraph();
    await preview(backend, BEFORE_LOAD);

    close();
    await open(backend);

    fireEvent.keyDown(window, { key: "y", ctrlKey: true });
    await waitFor(() => expect(backend.undoneTo).toEqual([START, MUTED]));
  });
});

describe("Set as head", () => {
  it("still moves the head and the saved path, from a previewed node too", async () => {
    const backend = new Backend();
    await open(backend);
    await mute(true); // MUTED, with START on the path
    await quiet(backend);
    await openGraph();
    await preview(backend, BEFORE_LOAD);
    expect(backend.file).toEqual({ head: MUTED, trail: [START], redo: [] });

    await setAsHead(backend, BEFORE_LOAD);

    expect(backend.undoneTo).toEqual([BEFORE_LOAD]);
    await headShown(BEFORE_LOAD);
    // The jump is a step: the file ends at the new head, with the head it
    // left on the path.
    await waitFor(() =>
      expect(backend.file).toEqual({ head: BEFORE_LOAD, trail: [START, MUTED], redo: [] }),
    );
    // The preview was of the node that is now the head.
    expect(screen.queryByTestId("status-bar-preview")).not.toBeInTheDocument();
    // The bubbles take their marks from the head the window holds, and may
    // be drawn after the bar is.
    await bubbleMarks(BEFORE_LOAD, true, false);
  });

  it("survives a restart and undo comes back from it", async () => {
    const backend = new Backend();
    await open(backend);
    await mute(true);
    await quiet(backend);
    await openGraph();
    await setAsHead(backend, BEFORE_LOAD);
    await waitFor(() => expect(backend.file.head).toBe(BEFORE_LOAD));
    await quiet(backend);

    close();
    await open(backend);

    undo();
    await waitFor(() => expect(backend.undoneTo).toEqual([BEFORE_LOAD, MUTED]));
    expect(backend.askedForParents).toBe(0);
  });
});
