/**
 * Opening a project keeps the head the project has (#507).
 *
 * `view.json` remembers where the user was, head included, and is written
 * 500 ms after the view settles. So the head it names is the project's
 * `HEAD` or something older: older when an edit landed in the last
 * half-second before quitting, or when the head moved in a way the app did
 * not adopt. Opening a project used to obey it with `set_head_to`, which
 * moves `HEAD` back *on disk*, so the newer edits dropped off the timeline
 * and nothing pointed at them.
 *
 * Boot stopped doing that: it applies zoom, selection and playhead and
 * leaves the head to the backend, which restored it. Opening a project now
 * does the same, taking the head from what `open_project` returned.
 *
 * The real `App` runs here with only the IPC boundary replaced.
 */

import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it, vi } from "vitest";

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

/**
 * A node id. The status bar shows the first seven characters, so each id
 * is told apart by them: `node(2)` reads `head 0000002`.
 */
const node = (n: number): string => n.toString(16).padStart(7, "0").padEnd(64, "0");
const headLabel = (n: number): string => `head ${node(n).slice(0, 7)}`;

const OTHER = "/home/user/Music/other";

/** What the saved view of the project being opened says. */
interface SavedView {
  head?: string;
  zoom_px_per_sec?: number;
  selection?: [number, number];
  playhead_sec?: number;
}

/**
 * A backend with one project open (head 1) and another to open. The one to
 * open has its own `HEAD`, node 3, and a `view.json` that names node 2: the
 * head from before the edit that made node 3.
 */
class Backend {
  /** Every command received, in order, with its arguments. */
  readonly calls: { cmd: string; args: Args }[] = [];
  head = node(1);
  view: SavedView = {};
  openedHead = node(3);

  handle = (cmd: string, args: Args = {}): unknown => {
    this.calls.push({ cmd, args });
    switch (cmd) {
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
        return this.view;
      case "get_plan_first":
      case "get_sync_lock":
        return false;
      case "get_session_head":
        return this.head;
      case "get_node":
        return { id: args.id, parent: null };
      case "save_view_state":
      case "set_selection_context":
        return null;
      // The folder picker, answered with the folder to open.
      case "plugin:dialog|open":
        return OTHER;
      case "open_project":
        this.head = this.openedHead;
        return { path: OTHER, head: this.openedHead };
      // What the real command does if it is asked: move `HEAD`, durably.
      case "set_head_to":
        this.head = args.nodeId as string;
        return this.head;
      case "list_tracks":
        return [this.track()];
      case "get_graph":
        return { nodes: [], head: this.head };
      default:
        return undefined;
    }
  };

  /** The commands named `cmd`, in order. */
  named(cmd: string): Args[] {
    return this.calls.filter((c) => c.cmd === cmd).map((c) => c.args);
  }

  private track() {
    return {
      id: "3f2b8c1e-7d4a-4e9b-9c6f-1a2b3c4d5e6f",
      name: "Voice",
      muted: false,
      gain_db: 0,
      pan: 0,
      soloed: false,
      audio_path: "/tmp/voice.wav",
      clips: [
        { start_sec: 0, length_sec: 10, source_path: "/tmp/voice.wav", volume_envelope: [] },
      ],
    };
  }
}

/** Let what the app does next happen. */
const settle = () =>
  act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });

async function start(): Promise<Backend> {
  const backend = new Backend();
  mockIPC((cmd, args) => backend.handle(cmd, args as Args | undefined), {
    shouldMockEvents: true,
  });
  render(<App />);
  await screen.findByTestId("timeline-lane-gain");
  await waitFor(() =>
    expect(screen.getByTestId("status-bar-head")).toHaveTextContent(headLabel(1)),
  );
  await settle();
  return backend;
}

/** Click "Open project..." and wait until the open has run to its end. */
async function openOther(backend: Backend): Promise<void> {
  const before = backend.named("list_recent_projects").length;
  await act(async () => {
    screen.getByTestId("open-project-button").click();
  });
  // The open is over when it has refreshed the recents list, which it does
  // last, after the project, the view and the tracks.
  await waitFor(() =>
    expect(backend.named("list_recent_projects").length).toBeGreaterThan(before),
  );
  await settle();
}

afterEach(() => {
  // Unmount first: the app unlistens through the mocked IPC on its way out.
  cleanup();
  clearMocks();
  vi.restoreAllMocks();
});

describe("opening a project whose saved view names an older head", () => {
  it("does not move the project's head back to it", async () => {
    const backend = await start();
    backend.view = { head: node(2), zoom_px_per_sec: 120, selection: [0.5, 1.5] };

    await openOther(backend);

    expect(backend.named("open_project")).toEqual([{ path: OTHER }]);
    expect(
      backend.named("set_head_to"),
      "opening the project moved its HEAD back on disk, to the head view.json names",
    ).toEqual([]);
  });

  it("shows the head the store reported", async () => {
    const backend = await start();
    backend.view = { head: node(2) };

    await openOther(backend);

    expect(screen.getByTestId("status-bar-head")).toHaveTextContent(headLabel(3));
  });

  it("still restores the rest of the view", async () => {
    const backend = await start();
    backend.view = { head: node(2), selection: [0.5, 1.5] };
    expect(screen.queryByTestId("status-bar-selection")).toBeNull();

    await openOther(backend);

    await waitFor(() => expect(screen.getByTestId("status-bar-selection")).toBeVisible());
    expect(screen.getByTestId("status-bar-selection")).toHaveTextContent("sel 0:00.50 → 0:01.50");
  });
});
