/**
 * A refused clip move puts the chip back where the session has it.
 *
 * `ClipStrip` keeps a draft of the clips so a chip follows the pointer (or
 * the arrow key) without a round trip, and resets it from the `clips` array
 * it is handed whenever that array's identity changes. A rejected
 * `move_clip` changes nothing in the session, so the read that follows
 * returns data equal to the read before it. If the track query shared the
 * previous references for equal data, nothing downstream would re-run and
 * the chip would stay where it was dropped while the session held the old
 * position. The real `App` runs here with only the IPC boundary replaced.
 */

import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

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

const HEAD = "0000001".padEnd(64, "0");

class Backend {
  calls = 0;
  readonly moves: Args[] = [];
  /** Every `list_tracks` answer is a new object holding the same values. */
  handle = (cmd: string, _args: Args = {}): unknown => {
    this.calls++;
    switch (cmd) {
      case "move_clip":
        this.moves.push(_args);
        return Promise.reject("clip would overlap another clip");
      case "list_tracks":
        return [
          {
            id: "3f2b8c1e-7d4a-4e9b-9c6f-1a2b3c4d5e6f",
            name: "Voice",
            gain_db: 0,
            pan: 0,
            muted: false,
            soloed: false,
            audio_path: "/tmp/voice.wav",
            clips: [
              { start_sec: 2, length_sec: 3, source_path: "/tmp/voice.wav", volume_envelope: [] },
            ],
          },
        ];
      case "get_session_head":
        return HEAD;
      case "get_node":
        return { id: HEAD, parent: null };
      case "get_graph":
        return {
          nodes: [
            {
              id: HEAD,
              parent: null,
              label: "step 1",
              tool: "load",
              created_at: new Date(2026, 0, 1).toISOString(),
            },
          ],
          head: HEAD,
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
      case "get_transcript":
        return { words: [], source: { kind: "unknown" } };
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

const settle = () =>
  act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });

async function quiet(backend: Backend): Promise<void> {
  for (let guard = 0; guard < 20; guard++) {
    const before = backend.calls;
    await settle();
    if (backend.calls === before) return;
  }
  throw new Error("the app never stopped asking");
}

beforeEach(() => {
  vi.spyOn(window, "prompt").mockReturnValue(null);
});

afterEach(() => {
  cleanup();
  clearMocks();
  vi.restoreAllMocks();
});

it("returns the clip chip to the session's position after a refused move", async () => {
  const backend = new Backend();
  mockIPC((cmd, args) => backend.handle(cmd, args as Args | undefined), {
    shouldMockEvents: true,
  });
  render(<App />);
  const chip = await screen.findByTestId("clip-chip-0");
  await quiet(backend);
  expect(chip.getAttribute("aria-label")).toContain("2.00 to 5.00 seconds");

  // The chip moves at once, as a drag or an arrow key moves it.
  fireEvent.keyDown(chip, { key: "ArrowRight" });
  expect(screen.getByTestId("clip-chip-0").getAttribute("aria-label")).toContain("2.05 to 5.05");
  await waitFor(() => expect(backend.moves).toHaveLength(1));

  // The session refused, so the re-read of the same clip puts it back.
  await quiet(backend);
  await waitFor(() =>
    expect(screen.getByTestId("clip-chip-0").getAttribute("aria-label")).toContain(
      "2.00 to 5.00 seconds",
    ),
  );
});
