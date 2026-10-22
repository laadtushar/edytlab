/**
 * Every way the session moves by itself releases what the user holds on a
 * lane, and reads the tracks again (#596, ADR 0001).
 *
 * The rule (`lib/ownedControls.ts`): a lane control the user has set is the
 * user's until the session confirms it, and anything that moves the session
 * without the user's own commit (undo, redo, Set as head, opening a project)
 * calls `owned.clear()` before it re-reads the tracks. `App.laneControls`
 * pins undo and opening a project. Two moves were missing from the rule:
 *
 * - **Accept B** of an A/B compare moved the head and did neither. Nothing
 *   else re-reads the tracks on a head move, so the lanes went on showing
 *   side A's track list, and a control held before the accept stayed laid
 *   over it.
 * - **The assistant's edits** (`agent://node-created`) re-read the tracks
 *   but did not release the controls, so a slider the user had moved and
 *   not let go of stayed on the user's value over the assistant's.
 *
 * The real `App` runs with only the IPC boundary replaced, as in
 * `App.laneControls.test.tsx`. The backend can hold the answer to a
 * track-list read until the test delivers it (`held.ts`), with the list as
 * it was when the read was made.
 */

import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
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

type Args = Record<string, unknown>;

/** A node id; the status bar shows the first seven characters. */
const node = (n: number): string => n.toString(16).padStart(7, "0").padEnd(64, "0");

/** Side A, the head the comparison starts from. */
const A = node(1);
/** Side B: the same voice track, louder, and a second track. */
const B = node(2);
/** An edit the assistant makes from A: the voice track, quieter. */
const EDITED = node(3);

const headLabel = (id: string): string => `head ${id.slice(0, 7)}`;

const VOICE_ID = "3f2b8c1e-7d4a-4e9b-9c6f-1a2b3c4d5e6f";
const MUSIC_ID = "9a8b7c6d-5e4f-4a3b-8c2d-1e0f9a8b7c6d";

interface Spec {
  id: string;
  name: string;
  gain_db: number;
}

/** The tracks each node holds. Voice is the same track in all three. */
const TRACKS: Record<string, Spec[]> = {
  [A]: [{ id: VOICE_ID, name: "Voice", gain_db: 0 }],
  [B]: [
    { id: VOICE_ID, name: "Voice", gain_db: 6 },
    { id: MUSIC_ID, name: "Music", gain_db: 0 },
  ],
  [EDITED]: [{ id: VOICE_ID, name: "Voice", gain_db: -6 }],
};

/** A track list read the backend has been asked for and has not answered. */
interface Read {
  /** The list as it was when the read was made. */
  snapshot: unknown;
  reply: Held<unknown>;
}

class Backend {
  calls = 0;
  head = A;
  /** Answer a track-list read when the test delivers it, not at once. */
  holdReads = false;
  /** Every command the app sent, in order. */
  readonly sent: { cmd: string; args: Args }[] = [];
  private reads: Read[] = [];

  /** The reads waiting for an answer, oldest first. */
  get pendingReads(): readonly Read[] {
    return this.reads;
  }

  /** How many times the app has asked for `cmd`. */
  count(cmd: string): number {
    return this.sent.filter((s) => s.cmd === cmd).length;
  }

  /** The IPC boundary. */
  handle = (cmd: string, args: Args = {}): unknown => {
    this.calls++;
    this.sent.push({ cmd, args });
    switch (cmd) {
      case "list_tracks": {
        const snapshot = this.tracks();
        if (!this.holdReads) return snapshot;
        const read: Read = { snapshot, reply: held<unknown>() };
        this.reads.push(read);
        return read.reply.promise;
      }
      case "prepare_compare":
        return {
          a_path: "/tmp/a.wav",
          b_path: "/tmp/b.wav",
          a_warnings: [],
          b_warnings: [],
          compare_id: 1,
        };
      case "end_compare":
        return null;
      case "accept_b":
        // What the command does: the session head becomes `b`, and it is
        // the answer.
        this.head = args.b as string;
        return this.head;
      case "get_node":
        return { id: args.id, parent: args.id === A ? null : A };
      case "get_session_head":
        return this.head;
      case "get_graph":
        return {
          nodes: [
            { id: A, parent: null, label: "load", tool: "load", created_at: at(0) },
            { id: B, parent: A, label: "mix", tool: "set_track_gain", created_at: at(1) },
          ],
          head: this.head,
        };
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
      case "save_head_trail":
      case "set_selection_context":
        return null;
      default:
        return undefined;
    }
  };

  /** The track list as the session holds it now. */
  tracks() {
    return TRACKS[this.head].map((t) => ({
      id: t.id,
      name: t.name,
      gain_db: t.gain_db,
      pan: 0,
      muted: false,
      soloed: false,
      audio_path: "/tmp/voice.wav",
      clips: [
        { start_sec: 0, length_sec: 10, source_path: "/tmp/voice.wav", volume_envelope: [] },
      ],
    }));
  }

  /** Deliver a held read: the list as it was when it was asked for. */
  async deliver(read: Read): Promise<void> {
    this.reads = this.reads.filter((r) => r !== read);
    await read.reply.resolve(read.snapshot);
  }

  /** Deliver every held read, oldest first, until none is left. */
  async deliverAll(): Promise<void> {
    for (let guard = 0; guard < 20; guard++) {
      await settle();
      if (this.reads.length === 0) return;
      await this.deliver(this.reads[0]);
    }
    throw new Error("the app kept asking for the tracks");
  }
}

const at = (n: number) => `2026-10-22T00:0${n}:00Z`;

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

async function start(): Promise<Backend> {
  const backend = new Backend();
  mockIPC((cmd, args) => backend.handle(cmd, args as Args | undefined), {
    shouldMockEvents: true,
  });
  const { rerender } = render(<App />);
  await screen.findByTestId("timeline-lane-gain");
  await waitFor(() =>
    expect(screen.getByTestId("status-bar-head")).toHaveTextContent(headLabel(A)),
  );
  await quiet(backend);
  // A render inside `act` pays all the effects boot's answers still owe the
  // timeline before the first input (see `App.laneControls.test.tsx`).
  await act(async () => {
    rerender(<App />);
  });
  await quiet(backend);
  return backend;
}

const slider = () => screen.getAllByTestId("timeline-lane-gain")[0] as HTMLInputElement;
/** The first lane's gain readout. */
const readout = () => screen.getAllByTestId("timeline-lane-gain-readout")[0];
const laneNames = () => screen.getAllByTestId("timeline-lane-name").map((n) => n.textContent);

/** Move the first lane's gain the way an arrow key does, and do not let go. */
const move = (to: number) => fireEvent.change(slider(), { target: { value: String(to) } });

/** A bubble of the history graph. */
const bubble = (id: string): HTMLElement => {
  const found = screen
    .getAllByTestId("graph-bubble")
    .find((b) => b.getAttribute("data-node-id") === id);
  if (!found) throw new Error(`no bubble for ${id}`);
  return found;
};

/** Open an A/B compare of the head (A) with B, from the history graph. */
async function openCompare(backend: Backend): Promise<void> {
  fireEvent.click(screen.getByTestId("tab-graph"));
  await waitFor(() => bubble(B));
  fireEvent.contextMenu(bubble(B));
  fireEvent.click(
    within(screen.getByTestId("graph-context-menu")).getByRole("menuitem", {
      name: "Compare with…",
    }),
  );
  await screen.findByTestId("ab-compare-bar");
  await waitFor(() => expect(screen.getByTestId("ab-accept-b")).toBeEnabled());
  expect(backend.count("prepare_compare")).toBe(1);
  // The comparison plays on the timeline.
  fireEvent.click(screen.getByTestId("tab-timeline"));
  await screen.findByTestId("timeline-lane-gain");
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
  vi.spyOn(window, "prompt").mockReturnValue(null);
});

afterEach(() => {
  cleanup();
  clearMocks();
  vi.restoreAllMocks();
});

describe("Accept B of an A/B compare", () => {
  it("reads the tracks again, and the lanes show side B's", async () => {
    const backend = await start();
    expect(laneNames()).toEqual(["Voice"]);
    await openCompare(backend);
    const before = backend.count("list_tracks");

    fireEvent.click(screen.getByTestId("ab-accept-b"));
    await waitFor(() =>
      expect(screen.getByTestId("status-bar-head")).toHaveTextContent(headLabel(B)),
    );
    await backend.deliverAll();

    expect(backend.head).toBe(B);
    expect(backend.count("list_tracks")).toBeGreaterThan(before);
    await waitFor(() => expect(laneNames()).toEqual(["Voice", "Music"]));
    expect(readout()).toHaveTextContent("+6.0 dB");
    // The comparison is over.
    expect(screen.queryByTestId("ab-compare-bar")).not.toBeInTheDocument();
  });

  it("does not leave a control the user held before it over side B's tracks", async () => {
    const backend = await start();
    await openCompare(backend);

    // The slider is moved on, and not let go of: nothing is committed.
    move(1);
    expect(readout()).toHaveTextContent("+1.0 dB");

    fireEvent.click(screen.getByTestId("ab-accept-b"));
    await waitFor(() =>
      expect(screen.getByTestId("status-bar-head")).toHaveTextContent(headLabel(B)),
    );
    await backend.deliverAll();

    // B's voice track is 6 dB: that is what the session holds, not the 1 dB
    // that was set against side A.
    expect(backend.tracks()[0].gain_db).toBe(6);
    await waitFor(() => expect(readout()).toHaveTextContent("+6.0 dB"));
    expect(Number(slider().value)).toBe(6);
    expect(backend.count("set_track_gain")).toBe(0);
  });

  it("draws the list read after it, whenever a read made before it lands", async () => {
    const backend = await start();
    await openCompare(backend);

    // A read of side A's tracks is on its way: here, the read-back of an
    // edit made just before the accept.
    backend.holdReads = true;
    move(1);
    fireEvent.keyUp(slider(), { key: "ArrowRight" });
    await settle();
    expect(backend.pendingReads).toHaveLength(1);
    const older = backend.pendingReads[0];

    fireEvent.click(screen.getByTestId("ab-accept-b"));
    await waitFor(() =>
      expect(screen.getByTestId("status-bar-head")).toHaveTextContent(headLabel(B)),
    );
    await settle();
    // The accept asked for the list again, after the one on its way.
    await waitFor(() => expect(backend.pendingReads.length).toBeGreaterThanOrEqual(2));
    const newer = backend.pendingReads[backend.pendingReads.length - 1];
    expect(newer).not.toBe(older);

    // The newer answer lands first, then the older one: side B's tracks
    // stay.
    await backend.deliver(newer);
    await waitFor(() => expect(laneNames()).toEqual(["Voice", "Music"]));
    await backend.deliver(older);
    await backend.deliverAll();

    expect(laneNames()).toEqual(["Voice", "Music"]);
    expect(readout()).toHaveTextContent("+6.0 dB");
  });

  it("leaves the lanes the user's again once it is done", async () => {
    const backend = await start();
    await openCompare(backend);
    fireEvent.click(screen.getByTestId("ab-accept-b"));
    await waitFor(() =>
      expect(screen.getByTestId("status-bar-head")).toHaveTextContent(headLabel(B)),
    );
    await backend.deliverAll();
    await waitFor(() => expect(laneNames()).toEqual(["Voice", "Music"]));

    // A control set after the move is held and committed like any other.
    move(2);
    expect(readout()).toHaveTextContent("+2.0 dB");
    fireEvent.keyUp(slider(), { key: "ArrowRight" });
    await backend.deliverAll();
    await waitFor(() => expect(backend.count("set_track_gain")).toBe(1));
    expect(backend.sent.find((s) => s.cmd === "set_track_gain")?.args).toMatchObject({
      track: 0,
      gainDb: 2,
    });
  });
});

describe("a node the assistant creates", () => {
  /** The assistant edits the session: a new head, and the node's event. */
  async function assistantEdits(backend: Backend, id: string): Promise<void> {
    backend.head = id;
    await act(async () => {
      await emit("agent://node-created", { node_id: id });
    });
    await waitFor(() =>
      expect(screen.getByTestId("status-bar-head")).toHaveTextContent(headLabel(id)),
    );
  }

  it("takes a control the user set and has not let go of back to the session's value", async () => {
    const backend = await start();
    move(1);
    expect(readout()).toHaveTextContent("+1.0 dB");

    await assistantEdits(backend, EDITED);
    await backend.deliverAll();

    // The assistant's edit, not the 1 dB that was set against the head
    // before it.
    await waitFor(() => expect(readout()).toHaveTextContent("-6.0 dB"));
    expect(Number(slider().value)).toBe(-6);
    expect(backend.count("set_track_gain")).toBe(0);
  });

  it("shows no stale value while the tracks it asked for are on their way", async () => {
    const backend = await start();
    move(1);
    backend.holdReads = true;

    await assistantEdits(backend, EDITED);
    // The read has not been answered. The user's 1 dB belonged to the head
    // before the move and is not drawn over anything.
    expect(backend.pendingReads).toHaveLength(1);
    expect(readout()).not.toHaveTextContent("+1.0 dB");

    await backend.deliverAll();
    await waitFor(() => expect(readout()).toHaveTextContent("-6.0 dB"));
  });

  it("leaves the control the user's again for what they set after it", async () => {
    const backend = await start();
    move(1);
    await assistantEdits(backend, EDITED);
    await backend.deliverAll();
    await waitFor(() => expect(readout()).toHaveTextContent("-6.0 dB"));

    move(2);
    expect(readout()).toHaveTextContent("+2.0 dB");
    fireEvent.keyUp(slider(), { key: "ArrowRight" });
    await backend.deliverAll();

    await waitFor(() => expect(backend.count("set_track_gain")).toBe(1));
    expect(backend.sent.find((s) => s.cmd === "set_track_gain")?.args).toMatchObject({
      track: 0,
      gainDb: 2,
    });
  });
});
