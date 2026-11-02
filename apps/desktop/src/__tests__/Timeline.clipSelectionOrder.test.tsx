/**
 * A selected clip does not move to another track when the track order changes
 * (#771, `reorder_tracks`).
 *
 * The selection is kept as "lane:clip". After the assistant reorders the
 * tracks, the same key would name another track's clip, and Remove would act
 * on it. The selection is dropped when the order changes.
 */

import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

const { instances } = vi.hoisted(() => ({ instances: [] as unknown[] }));

vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  convertFileSrc: (path: string) => `asset://${path}`,
}));

vi.mock("wavesurfer.js", async () => {
  const { eventedPlayer } = await import("./wavesurferStub");
  return { default: { create: () => eventedPlayer(instances as never) } };
});

import { Timeline } from "../components/Timeline";

const clip = (path: string) => ({
  start_sec: 0,
  length_sec: 60,
  source_path: path,
  volume_envelope: [],
});

const voice = { name: "voice", audioPath: "/tmp/voice.wav", muted: false, clips: [clip("/tmp/voice.wav")] };
const music = { name: "music", audioPath: "/tmp/music.wav", muted: false, clips: [clip("/tmp/music.wav")] };

function pressed(trackName: string): boolean {
  const strip = screen.getByRole("group", { name: `${trackName} clips` });
  return within(strip).getByTestId("clip-chip-0").getAttribute("aria-pressed") === "true";
}

describe("Timeline clip selection and track order", () => {
  it("drops the selection when the tracks are reordered", () => {
    const props = { mixPath: null, selection: null, onSelectionChange: vi.fn(), onMoveClip: vi.fn() };
    const view = render(
      <Timeline tracks={[{ ...voice, index: 0 }, { ...music, index: 1 }]} {...props} />,
    );
    const strip = screen.getByRole("group", { name: "voice clips" });
    fireEvent.pointerDown(within(strip).getByTestId("clip-chip-0"));
    expect(pressed("voice")).toBe(true);
    expect(pressed("music")).toBe(false);

    view.rerender(
      <Timeline tracks={[{ ...music, index: 0 }, { ...voice, index: 1 }]} {...props} />,
    );
    // Lane 0 is now "music": its clip must not inherit the selection.
    expect(pressed("music")).toBe(false);
    expect(pressed("voice")).toBe(false);
  });

  it("keeps the selection when the order is unchanged", () => {
    const props = { mixPath: null, selection: null, onSelectionChange: vi.fn(), onMoveClip: vi.fn() };
    const view = render(
      <Timeline tracks={[{ ...voice, index: 0 }, { ...music, index: 1 }]} {...props} />,
    );
    const strip = screen.getByRole("group", { name: "voice clips" });
    fireEvent.pointerDown(within(strip).getByTestId("clip-chip-0"));
    view.rerender(
      <Timeline tracks={[{ ...voice, index: 0, gainDb: -3 }, { ...music, index: 1 }]} {...props} />,
    );
    expect(pressed("voice")).toBe(true);
  });
});
