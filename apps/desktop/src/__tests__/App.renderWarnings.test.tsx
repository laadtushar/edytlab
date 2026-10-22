/**
 * The window says what a render leaves out (#532).
 *
 * #531 made the renderer skip a stored effect it cannot run (a reverb in a
 * project saved by an older build, say) and report it, instead of failing
 * every render. The agent's tools pass the report on; the window showed
 * nothing, so Play and Export dropped the effect and said not a word.
 *
 * The real `App` runs here with only the IPC boundary replaced. The
 * backend answers `get_render_warnings` per node, as the real one does
 * (from the node's stored chains, no render), and the head moves the ways
 * it really moves: the agent's edit announces itself with
 * `agent://node-created`, undo calls `set_head_to` and announces nothing.
 * The notice must follow the node in both.
 */

import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
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
import type { RenderWarning } from "../lib/tauri-bridge";

type Args = Record<string, unknown>;

const node = (n: number): string => n.toString(16).padStart(7, "0").padEnd(64, "0");
const headLabel = (id: string): string => `head ${id.slice(0, 7)}`;

/** The state an older build saved: a reverb on the track, and one on the master. */
const WITH_REVERB = node(2);
/** The agent's `remove_effect` / `set_effect_bypassed`: the same project without them. */
const WITHOUT = node(3);

const reverbOnTrack: RenderWarning = {
  scope: { type: "track", index: 0, name: "Voice" },
  effect_index: 1,
  kind: "reverb",
  reason: "not_yet_streamable",
};
const reverbOnMaster: RenderWarning = {
  scope: { type: "master" },
  effect_index: 0,
  kind: "reverb",
  reason: "not_yet_streamable",
};

class Backend {
  head: string;
  /** What `get_render_warnings` answers, by node. */
  readonly warnings = new Map<string, RenderWarning[]>();
  /** How `get_render_warnings` is answered: from `warnings`, or not at all. */
  reads: "answer" | "fail" | "hang" = "answer";
  readonly sent: { cmd: string; args: Args }[] = [];

  constructor(head: string, warnings: Record<string, RenderWarning[]>) {
    this.head = head;
    for (const [id, list] of Object.entries(warnings)) this.warnings.set(id, list);
  }

  handle = (cmd: string, args: Args = {}): unknown => {
    this.sent.push({ cmd, args });
    switch (cmd) {
      case "get_render_warnings":
        if (this.reads === "fail") return Promise.reject("no session loaded; call open_project first");
        if (this.reads === "hang") return new Promise(() => undefined);
        return this.warnings.get(args.node as string) ?? [];
      case "set_head_to":
        this.head = args.nodeId as string;
        return this.head;
      case "get_node":
        return { id: args.id, parent: args.id === WITHOUT ? WITH_REVERB : null };
      case "get_session_head":
        return this.head;
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
        return [];
      case "get_transcript":
        return { words: [], source: { kind: "unknown" } };
      case "get_head_trail":
        return { head: null, trail: [], redo: [] };
      case "get_view_state":
        return {};
      case "get_plan_first":
      case "get_sync_lock":
        return false;
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

  /** The nodes `get_render_warnings` was asked about, in order. */
  asked(): string[] {
    return this.sent.filter((s) => s.cmd === "get_render_warnings").map((s) => s.args.node as string);
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
    expect(screen.getByTestId("status-bar-head")).toHaveTextContent(headLabel(backend.head)),
  );
  await settle();
}

afterEach(() => {
  cleanup();
  clearMocks();
});

describe("a stored effect the render leaves out", () => {
  it("is named on screen: the track, its name and the effect", async () => {
    const backend = new Backend(WITH_REVERB, { [WITH_REVERB]: [reverbOnTrack] });
    await open(backend);

    const notice = await screen.findByTestId("render-warnings");
    expect(notice).toHaveTextContent(
      'Reverb on track 0 "Voice" is left out: this build can\'t run it in a chain yet. ' +
        "Remove or bypass it to clear this.",
    );
    // A status, not an alert: nothing failed.
    expect(notice).toHaveAttribute("role", "status");
    expect(screen.queryByTestId("render-error")).not.toBeInTheDocument();
    expect(backend.asked()).toEqual([WITH_REVERB]);
  });

  it("is counted, with each one listed, when there are several", async () => {
    const backend = new Backend(WITH_REVERB, { [WITH_REVERB]: [reverbOnTrack, reverbOnMaster] });
    await open(backend);

    const notice = await screen.findByTestId("render-warnings");
    expect(notice).toHaveTextContent("2 effects are left out of the mix");
    const items = Array.from(
      screen.getByTestId("render-warnings-list").querySelectorAll("li"),
      (li) => li.textContent,
    );
    expect(items).toEqual([
      'Reverb on track 0 "Voice": this build can\'t run it in a chain yet.',
      "Reverb on the master bus: this build can't run it in a chain yet.",
    ]);
  });

  it("is still said once Play's preview render has run, and the render is not blocked by it", async () => {
    const backend = new Backend(WITH_REVERB, { [WITH_REVERB]: [reverbOnTrack] });
    await open(backend);
    await screen.findByTestId("render-warnings");

    fireEvent.click(screen.getByTestId("render-preview-button"));
    await waitFor(() =>
      expect(backend.sent.filter((s) => s.cmd === "render_preview")).toHaveLength(1),
    );
    await settle();

    expect(screen.getByTestId("render-warnings")).toHaveTextContent('Reverb on track 0 "Voice"');
    expect(screen.queryByTestId("render-error")).not.toBeInTheDocument();
  });

  it("clears when the agent removes or bypasses the effect", async () => {
    const backend = new Backend(WITH_REVERB, { [WITH_REVERB]: [reverbOnTrack] });
    await open(backend);
    await screen.findByTestId("render-warnings");

    // The edit lands as a new node, and announces itself.
    backend.head = WITHOUT;
    await act(async () => {
      await emit("agent://node-created", { node_id: WITHOUT });
    });

    await waitFor(() =>
      expect(screen.getByTestId("status-bar-head")).toHaveTextContent(headLabel(WITHOUT)),
    );
    await waitFor(() => expect(screen.queryByTestId("render-warnings")).not.toBeInTheDocument());
    expect(backend.asked()).toEqual([WITH_REVERB, WITHOUT]);
  });

  it("comes back on undo, which announces nothing", async () => {
    const backend = new Backend(WITH_REVERB, { [WITH_REVERB]: [reverbOnTrack] });
    await open(backend);
    backend.head = WITHOUT;
    await act(async () => {
      await emit("agent://node-created", { node_id: WITHOUT });
    });
    await waitFor(() => expect(screen.queryByTestId("render-warnings")).not.toBeInTheDocument());

    fireEvent.keyDown(window, { key: "z", ctrlKey: true });

    await waitFor(() =>
      expect(screen.getByTestId("status-bar-head")).toHaveTextContent(headLabel(WITH_REVERB)),
    );
    expect(await screen.findByTestId("render-warnings")).toHaveTextContent(
      'Reverb on track 0 "Voice"',
    );
  });

  it("is not shown for the old node while the new one is being read", async () => {
    const backend = new Backend(WITH_REVERB, { [WITH_REVERB]: [reverbOnTrack] });
    await open(backend);
    await screen.findByTestId("render-warnings");

    // The read for the next node never answers.
    backend.reads = "hang";
    backend.head = WITHOUT;
    await act(async () => {
      await emit("agent://node-created", { node_id: WITHOUT });
    });

    await waitFor(() => expect(screen.queryByTestId("render-warnings")).not.toBeInTheDocument());
  });
});

/** The app has asked for the node's warnings and had its answer. */
async function answered(backend: Backend, nodes: string[]) {
  await waitFor(() => expect(backend.asked()).toEqual(nodes));
  await settle();
}

describe("with nothing left out", () => {
  it("shows nothing new", async () => {
    const backend = new Backend(WITHOUT, {});
    await open(backend);
    await answered(backend, [WITHOUT]);

    expect(screen.queryByTestId("render-warnings")).not.toBeInTheDocument();
    expect(screen.queryByTestId("export-warnings")).not.toBeInTheDocument();
    expect(screen.queryByTestId("render-error")).not.toBeInTheDocument();
  });

  it("shows nothing when the read fails", async () => {
    const backend = new Backend(WITH_REVERB, { [WITH_REVERB]: [reverbOnTrack] });
    backend.reads = "fail";
    await open(backend);
    await answered(backend, [WITH_REVERB]);

    expect(screen.queryByTestId("render-warnings")).not.toBeInTheDocument();
    expect(screen.queryByTestId("render-error")).not.toBeInTheDocument();
  });
});
