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
import { clearMocks } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("wavesurfer.js", async () => (await import("./wavesurferMock")).wavesurferModule());

import { type Args, appAnswer, bootIpc, node, settle, voiceTrack } from "./appHarness";
import App from "../App";

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
      case "get_view_state":
        return this.view;
      case "get_session_head":
        return this.head;
      case "get_node":
        return { id: args.id, parent: null };
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
        return appAnswer(cmd);
    }
  };

  /** The commands named `cmd`, in order. */
  named(cmd: string): Args[] {
    return this.calls.filter((c) => c.cmd === cmd).map((c) => c.args);
  }

  private track() {
    return voiceTrack();
  }
}

async function start(): Promise<Backend> {
  const backend = new Backend();
  bootIpc(backend);
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
