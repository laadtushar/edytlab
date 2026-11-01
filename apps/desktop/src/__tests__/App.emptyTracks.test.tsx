/**
 * A track with no clips is drawn (#742).
 *
 * "Start from template" puts tracks in the session that hold no audio yet,
 * and the lane list used to leave out every track without an `audio_path`,
 * so the window kept the welcome screen up over a session of four tracks.
 * Each empty track gets a lane, and keeps the `index` the mixer commands
 * address it by.
 */

import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("wavesurfer.js", async () => {
  const { wavesurferWith } = await import("./wavesurferStub");
  return wavesurferWith(() => ({
    getDuration: () => 0,
  }));
});

import { type Args, appAnswer } from "./appHarness";
import App from "../App";

const HEAD = "0000001".padEnd(64, "0");

const empty = (name: string) => ({
  id: `id-${name}`,
  name,
  gain_db: 0,
  pan: 0,
  muted: false,
  soloed: false,
  audio_path: null,
  clips: [],
});

const voice = {
  ...empty("Voice"),
  audio_path: "/tmp/voice.wav",
  clips: [{ start_sec: 0, length_sec: 10, source_path: "/tmp/voice.wav", volume_envelope: [] }],
};

function backend(tracks: unknown[], sent: { cmd: string; args: Args }[] = []) {
  mockIPC(
    (cmd, a) => {
      const args = (a ?? {}) as Args;
      switch (cmd) {
        case "list_tracks":
          return tracks;
        case "get_session_head":
          return HEAD;
        case "get_graph":
          return { nodes: [], head: HEAD };
        case "set_track_muted":
        case "set_track_gain":
        case "set_track_pan":
        case "set_track_soloed":
          sent.push({ cmd, args });
          return HEAD;
        default:
          return appAnswer(cmd);
      }
    },
    { shouldMockEvents: true },
  );
}

afterEach(() => {
  cleanup();
  clearMocks();
  vi.restoreAllMocks();
});

describe("tracks with no audio", () => {
  it("draws a lane for each track of a fresh template", async () => {
    backend(["Lead", "Harmony", "Bass", "Drums"].map(empty));
    render(<App />);

    await waitFor(() => expect(screen.getAllByTestId("timeline-lane")).toHaveLength(4));
    const names = screen.getAllByTestId("timeline-lane-name").map((n) => n.textContent);
    expect(names).toEqual(["Lead", "Harmony", "Bass", "Drums"]);
  });

  it("addresses an empty lane by its place in the session, not in the drawn list", async () => {
    const sent: { cmd: string; args: Args }[] = [];
    backend([empty("Lead"), voice, empty("Bass")], sent);
    render(<App />);

    await waitFor(() => expect(screen.getAllByTestId("timeline-lane")).toHaveLength(3));
    const mutes = screen.getAllByTestId("timeline-lane-mute");
    await act(async () => {
      mutes[2].click();
    });
    await waitFor(() => expect(sent.map((s) => s.cmd)).toContain("set_track_muted"));
    expect(sent.find((s) => s.cmd === "set_track_muted")?.args.track).toBe(2);
  });
});
