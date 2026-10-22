/**
 * The Transcript tab follows the agent's transcription, and cuts only the
 * track the transcript is of (#384).
 *
 * `transcribe` appends a node, and the window learns of it as any agent edit:
 * `agent://node-created` moves the head, and the tab reads `get_transcript`
 * again on a head move. A transcript's word times are seconds of the audio
 * it was made from, so the tab's Delete cuts that track and no other. The tab
 * used to cut track 0 whatever the transcript was of, which is a destructive
 * edit of the wrong audio the moment a transcript can be of track 1.
 *
 * The real `App` runs with only the IPC boundary replaced, as in
 * `App.sessionMoves.test.tsx`.
 */

import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
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
/** Before the transcription, and the node the assistant's transcription made. */
const BEFORE = node(1);
const TRANSCRIBED = node(2);
/** What a cut from the tab answers with. */
const CUT = node(3);

const headLabel = (id: string): string => `head ${id.slice(0, 7)}`;

const WORDS = ["So,", "um", "this", "works."].map((text, i) => ({
  text,
  start_sec: i,
  end_sec: i + 0.8,
  confidence: 0.9,
}));

type Source =
  | { kind: "track"; track: number }
  | { kind: "file" }
  | { kind: "gone" }
  | { kind: "unknown" };

class Backend {
  head = BEFORE;
  /** What the transcript is at the new head. */
  source: Source = { kind: "track", track: 1 };
  readonly sent: { cmd: string; args: Args }[] = [];

  count(cmd: string): number {
    return this.sent.filter((s) => s.cmd === cmd).length;
  }

  handle = (cmd: string, args: Args = {}): unknown => {
    this.sent.push({ cmd, args });
    switch (cmd) {
      case "list_tracks":
        return ["Music", "Voice"].map((name, i) => ({
          id: `0000000${i}-0000-4000-8000-000000000000`,
          name,
          gain_db: 0,
          pan: 0,
          muted: false,
          soloed: false,
          audio_path: "/tmp/voice.wav",
          clips: [
            { start_sec: 0, length_sec: 10, source_path: "/tmp/voice.wav", volume_envelope: [] },
          ],
        }));
      case "get_transcript":
        return this.head === TRANSCRIBED
          ? { words: WORDS, source: this.source }
          : { words: [], source: { kind: "unknown" } };
      case "cut_transcript_words":
        return CUT;
      case "get_session_head":
        return this.head;
      case "get_node":
        return { id: args.id, parent: args.id === BEFORE ? null : BEFORE };
      case "get_graph":
        return {
          nodes: [
            { id: BEFORE, parent: null, label: "load", tool: "load", created_at: at(0) },
            { id: TRANSCRIBED, parent: BEFORE, label: "transcribe", tool: "transcribe", created_at: at(1) },
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
}

const at = (n: number) => `2026-10-22T00:0${n}:00Z`;

const settle = () =>
  act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });

async function start(source: Source): Promise<Backend> {
  const backend = new Backend();
  backend.source = source;
  mockIPC((cmd, args) => backend.handle(cmd, args as Args | undefined), {
    shouldMockEvents: true,
  });
  render(<App />);
  await screen.findAllByTestId("timeline-lane-gain");
  await waitFor(() =>
    expect(screen.getByTestId("status-bar-head")).toHaveTextContent(headLabel(BEFORE)),
  );
  await settle();
  return backend;
}

/** The assistant transcribes: the store's head moves, and the window is told. */
async function theAssistantTranscribes(backend: Backend): Promise<void> {
  backend.head = TRANSCRIBED;
  await act(async () => {
    await emit("agent://node-created", { node_id: TRANSCRIBED });
  });
  await waitFor(() =>
    expect(screen.getByTestId("status-bar-head")).toHaveTextContent(headLabel(TRANSCRIBED)),
  );
  await settle();
}

/** Select words `from..=to` in the pane and press Delete. */
function dragAndDelete(from: number, to: number): void {
  const words = screen.getAllByTestId("transcript-word");
  fireEvent.mouseDown(words[from], { button: 0 });
  fireEvent.mouseEnter(words[to]);
  fireEvent.mouseUp(window);
  fireEvent.keyDown(screen.getByTestId("transcript-pane"), { key: "Delete" });
}

beforeEach(() => {
  if (!("ResizeObserver" in window)) {
    class RO {
      observe(): void {}
      unobserve(): void {}
      disconnect(): void {}
    }
    (window as unknown as { ResizeObserver: typeof RO }).ResizeObserver = RO;
  }
  vi.spyOn(window, "prompt").mockReturnValue(null);
});

afterEach(() => {
  cleanup();
  clearMocks();
  vi.restoreAllMocks();
});

describe("the Transcript tab and a transcription by the assistant", () => {
  it("shows the words that arrive with the assistant's node, read again on the head move", async () => {
    const backend = await start({ kind: "track", track: 1 });
    fireEvent.click(screen.getByTestId("tab-transcript"));
    expect(await screen.findByTestId("transcript-empty")).toBeInTheDocument();
    const before = backend.count("get_transcript");

    await theAssistantTranscribes(backend);

    await waitFor(() => expect(screen.getAllByTestId("transcript-word")).toHaveLength(4));
    expect(backend.count("get_transcript")).toBeGreaterThan(before);
    expect(screen.queryByTestId("transcript-empty")).toBeNull();
    expect(screen.getAllByTestId("transcript-word")[1]).toHaveTextContent("um");
  });

  it("cuts the track the transcript is of, not track 0", async () => {
    const backend = await start({ kind: "track", track: 1 });
    fireEvent.click(screen.getByTestId("tab-transcript"));
    await theAssistantTranscribes(backend);
    await waitFor(() => expect(screen.getAllByTestId("transcript-word")).toHaveLength(4));
    expect(screen.queryByTestId("transcript-cut-blocked")).toBeNull();

    dragAndDelete(1, 1);

    await waitFor(() => expect(backend.count("cut_transcript_words")).toBe(1));
    const sent = backend.sent.find((s) => s.cmd === "cut_transcript_words");
    expect(sent?.args).toEqual({ track: 1, fromWord: 1, toWord: 2 });
  });

  it("follows the track when it moves: the index the backend names is the one cut", async () => {
    const backend = await start({ kind: "track", track: 0 });
    fireEvent.click(screen.getByTestId("tab-transcript"));
    await theAssistantTranscribes(backend);
    await waitFor(() => expect(screen.getAllByTestId("transcript-word")).toHaveLength(4));

    dragAndDelete(0, 0);

    await waitFor(() => expect(backend.count("cut_transcript_words")).toBe(1));
    const sent = backend.sent.find((s) => s.cmd === "cut_transcript_words");
    expect(sent?.args).toEqual({ track: 0, fromWord: 0, toWord: 1 });
  });

  for (const [name, source, says] of [
    ["a file", { kind: "file" }, /made from a file/],
    ["a track that is gone", { kind: "gone" }, /has been removed/],
    ["a source it does not record", { kind: "unknown" }, /does not say which track/],
  ] as [string, Source, RegExp][]) {
    it(`shows the words of ${name} and cuts nothing, saying why`, async () => {
      const backend = await start(source);
      fireEvent.click(screen.getByTestId("tab-transcript"));
      await theAssistantTranscribes(backend);
      await waitFor(() => expect(screen.getAllByTestId("transcript-word")).toHaveLength(4));
      expect(screen.getByTestId("transcript-cut-blocked").textContent).toMatch(says);

      dragAndDelete(1, 2);
      await settle();

      expect(backend.count("cut_transcript_words")).toBe(0);
    });
  }
});
