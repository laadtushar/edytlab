/**
 * Switching the mix must not throw away where you were (#246).
 *
 * The `[mixPath]` effect called `ws.load(...)` and nothing else.
 * WaveSurfer's `loadAudio()` pauses when playing, and `setSrc()`
 * reassigns `media.src`, which zeroes `currentTime` — so every A→B
 * click stopped playback and dropped the playhead to 0.
 *
 * That breaks the feature outright: comparing two renders means hearing
 * the *same moment* on each side, and doing that required manually
 * re-seeking and re-pressing Space after every switch. The M26 plan
 * promised "clicking toggles instantly without restart".
 *
 * The same effect also swallowed load failures with a blanket `.catch`.
 * The mix player is the transport and the only audible source — the
 * lanes are muted — so a failed load left the app silently mute while
 * the lanes went on drawing normally.
 */

import { render, screen, waitFor } from "@testing-library/react";
import { createRef } from "react";
import { describe, expect, it, vi } from "vitest";

const { instances } = vi.hoisted(() => {
  const instances: {
    url: string | null;
    currentTime: number;
    playing: boolean;
    play: ReturnType<typeof vi.fn>;
    setTime: ReturnType<typeof vi.fn>;
    loadResult: () => Promise<void>;
  }[] = [];
  return { instances };
});

vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  convertFileSrc: (path: string) => `asset://${path}`,
}));

vi.mock("wavesurfer.js", () => ({
  default: {
    create: () => {
      const entry = {
        url: null as string | null,
        currentTime: 0,
        playing: false,
        play: vi.fn(() => {
          entry.playing = true;
          return Promise.resolve();
        }),
        setTime: vi.fn((t: number) => {
          entry.currentTime = t;
        }),
        loadResult: () => Promise.resolve(),
      };
      instances.push(entry);
      return {
        on: vi.fn((event: string, cb: () => void) => {
          if (event === "decode") cb();
        }),
        un: vi.fn(),
        // Mirrors the real thing: loading a new source zeroes the
        // position and stops playback. If the component does not put
        // them back, they stay lost.
        load: vi.fn((url: string) => {
          entry.url = url;
          entry.currentTime = 0;
          entry.playing = false;
          return entry.loadResult();
        }),
        zoom: vi.fn(),
        play: entry.play,
        pause: vi.fn(() => {
          entry.playing = false;
        }),
        seekTo: vi.fn(),
        setTime: entry.setTime,
        setVolume: vi.fn(),
        setOptions: vi.fn(),
        isPlaying: () => entry.playing,
        destroy: vi.fn(),
        getDuration: () => (entry.url ? 60 : 0),
        getCurrentTime: () => entry.currentTime,
      };
    },
  },
}));

import { Timeline, type TimelineHandle } from "../components/Timeline";

const TRACKS = [
  { index: 0, name: "voice", audioPath: "/tmp/voice.wav", muted: false },
];

function mount(mixPath: string | null = "/tmp/a.wav") {
  instances.length = 0;
  const ref = createRef<TimelineHandle>();
  const view = render(<Timeline ref={ref} tracks={TRACKS} mixPath={mixPath} />);
  // The parent's effect runs after its children's, so the mix player is
  // the last one created.
  const mix = () => instances[instances.length - 1];
  return { ref, view, mix };
}

describe("switching the mix path", () => {
  it("restores the playhead after the new side loads", async () => {
    const { view, mix } = mount("/tmp/a.wav");

    // Ten seconds into side A.
    mix().currentTime = 10;

    view.rerender(<Timeline tracks={TRACKS} mixPath="/tmp/b.wav" />);

    await waitFor(() =>
      expect(mix().url).toBe("asset:///tmp/b.wav"),
    );
    await waitFor(() =>
      expect(
        mix().currentTime,
        "the A/B switch dropped the playhead to 0",
      ).toBe(10),
    );
  });

  it("resumes playing if it was playing before the switch", async () => {
    const { view, mix } = mount("/tmp/a.wav");

    mix().currentTime = 4;
    mix().playing = true;

    view.rerender(<Timeline tracks={TRACKS} mixPath="/tmp/b.wav" />);

    await waitFor(() =>
      expect(
        mix().play,
        "playback stopped on the switch and never came back",
      ).toHaveBeenCalled(),
    );
  });

  it("does not start playing if it was paused before the switch", async () => {
    const { view, mix } = mount("/tmp/a.wav");

    mix().currentTime = 4;
    mix().playing = false;
    mix().play.mockClear();

    view.rerender(<Timeline tracks={TRACKS} mixPath="/tmp/b.wav" />);

    await waitFor(() => expect(mix().url).toBe("asset:///tmp/b.wav"));
    expect(
      mix().play,
      "a switch while paused must not start playback",
    ).not.toHaveBeenCalled();
  });
});

describe("when the mix cannot load", () => {
  it("says so instead of going silently mute", async () => {
    const { view, mix } = mount("/tmp/a.wav");

    mix().loadResult = () => Promise.reject(new Error("ENOENT"));
    view.rerender(<Timeline tracks={TRACKS} mixPath="/tmp/gone.wav" />);

    const alert = await screen.findByTestId("timeline-mix-error");
    expect(alert.textContent).toMatch(/ENOENT/);
  });

  /**
   * A rapid A/B toggle aborts the previous load. That is the system
   * working — surfacing it would put an error on screen every time the
   * user clicks quickly, which is the likely reason the original
   * blanket `.catch` existed.
   */
  it("stays quiet when a load is superseded", async () => {
    const { view, mix } = mount("/tmp/a.wav");

    const abort = new DOMException("aborted", "AbortError");
    mix().loadResult = () => Promise.reject(abort);
    view.rerender(<Timeline tracks={TRACKS} mixPath="/tmp/b.wav" />);

    await waitFor(() => expect(mix().url).toBe("asset:///tmp/b.wav"));
    expect(screen.queryByTestId("timeline-mix-error")).not.toBeInTheDocument();
  });
});
