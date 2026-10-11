/**
 * A marker added from the ruler does not depend on `window.prompt` (#546).
 *
 * The ruler used to ask for the marker's name with `window.prompt`. A
 * macOS webview answers that with `null` (neither wry nor
 * tauri-plugin-dialog provides a prompt panel), so there a click on the
 * ruler did nothing: no marker, no error, nothing on screen.
 *
 * The real `App` runs here with only the IPC boundary replaced, and
 * `window.prompt` stubbed to answer as that webview does. The backend
 * keeps the markers it is given, as `add_marker` does, and answers
 * `list_markers` from them.
 */

import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
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

const node = (n: number): string => n.toString(16).padStart(7, "0").padEnd(64, "0");
const headLabel = (n: number): string => `head ${node(n).slice(0, 7)}`;

/** The ruler's tick area, as `Ruler.test.tsx` lays it out: past the 132px spacer. */
const SIDEBAR = 132;
const TICKS = 400;

class Backend {
  head = node(1);
  private nodes = 1;
  /** The labels `add_marker` has recorded, as `list_markers` serialises them. */
  readonly markers: { id: string; name: string; kind: "marker"; time_sec: number }[] = [];
  /** Every command received, in order. */
  readonly sent: { cmd: string; args: Args }[] = [];

  handle = (cmd: string, args: Args = {}): unknown => {
    this.sent.push({ cmd, args });
    switch (cmd) {
      case "add_marker": {
        this.markers.push({
          id: `1a2b3c4d-5e6f-4a7b-8c9d-${String(this.markers.length).padStart(12, "0")}`,
          name: args.name as string,
          kind: "marker",
          time_sec: args.time as number,
        });
        this.head = node(++this.nodes);
        return this.head;
      }
      case "list_markers":
        return this.markers.map((m) => ({ ...m }));
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
      case "get_view_state":
        return {};
      case "get_plan_first":
      case "get_sync_lock":
        return false;
      case "get_session_head":
        return this.head;
      case "get_node":
        return { id: args.id, parent: null };
      case "get_head_trail":
        return { head: null, trail: [], redo: [] };
      case "save_view_state":
      case "set_selection_context":
        return null;
      case "list_tracks":
        return [
          {
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
          },
        ];
      default:
        return undefined;
    }
  };

  /** The `add_marker` calls received. */
  adds(): Args[] {
    return this.sent.filter((s) => s.cmd === "add_marker").map((s) => s.args);
  }
}

const settle = () =>
  act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });

async function open(backend: Backend) {
  mockIPC((cmd, args) => backend.handle(cmd, args as Args | undefined), {
    shouldMockEvents: true,
  });
  render(<App />);
  await screen.findByTestId("timeline-lane-mute");
  await waitFor(() =>
    expect(screen.getByTestId("status-bar-head")).toHaveTextContent(headLabel(1)),
  );
  await settle();
}

/** Give the ruler's tick area, and only it, a real box; jsdom reports zeroes. */
function layOutRuler() {
  const real = HTMLElement.prototype.getBoundingClientRect;
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (
    this: HTMLElement,
  ) {
    if (this.parentElement?.dataset.testid === "ruler" && this.previousElementSibling) {
      return {
        left: SIDEBAR,
        top: 0,
        width: TICKS,
        height: 20,
        right: SIDEBAR + TICKS,
        bottom: 20,
        x: SIDEBAR,
        y: 0,
        toJSON: () => ({}),
      } as DOMRect;
    }
    return real.call(this);
  });
}

/** Click the ruler `fraction` of the way across its ticks, as a user does. */
function clickRuler(fraction: number) {
  fireEvent.click(screen.getByTestId("ruler"), { clientX: SIDEBAR + TICKS * fraction });
}

let prompt: ReturnType<typeof vi.spyOn>;

beforeEach(() => {
  // What a macOS webview answers: no prompt is shown, and `null` comes back.
  prompt = vi.spyOn(window, "prompt").mockReturnValue(null);
  layOutRuler();
});

afterEach(() => {
  cleanup();
  clearMocks();
  vi.restoreAllMocks();
});

/** The box a new label's name is typed into, in the lane. */
const nameBox = () => screen.queryByTestId("label-draft-input") as HTMLInputElement | null;

/** Type `name` into the open box and press Enter, as a user does. */
function nameIt(name: string) {
  const box = screen.getByTestId("label-draft-input");
  fireEvent.change(box, { target: { value: name } });
  fireEvent.keyDown(box, { key: "Enter" });
}

describe("adding a marker from the ruler, in a webview that cannot prompt", () => {
  it("opens the lane's name box where the ruler was clicked, and adds nothing yet", async () => {
    const backend = new Backend();
    await open(backend);

    clickRuler(0.5);
    await settle();

    // The box is the one the lane's double-click opens, at the same point
    // on the same axis: half way across ten seconds.
    expect(nameBox()).toBeInTheDocument();
    expect(screen.getByTestId("label-draft")).toHaveStyle({ left: "50%" });
    expect(nameBox()).toHaveFocus();
    expect(backend.adds()).toEqual([]);
    expect(prompt).not.toHaveBeenCalled();
  });

  it("adds the marker that is named, at the point clicked, as one add_marker", async () => {
    const backend = new Backend();
    await open(backend);

    clickRuler(0.5);
    await settle();
    nameIt("  verse ");
    await settle();

    expect(backend.adds()).toEqual([{ time: expect.closeTo(5, 5), name: "verse" }]);
    // Its node is the head the app works from, and the lane shows it.
    await waitFor(() =>
      expect(screen.getByTestId("status-bar-head")).toHaveTextContent(headLabel(2)),
    );
    expect(await screen.findByTestId("label-chip")).toHaveAttribute("data-label-name", "verse");
    expect(nameBox()).not.toBeInTheDocument();
    expect(prompt).not.toHaveBeenCalled();
  });

  it("adds nothing when the name is left empty, or Escape is pressed", async () => {
    const backend = new Backend();
    await open(backend);

    clickRuler(0.25);
    await settle();
    nameIt("   ");
    await settle();
    expect(nameBox()).not.toBeInTheDocument();

    clickRuler(0.75);
    await settle();
    fireEvent.change(screen.getByTestId("label-draft-input"), { target: { value: "verse" } });
    fireEvent.keyDown(screen.getByTestId("label-draft-input"), { key: "Escape" });
    await settle();
    expect(nameBox()).not.toBeInTheDocument();

    expect(backend.adds()).toEqual([]);
    expect(prompt).not.toHaveBeenCalled();
  });

  it("opens the box again at the new point when the ruler is clicked a second time", async () => {
    const backend = new Backend();
    await open(backend);

    clickRuler(0.2);
    await settle();
    expect(screen.getByTestId("label-draft")).toHaveStyle({ left: "20%" });

    // Without a name typed, so the first box adds nothing as it closes.
    clickRuler(0.8);
    await settle();
    expect(screen.getAllByTestId("label-draft-input")).toHaveLength(1);
    expect(screen.getByTestId("label-draft")).toHaveStyle({ left: "80%" });
    nameIt("outro");
    await settle();

    expect(backend.adds()).toEqual([{ time: expect.closeTo(8, 5), name: "outro" }]);
  });

  it("opens the box for a click at the same point again after one was finished", async () => {
    const backend = new Backend();
    await open(backend);

    clickRuler(0.5);
    await settle();
    nameIt("verse");
    await settle();
    expect(nameBox()).not.toBeInTheDocument();

    // The same time as before: a request that compared equal to the last
    // one would be taken for nothing new, and the click would do nothing.
    clickRuler(0.5);
    await settle();
    expect(nameBox()).toBeInTheDocument();
  });

  it("does not open a box again when the lane is mounted afresh", async () => {
    const backend = new Backend();
    await open(backend);

    clickRuler(0.5);
    await settle();
    fireEvent.keyDown(screen.getByTestId("label-draft-input"), { key: "Escape" });
    await settle();

    // Away to the transcript and back unmounts the timeline and its lane.
    fireEvent.click(screen.getByTestId("tab-transcript"));
    await settle();
    fireEvent.click(screen.getByTestId("tab-timeline"));
    await screen.findByTestId("label-lane");
    await settle();

    expect(nameBox()).not.toBeInTheDocument();
    expect(backend.adds()).toEqual([]);
  });
});
