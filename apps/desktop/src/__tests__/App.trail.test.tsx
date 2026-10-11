/**
 * The path the user took survives a restart, in the real app (#453).
 *
 * Undo goes back along the heads the user was on, because a node's stored
 * parent is wrong once an edit has returned to an earlier state (mute,
 * then unmute: #398). That path used to live in the webview alone, so
 * after a restart undo fell back on the parent and skipped the mute. It is
 * now saved with the project, in the store's directory, and read back as
 * the project opens.
 *
 * The real `App` runs with only the IPC boundary replaced. The backend
 * here keeps the saved path the way the real one does: `save_head_trail`
 * writes it, `get_head_trail` reads what was written, and a restart is the
 * app unmounted and mounted again against the same backend, with nothing
 * carried over in the webview.
 */

import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { held, type Held } from "./held";

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
const headLabel = (n: number): string => `head ${node(n).slice(0, 7)}`;

/** The load before the project's state, the state, and the state muted. */
const BEFORE_LOAD = node(1);
const START = node(2);
const MUTED = node(3);

const NO_PATH: SavedTrail = { head: null, trail: [], redo: [] };

/**
 * One project's store, as the commands the app uses see it: a head that
 * mute and unmute move between two nodes (the unmute lands on the node
 * that already exists, whose stored parent is the load before it), and
 * the path file.
 */
class Backend {
  head = START;
  muted = false;
  /** `.audiograph/trail.json`: what `save_head_trail` last wrote. */
  file: SavedTrail = NO_PATH;
  /** Every command the app sent, in order. */
  readonly sent: { cmd: string; args: Args }[] = [];
  /** Every path the app asked to save. */
  readonly saves: SavedTrail[] = [];
  /** When set, `save_head_trail` is not answered until it is released. */
  saveHeld: Held<null> | null = null;
  /** How many commands of any kind have been received. */
  calls = 0;

  handle = (cmd: string, args: Args = {}): unknown => {
    this.calls++;
    if (cmd.startsWith("set_") || cmd.startsWith("get_node") || cmd.endsWith("_head_trail")) {
      this.sent.push({ cmd, args });
    }
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
      case "save_head_trail": {
        const trail = args.trail as SavedTrail;
        this.saves.push(trail);
        // `session::trail::write_trail`: a path that does not end at a
        // node of the project is dropped.
        const wait = this.saveHeld;
        const write = () => {
          this.file = trail;
          return null;
        };
        return wait ? wait.promise.then(write) : write();
      }
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

  get undoneTo(): string[] {
    return this.sent.filter((s) => s.cmd === "set_head_to").map((s) => s.args.nodeId as string);
  }

  get askedForParents(): number {
    return this.sent.filter((s) => s.cmd === "get_node").length;
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
    expect(screen.getByTestId("status-bar-head")).toHaveTextContent(
      backend.head === START ? headLabel(2) : headLabel(3),
    ),
  );
  await quiet(backend);
}

/** Close the app: nothing the webview held survives. */
function close() {
  cleanup();
  clearMocks();
}

/**
 * Click the mute button, and wait until the lane shows the track as
 * `pressed`. The button toggles what it was last drawn with, so a second
 * click made before the first is drawn would send the same mute again.
 */
async function mute(pressed: boolean) {
  fireEvent.click(screen.getByTestId("timeline-lane-mute"));
  await waitFor(() =>
    expect(screen.getByTestId("timeline-lane-mute")).toHaveAttribute(
      "aria-pressed",
      String(pressed),
    ),
  );
}
const undo = () => fireEvent.keyDown(window, { key: "z", ctrlKey: true });

beforeEach(() => {
  vi.spyOn(window, "prompt").mockReturnValue("verse");
});

afterEach(() => {
  close();
  vi.restoreAllMocks();
});

describe("after a restart", () => {
  it("undo retraces mute, unmute: first to the mute, which the stored parent does not know", async () => {
    const backend = new Backend();
    await open(backend);

    await mute(true); // MUTED
    await quiet(backend);
    await mute(false); // unmuted: START again, the node it began on
    await quiet(backend);
    expect(backend.head).toBe(START);

    // The app is closed and opened again, on the same project.
    close();
    await open(backend);

    undo();
    await waitFor(() => expect(backend.undoneTo).toEqual([MUTED]));
    await quiet(backend);
    undo();
    await waitFor(() => expect(backend.undoneTo).toEqual([MUTED, START]));
    await quiet(backend);
    // Not a single look at the stored parent while the path had steps.
    expect(backend.askedForParents).toBe(0);

    // The path is spent: now the stored parent is the right answer.
    undo();
    await waitFor(() => expect(backend.undoneTo).toEqual([MUTED, START, BEFORE_LOAD]));
    expect(backend.askedForParents).toBe(1);
  });

  it("redo survives it too", async () => {
    const backend = new Backend();
    await open(backend);

    await mute(true);
    await quiet(backend);
    undo(); // back to START, with the mute to redo
    await waitFor(() => expect(backend.undoneTo).toEqual([START]));
    await quiet(backend);

    close();
    await open(backend);

    fireEvent.keyDown(window, { key: "y", ctrlKey: true });
    await waitFor(() => expect(backend.undoneTo).toEqual([START, MUTED]));
  });

  it("a project with no saved path undoes by the stored parent, as before", async () => {
    const backend = new Backend();
    await open(backend);

    undo();

    await waitFor(() => expect(backend.undoneTo).toEqual([BEFORE_LOAD]));
    expect(backend.askedForParents).toBe(1);
  });

  it("a saved path that ends at another head is not used", async () => {
    // A save lost to a crash: the file is one move behind the store.
    const backend = new Backend();
    backend.file = { head: MUTED, trail: [START], redo: [] };
    await open(backend);

    undo();

    await waitFor(() => expect(backend.undoneTo).toEqual([BEFORE_LOAD]));
  });

  it("a backend that cannot read the path still opens the project", async () => {
    const backend = new Backend();
    const handle = backend.handle;
    backend.handle = (cmd, args) => {
      if (cmd === "get_head_trail") return Promise.reject("could not read trail.json");
      return handle(cmd, args);
    };
    await open(backend);

    undo();

    await waitFor(() => expect(backend.undoneTo).toEqual([BEFORE_LOAD]));
  });
});

describe("opening another project", () => {
  it("takes the path saved with that project", async () => {
    const backend = new Backend();
    await open(backend);

    // The project to open was closed on a mute and an unmute.
    backend.file = { head: START, trail: [START, MUTED], redo: [] };
    fireEvent.click(screen.getByTestId("open-project-button"));
    await waitFor(() =>
      expect(backend.sent.filter((s) => s.cmd === "get_head_trail")).toHaveLength(2),
    );
    await quiet(backend);

    undo();
    await waitFor(() => expect(backend.undoneTo).toEqual([MUTED]));
    expect(backend.askedForParents).toBe(0);
  });
});

describe("saving the path", () => {
  it("writes the whole path after each move, so the file always ends at the head", async () => {
    const backend = new Backend();
    await open(backend);

    await mute(true);
    await waitFor(() => expect(backend.file.head).toBe(MUTED));
    await quiet(backend);
    expect(backend.file).toEqual({ head: MUTED, trail: [START], redo: [] });

    await mute(false);
    await waitFor(() => expect(backend.file.head).toBe(START));
    await quiet(backend);
    expect(backend.file).toEqual({ head: START, trail: [START, MUTED], redo: [] });

    undo();
    await waitFor(() => expect(backend.file.head).toBe(MUTED));
    await quiet(backend);
    expect(backend.file).toEqual({ head: MUTED, trail: [START], redo: [START] });
  });

  it("does not hold up an edit, or an undo, behind a save the disk has not finished", async () => {
    const backend = new Backend();
    await open(backend);
    backend.saveHeld = held<null>();

    await mute(true);
    await waitFor(() => expect(backend.saves).toHaveLength(1));
    // The first save is not answered. The next edit, and an undo behind
    // it, still reach the session, in order.
    await mute(false);
    await waitFor(() =>
      expect(backend.sent.filter((s) => s.cmd === "set_track_muted")).toHaveLength(2),
    );
    undo();
    await waitFor(() => expect(backend.undoneTo).toEqual([MUTED]));

    // And only one save is in flight: the others wait behind it.
    await settle();
    expect(backend.saves).toHaveLength(1);

    // When it is answered, the newest path is what is written next.
    await backend.saveHeld.resolve(null);
    await waitFor(() => expect(backend.saves).toHaveLength(2));
    expect(backend.saves[1]).toEqual({ head: MUTED, trail: [START], redo: [START] });
    // Release the second as well, so nothing is left pending.
    backend.saveHeld = null;
  });
});
