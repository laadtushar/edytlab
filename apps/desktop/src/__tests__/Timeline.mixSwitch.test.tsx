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

import { act, render, screen, waitFor } from "@testing-library/react";
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

/**
 * The mix plays on one of two players (#269 §2): a switch loads the new
 * side onto the idle one and hands the transport over. So these name
 * players by what they hold, not by when they were created.
 */
const holding = (path: string) => instances.find((i) => i.url === `asset://${path}`);

async function mount(mixPath = "/tmp/a.wav") {
  instances.length = 0;
  const ref = createRef<TimelineHandle>();
  const view = render(<Timeline ref={ref} tracks={TRACKS} mixPath={mixPath} />);
  await waitFor(() => expect(holding(mixPath)).toBeDefined());
  const current = holding(mixPath)!;
  // The other mix player — the parent creates both after the lanes — is
  // where the next switch loads.
  const idle = instances.slice(-2).find((i) => i !== current)!;
  return { ref, view, current, idle };
}

describe("switching the mix path", () => {
  it("restores the playhead after the new side loads", async () => {
    const { view, current } = await mount("/tmp/a.wav");

    // Ten seconds into side A.
    current.currentTime = 10;

    view.rerender(<Timeline tracks={TRACKS} mixPath="/tmp/b.wav" />);

    await waitFor(() =>
      expect(
        holding("/tmp/b.wav")?.currentTime,
        "the A/B switch dropped the playhead to 0",
      ).toBe(10),
    );
  });

  it("resumes playing if it was playing before the switch", async () => {
    const { view, current } = await mount("/tmp/a.wav");

    current.currentTime = 4;
    current.playing = true;

    view.rerender(<Timeline tracks={TRACKS} mixPath="/tmp/b.wav" />);

    await waitFor(() =>
      expect(
        holding("/tmp/b.wav")?.play,
        "playback stopped on the switch and never came back",
      ).toHaveBeenCalled(),
    );
  });

  it("does not start playing if it was paused before the switch", async () => {
    const { view, current } = await mount("/tmp/a.wav");

    current.currentTime = 4;
    current.playing = false;

    view.rerender(<Timeline tracks={TRACKS} mixPath="/tmp/b.wav" />);

    await waitFor(() => expect(holding("/tmp/b.wav")).toBeDefined());
    await act(async () => {});
    expect(
      holding("/tmp/b.wav")!.play,
      "a switch while paused must not start playback",
    ).not.toHaveBeenCalled();
  });
});

describe("when the mix cannot load", () => {
  it("says so instead of going silently mute", async () => {
    const { view, idle } = await mount("/tmp/a.wav");

    idle.loadResult = () => Promise.reject(new Error("ENOENT"));
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
    const { view, idle } = await mount("/tmp/a.wav");

    const abort = new DOMException("aborted", "AbortError");
    idle.loadResult = () => Promise.reject(abort);
    view.rerender(<Timeline tracks={TRACKS} mixPath="/tmp/b.wav" />);

    await waitFor(() => expect(holding("/tmp/b.wav")).toBeDefined());
    await act(async () => {});
    expect(screen.queryByTestId("timeline-mix-error")).not.toBeInTheDocument();
  });
});
