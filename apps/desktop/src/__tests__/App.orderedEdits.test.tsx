/**
 * Edits reach the session in the order the user made them (#498).
 *
 * Every command that changes the session is its own task in the backend,
 * and when a long tool has the store locked they all wait on it and
 * wake in no order anyone promised. The app used to fire them at once and
 * adopt whatever came back, so two fader moves could apply the wrong way
 * round (the older value landed last, and the refresh showed it) and two
 * replies could cross (the head went to a node older than the one it was
 * on, which `applyNewHead` treats as lost work).
 *
 * The real `App` runs here with only the IPC boundary replaced, by a
 * backend that holds each edit's answer until the test releases it
 * (`held.ts`). The test always answers the newest edit first, which is the
 * order the bug needs: if the app sends everything at once, that is the
 * order the answers cross in; if it sends one at a time, there is only
 * ever one to answer.
 *
 * The backend applies an edit at one of two moments, because the two
 * failures are different:
 *
 * - `answer`: when the test releases it. Releasing the newest first makes
 *   the backend apply edits the wrong way round, as a lock that wakes the
 *   wrong waiter does.
 * - `send`: when the command arrives. Edits apply in order, and only the
 *   replies cross.
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

type Args = Record<string, unknown>;

/**
 * A node id. The status bar shows the first seven characters, so each id
 * is told apart by them: `node(2)` reads `head 0000002`.
 */
const node = (n: number): string => n.toString(16).padStart(7, "0").padEnd(64, "0");
const headLabel = (n: number): string => `head ${node(n).slice(0, 7)}`;

/** An edit the backend has been asked for and has not answered. */
interface Edit {
  cmd: string;
  args: Args;
  reply: Held<unknown>;
  /** What the backend does to the session when it applies the edit. */
  apply: () => void;
  /** The node it created, once applied. */
  node: string | null;
}

/** The commands that change the session; every other command answers at once. */
const EDITS = new Set([
  "set_track_gain",
  "set_track_muted",
  "set_sync_lock",
  "add_marker",
  "update_marker",
  "remove_marker",
  "set_head_to",
  "batch_load",
]);

class Backend {
  /** How many commands of any kind have been received. */
  calls = 0;
  /** Every edit command received, in the order it arrived. */
  readonly sent: { cmd: string; args: Args }[] = [];
  /** What the backend did, in the order it did it. */
  readonly applied: string[] = [];
  head = node(1);
  gainDb = 0;
  muted = false;
  /** The labels in the lane: what `list_markers` answers. */
  labels: unknown[] = [];
  private nodes = 1;
  private waiting: Edit[] = [];

  constructor(private readonly appliesOn: "send" | "answer") {}

  /** The IPC boundary. */
  handle = (cmd: string, args: Args = {}): unknown => {
    this.calls++;
    if (EDITS.has(cmd)) return this.edit(cmd, args);
    switch (cmd) {
      case "has_api_key":
        return true;
      case "get_keychain_persistence":
        return { persistent: true, reason: null };
      case "install_bundled_skills":
        return 0;
      case "list_templates":
      case "list_recent_projects":
      case "get_transcript":
        return [];
      case "list_markers":
        return this.labels;
      case "get_view_state":
        return {};
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
      // The file picker, answered with one file.
      case "plugin:dialog|open":
        return ["/tmp/other.wav"];
      case "list_tracks":
        return [this.track()];
      // The history: the node the project began on, and the one after it.
      case "get_graph":
        return {
          nodes: [1, 2].map((n) => ({
            id: node(n),
            parent: n === 1 ? null : node(n - 1),
            label: `step ${n}`,
            tool: "set_track_gain",
            created_at: new Date(2026, 0, 1, 0, n).toISOString(),
          })),
          head: this.head,
        };
      default:
        return undefined;
    }
  };

  private track() {
    return {
      id: "3f2b8c1e-7d4a-4e9b-9c6f-1a2b3c4d5e6f",
      name: "Voice",
      muted: this.muted,
      gain_db: this.gainDb,
      pan: 0,
      soloed: false,
      audio_path: "/tmp/voice.wav",
      clips: [
        { start_sec: 0, length_sec: 10, source_path: "/tmp/voice.wav", volume_envelope: [] },
      ],
    };
  }

  private edit(cmd: string, args: Args): Promise<unknown> {
    const edit: Edit = {
      cmd,
      args,
      reply: held<unknown>(),
      node: null,
      apply: () => {
        if (cmd === "set_track_gain") this.gainDb = args.gainDb as number;
        if (cmd === "set_track_muted") this.muted = args.muted as boolean;
        if (cmd === "set_head_to") this.head = args.nodeId as string;
      },
    };
    this.sent.push({ cmd, args });
    this.waiting.push(edit);
    if (this.appliesOn === "send") this.run(edit);
    return edit.reply.promise;
  }

  private run(edit: Edit): void {
    edit.apply();
    this.applied.push(`${edit.cmd} ${JSON.stringify(edit.args)}`);
    if (edit.cmd === "set_head_to") {
      edit.node = edit.args.nodeId as string;
    } else {
      edit.node = node(++this.nodes);
      this.head = edit.node;
    }
  }

  /** The edits waiting for an answer, oldest first. */
  get inFlight(): readonly Edit[] {
    return this.waiting;
  }

  async answer(edit: Edit): Promise<void> {
    this.take(edit);
    if (this.appliesOn === "answer") this.run(edit);
    // `batch_load` answers with a report; every other edit with the head.
    await edit.reply.resolve(
      edit.cmd === "batch_load"
        ? { tracks_loaded: 1, last_node_id: edit.node, failures: [] }
        : edit.node,
    );
  }

  async refuse(edit: Edit, message: string): Promise<void> {
    this.take(edit);
    await edit.reply.reject(message);
  }

  private take(edit: Edit): void {
    this.waiting = this.waiting.filter((e) => e !== edit);
  }
}

/** Let what the app does next (sending the next edit, refreshing) happen. */
const settle = () =>
  act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });

/**
 * Wait until the app has stopped asking for things.
 *
 * Boot is not over when the first lane is drawn: the head arriving makes
 * the app read the transcript, the labels and the sync-lock, and each
 * answer re-renders it. The timeline puts its lanes back to what the
 * session holds whenever it is handed a new track list, which every
 * render of the app does, so a fader dragged just as one of those lands
 * is put back before it is let go of, and the drag is lost.
 */
async function quiet(backend: Backend): Promise<void> {
  for (let guard = 0; guard < 20; guard++) {
    const before = backend.calls;
    await settle();
    if (backend.calls === before) return;
  }
  throw new Error("the app never stopped asking");
}

/**
 * Answer the newest edit first, until nothing is waiting and nothing more
 * is sent: the order in which a backend that was racing would answer.
 */
async function answerNewestFirst(backend: Backend): Promise<void> {
  for (let guard = 0; guard < 20; guard++) {
    await settle();
    const open = backend.inFlight;
    if (open.length === 0) return;
    await backend.answer(open[open.length - 1]);
  }
  throw new Error("the app kept sending edits");
}

/** The heads the status bar showed, in order, without repeats. */
function watchHead() {
  const el = screen.getByTestId("status-bar-head");
  const seen = [el.textContent ?? ""];
  const observer = new MutationObserver(() => {
    const now = el.textContent ?? "";
    if (now !== seen[seen.length - 1]) seen.push(now);
  });
  observer.observe(el, { childList: true, characterData: true, subtree: true });
  return { seen, stop: () => observer.disconnect() };
}

/** Move the first fader to `db` and let go of it. */
function dragGain(db: number) {
  const fader = screen.getByTestId("timeline-lane-gain");
  fireEvent.change(fader, { target: { value: String(db) } });
  fireEvent.pointerUp(fader);
}

/** Add a label in the lane the way a user does: double-click, type a name, Enter. */
function typeLabel() {
  fireEvent.doubleClick(screen.getByTestId("label-lane-track"));
  const box = screen.getByTestId("label-draft-input");
  fireEvent.change(box, { target: { value: "verse" } });
  fireEvent.keyDown(box, { key: "Enter" });
}

const gainReadout = () => screen.getByTestId("timeline-lane-gain-readout");
const names = (backend: Backend) => backend.sent.map((s) => s.cmd);

async function start(appliesOn: "send" | "answer", labels: unknown[] = []): Promise<Backend> {
  const backend = new Backend(appliesOn);
  backend.labels = labels;
  mockIPC((cmd, args) => backend.handle(cmd, args as Args | undefined), {
    shouldMockEvents: true,
  });
  const { rerender } = render(<App />);
  await screen.findByTestId("timeline-lane-gain");
  await waitFor(() =>
    expect(screen.getByTestId("status-bar-head")).toHaveTextContent(headLabel(1)),
  );
  await quiet(backend);
  // The answers to boot's reads landed outside `act`, so React may still
  // owe the timeline an effect that puts its lanes back to the session's
  // values: queued behind a scheduler that can be late, it would run at
  // the first fader move and undo it. A render inside `act` pays all that
  // is owed before it starts, and what it queues is paid on the way out.
  await act(async () => {
    rerender(<App />);
  });
  await quiet(backend);
  return backend;
}

beforeEach(() => {
  // The ruler asks for a marker's name with a prompt; the lane does not.
  vi.spyOn(window, "prompt").mockReturnValue("verse");
});

afterEach(() => {
  // Unmount first: the app unlistens through the mocked IPC on its way out.
  cleanup();
  clearMocks();
  vi.restoreAllMocks();
});

describe("edits made in quick succession", () => {
  it("reach the session in the order they were made, so the last one wins", async () => {
    const backend = await start("answer");

    dragGain(-3);
    dragGain(-9);
    await answerNewestFirst(backend);

    expect(backend.sent).toEqual([
      { cmd: "set_track_gain", args: { track: 0, gainDb: -3 } },
      { cmd: "set_track_gain", args: { track: 0, gainDb: -9 } },
    ]);
    // The backend applied them in that order, so the older value did not
    // land last...
    expect(backend.applied.map((a) => /gainDb":(-?\d+)/.exec(a)?.[1])).toEqual(["-3", "-9"]);
    expect(backend.gainDb).toBe(-9);
    // ...and the fader shows what the session holds.
    await waitFor(() => expect(gainReadout()).toHaveTextContent("-9.0 dB"));
  });

  // The fader is compared with what the session holds so that a release,
  // a key-up and a blur carrying one value make one edit, not three. What
  // the session holds is what the edits ahead of this one leave it with,
  // not what the last refresh showed: 0 -> -3 -> 0 inside one window ends
  // on 0, and the 0 must be sent.
  it("keep the last value when the fader is put back where it started", async () => {
    const backend = await start("answer");

    dragGain(-3);
    dragGain(0);
    await answerNewestFirst(backend);

    expect(backend.sent.map((s) => s.args.gainDb)).toEqual([-3, 0]);
    expect(backend.gainDb).toBe(0);
    await waitFor(() => expect(gainReadout()).toHaveTextContent("0.0 dB"));
  });

  it("send a value once when release and blur both carry it", async () => {
    const backend = await start("answer");

    dragGain(-3);
    dragGain(-3);
    await answerNewestFirst(backend);

    expect(backend.sent.map((s) => s.args.gainDb)).toEqual([-3]);
    expect(backend.gainDb).toBe(-3);

    // Once the edit is over and the session shows the value, a later
    // commit of the same value is still not an edit...
    await waitFor(() => expect(gainReadout()).toHaveTextContent("-3.0 dB"));
    dragGain(-3);
    await answerNewestFirst(backend);
    expect(backend.sent).toHaveLength(1);

    // ...and one that differs is, so nothing is left remembered as pending.
    dragGain(-9);
    await answerNewestFirst(backend);
    expect(backend.sent.map((s) => s.args.gainDb)).toEqual([-3, -9]);
  });

  it("go out one at a time: the second waits for the first to be answered", async () => {
    const backend = await start("answer");

    dragGain(-3);
    dragGain(-9);
    await settle();

    expect(backend.sent).toEqual([{ cmd: "set_track_gain", args: { track: 0, gainDb: -3 } }]);

    await backend.answer(backend.inFlight[0]);
    await settle();

    expect(backend.sent[1]).toEqual({ cmd: "set_track_gain", args: { track: 0, gainDb: -9 } });
    await answerNewestFirst(backend);
  });

  it("never take the head back to a node it has already left", async () => {
    // Edits apply in order here; only the answers cross.
    const backend = await start("send");
    const heads = watchHead();

    dragGain(-3);
    dragGain(-9);
    await answerNewestFirst(backend);

    expect(backend.applied).toHaveLength(2);
    expect(heads.seen).toEqual([headLabel(1), headLabel(2), headLabel(3)]);
    expect(screen.getByTestId("status-bar-head")).toHaveTextContent(headLabel(3));
    await waitFor(() => expect(gainReadout()).toHaveTextContent("-9.0 dB"));
    heads.stop();
  });

  it("still run after one that failed, and the failure is shown", async () => {
    const backend = await start("answer");

    dragGain(-3);
    dragGain(-9);
    await settle();
    await backend.refuse(backend.inFlight[0], "gain must be between -60 and 24 dB");
    await answerNewestFirst(backend);

    expect(names(backend)).toEqual(["set_track_gain", "set_track_gain"]);
    expect(backend.sent[1].args).toEqual({ track: 0, gainDb: -9 });
    // Only the second was applied, and its head was adopted.
    expect(backend.gainDb).toBe(-9);
    expect(screen.getByTestId("status-bar-head")).toHaveTextContent(headLabel(2));
    expect(screen.getByTestId("render-error")).toHaveTextContent(
      "gain must be between -60 and 24 dB",
    );
    await waitFor(() => expect(gainReadout()).toHaveTextContent("-9.0 dB"));
  });

  // One line for every kind of edit, not one per kind: each of these goes
  // through a different handler, and each has to wait for the fader move
  // made before it.
  it.each([
    ["a label", "add_marker", typeLabel],
    ["a mute", "set_track_muted", () => fireEvent.click(screen.getByTestId("timeline-lane-mute"))],
    ["the sync-lock toggle", "set_sync_lock", () => fireEvent.click(screen.getByTestId("sync-lock-btn"))],
    ["a file opened from the toolbar", "batch_load", () => fireEvent.click(screen.getByTestId("open-audio-button"))],
  ])("%s waits for the edit made before it", async (_what, command, act_) => {
    const backend = await start("answer");

    dragGain(-3);
    await settle();
    act_();
    await settle();

    expect(names(backend)).toEqual(["set_track_gain"]);

    await answerNewestFirst(backend);

    expect(names(backend)).toEqual(["set_track_gain", command]);
  });

  // The other three gestures of the label lane (#203 §1). Each is a
  // command that changes the session, so each takes its turn in the same
  // line as the fader: sent after the edit made before it is answered,
  // and with the head it returns adopted.
  describe("in the label lane", () => {
    const LABEL = { id: "7c1d4a52-3b7e-4f0a-9d21-5e8a6b0c9f13", name: "intro", kind: "marker", time_sec: 2 };

    const chip = () => screen.findByTestId("label-chip-button");
    const rename = async () => {
      fireEvent.doubleClick(await chip());
      const box = screen.getByTestId("label-chip-input");
      fireEvent.change(box, { target: { value: "cold open" } });
      fireEvent.keyDown(box, { key: "Enter" });
    };
    const move = async () => {
      fireEvent.mouseDown(await chip(), { button: 0, clientX: 0 });
      fireEvent.mouseMove(window, { clientX: 5000 });
      fireEvent.mouseUp(window, { clientX: 5000 });
    };
    const remove = async () => fireEvent.contextMenu(await chip());

    it.each([
      ["renamed", "update_marker", { id: LABEL.id, name: "cold open" }, rename],
      ["moved", "update_marker", { id: LABEL.id, time: expect.any(Number) }, move],
      ["deleted", "remove_marker", { id: LABEL.id }, remove],
    ])("a label %s waits for the edit made before it", async (_what, command, args, gesture) => {
      const backend = await start("answer", [LABEL]);

      dragGain(-3);
      await settle();
      await gesture();
      await settle();

      expect(names(backend)).toEqual(["set_track_gain"]);

      await answerNewestFirst(backend);

      expect(backend.sent).toEqual([
        { cmd: "set_track_gain", args: { track: 0, gainDb: -3 } },
        { cmd: command, args },
      ]);
      // And its head is the one the app ends on.
      expect(screen.getByTestId("status-bar-head")).toHaveTextContent(headLabel(3));
    });
  });

  it("are undone from where they leave the head, not from where it was when Ctrl+Z was pressed", async () => {
    const backend = await start("answer");

    dragGain(-3);
    await settle();
    // Pressed while the edit has not been answered yet.
    fireEvent.keyDown(window, { key: "z", ctrlKey: true });
    await settle();
    expect(names(backend)).toEqual(["set_track_gain"]);

    await backend.answer(backend.inFlight[0]);
    await settle();

    // Undo goes back over the edit it was pressed after.
    expect(backend.sent[1]).toEqual({ cmd: "set_head_to", args: { nodeId: node(1) } });
    await answerNewestFirst(backend);
    await waitFor(() =>
      expect(screen.getByTestId("status-bar-head")).toHaveTextContent(headLabel(1)),
    );
  });
});

describe("a head move asked for while another is waiting", () => {
  // jsdom has no layout, and the history graph measures its nodes.
  beforeEach(() => {
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    );
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("is answered with a notice in the graph, not dropped without a word", async () => {
    const backend = await start("answer");

    // An edit still on its way, and an undo pressed behind it: the undo
    // is a head move that has not finished, and holds the app's one slot
    // for them until the edit has been answered.
    dragGain(-3);
    await settle();
    fireEvent.keyDown(window, { key: "z", ctrlKey: true });
    await settle();

    // "Set as head" on the first node, from the graph.
    fireEvent.click(screen.getByTestId("tab-graph"));
    const first = await waitFor(() => {
      const bubble = document.querySelector(`[data-node-id="${node(1)}"]`);
      expect(bubble).not.toBeNull();
      return bubble as Element;
    });
    fireEvent.contextMenu(first);
    fireEvent.click(await screen.findByRole("menuitem", { name: "Set as head" }));

    await waitFor(() =>
      expect(screen.getByTestId("graph-error")).toHaveTextContent(/still running/i),
    );
    // It was not sent: only the edit has reached the backend, and the
    // undo behind it has not.
    expect(names(backend)).toEqual(["set_track_gain"]);

    // The undo is not lost either.
    await answerNewestFirst(backend);
    expect(names(backend)).toEqual(["set_track_gain", "set_head_to"]);
  });
});
