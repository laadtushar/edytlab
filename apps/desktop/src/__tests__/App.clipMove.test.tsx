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

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { clearMocks } from "@tauri-apps/api/mocks";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

vi.mock("wavesurfer.js", async () => (await import("./wavesurferMock")).wavesurferModule());

import { type Args, appAnswer, bootIpc, quiet, voiceTrack } from "./appHarness";
import App from "../App";

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
          voiceTrack({
            clips: [
              { start_sec: 2, length_sec: 3, source_path: "/tmp/voice.wav", volume_envelope: [] },
            ],
          }),
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
      case "save_head_trail":
        return null;
      default:
        return appAnswer(cmd);
    }
  };
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
  bootIpc(backend);
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
