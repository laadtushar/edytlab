/**
 * A lane control the user has set is not undone by a track list that was
 * already on its way (#596).
 *
 * The gain and pan sliders show a new value at once and commit it when the
 * key or the pointer comes up; mute and solo commit on the click. Every
 * commit is followed by a read of the track list, and the timeline puts its
 * lanes back to whatever list it is handed. A read that was requested
 * before the user's next input, and lands after it, carries the older
 * value, so the control jumped back: a flicker, or, when the jump came
 * before key-up, a tap lost, because the key-up then committed the value
 * the session already held and `commitSlider` sent nothing.
 *
 * The real `App` runs here with only the IPC boundary replaced. The
 * backend holds the answer to a track-list read until the test delivers it
 * (`held.ts`), with the list as it was when the read was made: delivering
 * it between an input and its key-up is the order the bug needs.
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

/** A node id; the status bar shows the first seven characters. */
const node = (n: number): string => n.toString(16).padStart(7, "0").padEnd(64, "0");

interface Mixer {
  gain_db: number;
  pan: number;
  muted: boolean;
  soloed: boolean;
}

const FLAT: Mixer = { gain_db: 0, pan: 0, muted: false, soloed: false };

/** A track list read the backend has been asked for and has not answered. */
interface Read {
  /** The list as it was when the read was made. */
  snapshot: unknown;
  reply: Held<unknown>;
}

/** An edit the backend has been asked for and has not answered. */
interface Edit {
  cmd: string;
  args: Args;
  reply: Held<unknown>;
  /** What the backend does to the session when it takes the edit. */
  apply: () => string;
}

/** The mixer commands: the field each one sets. */
const SETS: Record<string, (args: Args) => Partial<Mixer>> = {
  set_track_gain: (a) => ({ gain_db: a.gainDb as number }),
  set_track_pan: (a) => ({ pan: a.pan as number }),
  set_track_muted: (a) => ({ muted: a.muted as boolean }),
  set_track_soloed: (a) => ({ soloed: a.soloed as boolean }),
};

class Backend {
  calls = 0;
  /** Every mixer edit received, in the order it arrived. */
  readonly sent: { cmd: string; args: Args }[] = [];
  head = node(1);
  /** Answer a track-list read when the test delivers it, not at once. */
  holdReads = false;
  /** Answer a mixer edit when the test says so, not at once. */
  holdEdits = false;
  private readonly states = new Map<string, Mixer>();
  private nodes = 1;
  private trackId = "3f2b8c1e-7d4a-4e9b-9c6f-1a2b3c4d5e6f";
  private reads: Read[] = [];
  private edits: Edit[] = [];

  constructor(start: Mixer = FLAT) {
    this.states.set(this.head, start);
  }

  /** What the session holds now. */
  get mixer(): Mixer {
    return this.states.get(this.head)!;
  }

  /** The values of every `cmd` edit received, by the field it sets. */
  values(cmd: string, field: string): unknown[] {
    return this.sent.filter((s) => s.cmd === cmd).map((s) => s.args[field]);
  }

  /** The reads waiting for an answer, oldest first. */
  get pendingReads(): readonly Read[] {
    return this.reads;
  }

  /** The edits waiting for an answer, oldest first. */
  get pendingEdits(): readonly Edit[] {
    return this.edits;
  }

  /** The IPC boundary. */
  handle = (cmd: string, args: Args = {}): unknown => {
    this.calls++;
    if (cmd in SETS) return this.edit(cmd, args);
    switch (cmd) {
      case "list_tracks": {
        const snapshot = [this.track()];
        if (!this.holdReads) return snapshot;
        const read: Read = { snapshot, reply: held<unknown>() };
        this.reads.push(read);
        return read.reply.promise;
      }
      case "set_head_to":
        this.head = args.nodeId as string;
        return this.head;
      case "get_node": {
        const n = parseInt((args.id as string).slice(0, 7), 16);
        return { id: args.id, parent: n > 1 ? node(n - 1) : null };
      }
      case "get_session_head":
        return this.head;
      case "get_graph":
        return {
          nodes: [...this.states.keys()].map((id, i) => ({
            id,
            parent: i === 0 ? null : node(i),
            label: `step ${i + 1}`,
            tool: "set_track_gain",
            created_at: new Date(2026, 0, 1, 0, i).toISOString(),
          })),
          head: this.head,
        };
      case "open_project": {
        // Another project: its own history, and a track that has the same
        // id as this one's, as a copy of the folder does.
        this.states.clear();
        this.nodes = 100;
        this.head = node(100);
        this.states.set(this.head, { ...FLAT, gain_db: 6 });
        return { path: args.path, head: this.head };
      }
      case "plugin:dialog|open":
        return "/tmp/other-project";
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

  private track() {
    return {
      id: this.trackId,
      name: "Voice",
      ...this.mixer,
      audio_path: "/tmp/voice.wav",
      clips: [
        { start_sec: 0, length_sec: 10, source_path: "/tmp/voice.wav", volume_envelope: [] },
      ],
    };
  }

  private edit(cmd: string, args: Args): unknown {
    this.sent.push({ cmd, args });
    const apply = () => {
      const id = node(++this.nodes);
      this.states.set(id, { ...this.mixer, ...SETS[cmd](args) });
      this.head = id;
      return id;
    };
    if (!this.holdEdits) return apply();
    const edit: Edit = { cmd, args, reply: held<unknown>(), apply };
    this.edits.push(edit);
    return edit.reply.promise;
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

  /** The session takes a held edit, and answers with the node it made. */
  async answer(edit: Edit): Promise<void> {
    this.edits = this.edits.filter((e) => e !== edit);
    await edit.reply.resolve(edit.apply());
  }

  /** The session refuses a held edit and changes nothing. */
  async refuse(edit: Edit, message: string): Promise<void> {
    this.edits = this.edits.filter((e) => e !== edit);
    await edit.reply.reject(message);
  }
}

/** Let what the app does next (sending the next edit, refreshing) happen. */
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

async function start(mixer: Mixer = FLAT): Promise<Backend> {
  const backend = new Backend(mixer);
  mockIPC((cmd, args) => backend.handle(cmd, args as Args | undefined), {
    shouldMockEvents: true,
  });
  const { rerender } = render(<App />);
  await screen.findByTestId("timeline-lane-gain");
  await waitFor(() =>
    expect(screen.getByTestId("status-bar-head")).toHaveTextContent(
      `head ${backend.head.slice(0, 7)}`,
    ),
  );
  await quiet(backend);
  // The answers to boot's reads landed outside `act`, so React may still
  // owe the timeline an effect that puts its lanes back to the session's
  // values: queued behind a scheduler that can be late, it would run at
  // the first input and undo it. A render inside `act` pays all that is
  // owed before it starts.
  await act(async () => {
    rerender(<App />);
  });
  await quiet(backend);
  return backend;
}

/**
 * What an element shows, each time it changes, without repeats: the
 * control "never shows the old value" when nothing it showed comes back.
 */
function watch(el: Element, read: () => string) {
  const seen = [read()];
  const observer = new MutationObserver(() => {
    const now = read();
    if (now !== seen[seen.length - 1]) seen.push(now);
  });
  observer.observe(el, {
    attributes: true,
    childList: true,
    characterData: true,
    subtree: true,
  });
  return {
    seen,
    /** Nothing was shown twice: the control never went back. */
    get neverWentBack() {
      return new Set(seen).size === seen.length;
    },
    stop: () => observer.disconnect(),
  };
}

const slider = (id: "gain" | "pan") =>
  screen.getByTestId(`timeline-lane-${id}`) as HTMLInputElement;
const readout = (id: "gain" | "pan") => screen.getByTestId(`timeline-lane-${id}-readout`);
const shown = (id: "gain" | "pan") => () => readout(id).textContent ?? "";

/** Move a slider the way an arrow key does: the value changes. */
const move = (id: "gain" | "pan", to: number) =>
  fireEvent.change(slider(id), { target: { value: String(to) } });

/** Let go of the key: what commits a slider. */
const keyUp = (id: "gain" | "pan") => fireEvent.keyUp(slider(id), { key: "ArrowRight" });

beforeEach(() => {
  vi.spyOn(window, "prompt").mockReturnValue(null);
});

afterEach(() => {
  cleanup();
  clearMocks();
  vi.restoreAllMocks();
});

describe.each([
  {
    id: "gain" as const,
    cmd: "set_track_gain",
    field: "gainDb",
    steps: [0.5, 1, 1.5, 2],
    inSession: (b: Backend) => b.mixer.gain_db,
  },
  {
    id: "pan" as const,
    cmd: "set_track_pan",
    field: "pan",
    steps: [0.02, 0.04, 0.06, 0.08],
    inSession: (b: Backend) => b.mixer.pan,
  },
])("the $id slider, when the read-back of the commit before it lands", (c) => {
  it("is never shown the older value, and its key-up still commits", async () => {
    const backend = await start();
    const display = watch(readout(c.id), shown(c.id));
    backend.holdReads = true;

    // The first tap, committed. Its read-back is on its way.
    move(c.id, c.steps[0]);
    keyUp(c.id);
    await settle();
    expect(backend.values(c.cmd, c.field)).toEqual([c.steps[0]]);
    expect(backend.pendingReads).toHaveLength(1);

    // The second tap's input, and then the first one's read-back: the list
    // as it was when that tap committed.
    move(c.id, c.steps[1]);
    await backend.deliver(backend.pendingReads[0]);

    expect(Number(slider(c.id).value)).toBeCloseTo(c.steps[1], 5);
    keyUp(c.id);
    await settle();

    // The key-up carried the new value, so it went out.
    expect(backend.values(c.cmd, c.field)).toEqual([c.steps[0], c.steps[1]]);

    await backend.deliverAll();
    expect(c.inSession(backend)).toBeCloseTo(c.steps[1], 5);
    expect(Number(slider(c.id).value)).toBeCloseTo(c.steps[1], 5);
    expect(display.seen, "what the readout showed, in order").toHaveLength(3);
    expect(display.neverWentBack).toBe(true);
    display.stop();
  });

  it("ends four taps on the fourth, whenever each read-back lands", async () => {
    const backend = await start();
    const display = watch(readout(c.id), shown(c.id));
    backend.holdReads = true;

    for (const to of c.steps) {
      move(c.id, to);
      // The previous tap's read-back lands after this tap's input and
      // before its key-up.
      const earlier = backend.pendingReads[0];
      if (earlier) await backend.deliver(earlier);
      keyUp(c.id);
      await settle();
    }
    await backend.deliverAll();

    expect(backend.values(c.cmd, c.field)).toEqual(c.steps);
    expect(c.inSession(backend)).toBeCloseTo(c.steps[3], 5);
    expect(Number(slider(c.id).value)).toBeCloseTo(c.steps[3], 5);
    expect(display.seen).toHaveLength(c.steps.length + 1);
    expect(display.neverWentBack).toBe(true);
    display.stop();
  });
});

describe.each([
  { what: "mute", id: "timeline-lane-mute", cmd: "set_track_muted", field: "muted" as const },
  { what: "solo", id: "timeline-lane-solo", cmd: "set_track_soloed", field: "soloed" as const },
])("the $what button, when a track list read before the click lands after it", (c) => {
  it("stays pressed", async () => {
    const backend = await start();
    backend.holdReads = true;

    // A read of the track list on its way: here, the read-back of an edit
    // made just before.
    move("gain", 0.5);
    keyUp("gain");
    await settle();
    expect(backend.pendingReads).toHaveLength(1);

    const button = screen.getByTestId(c.id);
    const pressed = watch(button, () => button.getAttribute("aria-pressed") ?? "");
    backend.holdEdits = true;
    fireEvent.click(button);
    expect(button).toHaveAttribute("aria-pressed", "true");

    // The older list lands after the click, before the click's edit has
    // reached the session.
    await backend.deliver(backend.pendingReads[0]);
    expect(button).toHaveAttribute("aria-pressed", "true");

    // The edit reaches the session, and its own read-back follows.
    await settle();
    expect(backend.pendingEdits).toHaveLength(1);
    await backend.answer(backend.pendingEdits[0]);
    await backend.deliverAll();

    expect(backend.mixer[c.field]).toBe(true);
    expect(button).toHaveAttribute("aria-pressed", "true");
    expect(pressed.seen, "aria-pressed, in order").toEqual(["false", "true"]);
    pressed.stop();
  });
});

describe("a control the session did not take", () => {
  it("goes back to the session's value once the commit has been refused and the tracks read", async () => {
    const backend = await start();
    backend.holdEdits = true;

    move("gain", 3);
    keyUp("gain");
    await settle();
    expect(readout("gain")).toHaveTextContent("+3.0 dB");

    await backend.refuse(backend.pendingEdits[0], "gain must be between -60 and 24 dB");
    await backend.deliverAll();

    expect(screen.getByTestId("render-error")).toHaveTextContent("gain must be between");
    expect(backend.mixer.gain_db).toBe(0);
    // Not left under the user's value: this is what the session holds.
    expect(readout("gain")).toHaveTextContent("0.0 dB");
    expect(Number(slider("gain").value)).toBe(0);

    // And it is the session's control again, not the user's: the next
    // commit goes out and is shown.
    backend.holdEdits = false;
    move("gain", 1);
    keyUp("gain");
    await backend.deliverAll();
    expect(backend.values("set_track_gain", "gainDb")).toEqual([3, 1]);
    expect(readout("gain")).toHaveTextContent("+1.0 dB");
  });

  it("shows a change the session made itself once the user's own commit is confirmed", async () => {
    const backend = await start();
    move("gain", 1);
    keyUp("gain");
    await backend.deliverAll();
    expect(readout("gain")).toHaveTextContent("+1.0 dB");

    // The session moves on without the window (the agent set the gain);
    // the next read of the tracks carries it. What the user did earlier
    // must not hold the control on its old value.
    backend.handle("set_track_gain", { track: 0, gainDb: -6 });
    move("pan", 0.5);
    keyUp("pan");
    await backend.deliverAll();
    expect(readout("gain")).toHaveTextContent("-6.0 dB");
  });
});

describe("a control set but not yet committed, when the session moves by itself", () => {
  it("shows the session's value after an undo", async () => {
    const backend = await start();
    // A committed edit to undo.
    move("gain", 0.5);
    keyUp("gain");
    await backend.deliverAll();
    expect(readout("gain")).toHaveTextContent("+0.5 dB");

    // The slider is moved on, and not let go of: nothing is committed.
    move("gain", 1);
    expect(readout("gain")).toHaveTextContent("+1.0 dB");

    fireEvent.keyDown(window, { key: "z", ctrlKey: true });
    await waitFor(() => expect(backend.head).toBe(node(1)));
    await backend.deliverAll();

    expect(backend.mixer.gain_db).toBe(0);
    await waitFor(() => expect(readout("gain")).toHaveTextContent("0.0 dB"));
    expect(Number(slider("gain").value)).toBe(0);
  });

  it("shows the opened project's value after a project is opened", async () => {
    const backend = await start();
    move("gain", 1);
    expect(readout("gain")).toHaveTextContent("+1.0 dB");

    screen.getByTestId("open-project-button").click();
    await waitFor(() =>
      expect(screen.getByTestId("status-bar-head")).toHaveTextContent(
        `head ${node(100).slice(0, 7)}`,
      ),
    );
    await backend.deliverAll();

    await waitFor(() => expect(readout("gain")).toHaveTextContent("+6.0 dB"));
    expect(Number(slider("gain").value)).toBe(6);
  });
});
