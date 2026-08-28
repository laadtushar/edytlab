/**
 * One transport, on the mix (#155).
 *
 * Reported from the app: "when I play only first part played not the
 * second one". Each lane mounted its own player and the transport
 * captured exactly one of them — lane 0 — so pressing play played
 * track 1's raw audio and nothing else. Every other track was not
 * merely out of sync; it never started.
 *
 * And what did play was *unmixed*: a lane holds one track's own file,
 * so gain, pan, mute, solo, per-track chains, sends and the master
 * chain were all inaudible even though every one of them renders
 * correctly.
 *
 * So the test is about which player receives the command. A transport
 * that drives a lane would pass any test that only checked "play was
 * called".
 */

import { render } from "@testing-library/react";
import { createRef } from "react";
import { describe, expect, it, vi } from "vitest";

const { instances } = vi.hoisted(() => {
  const instances: {
    url: string | null;
    play: ReturnType<typeof vi.fn>;
    pause: ReturnType<typeof vi.fn>;
    setTime: ReturnType<typeof vi.fn>;
    playing: boolean;
  }[] = [];
  return { instances };
});

// `convertFileSrc` needs Tauri's internals, which jsdom does not have.
// The component already survives that (it catches and leaves the lanes
// drawing), but the test needs the URL to arrive to assert on it.
vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  convertFileSrc: (path: string) => `asset://${path}`,
}));

vi.mock("wavesurfer.js", () => ({
  default: {
    create: () => {
      const entry = {
        url: null as string | null,
        // WaveSurfer 7's play() returns a Promise; the mock must too,
        // or it hides a rejected play() rather than exercising it.
        play: vi.fn(() => Promise.resolve()),
        pause: vi.fn(),
        setTime: vi.fn(),
        playing: false,
      };
      instances.push(entry);
      return {
        on: vi.fn((event: string, cb: () => void) => {
          if (event === "decode") cb();
        }),
        un: vi.fn(),
        load: vi.fn((url: string) => {
          entry.url = url;
          return Promise.resolve();
        }),
        zoom: vi.fn(),
        play: entry.play,
        pause: entry.pause,
        seekTo: vi.fn(),
        setTime: entry.setTime,
        setVolume: vi.fn(),
        setOptions: vi.fn(),
        isPlaying: () => entry.playing,
        destroy: vi.fn(),
        // A player with nothing loaded has no duration, which is what
        // makes a seek against a cold start a no-op in the real thing.
        getDuration: () => (entry.url ? 60 : 0),
        getCurrentTime: () => 0,
      };
    },
  },
}));

import { Timeline, type TimelineHandle } from "../components/Timeline";

const TRACKS = [
  { index: 0, name: "voice", audioPath: "/tmp/voice.wav", muted: false },
  { index: 1, name: "music", audioPath: "/tmp/music.wav", muted: false },
];

function setup(mixPath: string | null = "/tmp/mix.wav") {
  instances.length = 0;
  const ref = createRef<TimelineHandle>();
  render(<Timeline ref={ref} tracks={TRACKS} mixPath={mixPath} />);
  // The parent's effect runs after its children's, so the mix player is
  // the last one created.
  const mix = instances[instances.length - 1];
  const lanes = instances.slice(0, -1);
  return { ref, mix, lanes };
}

describe("the transport", () => {
  it("plays the mix, and never a lane", () => {
    const { ref, mix, lanes } = setup();

    ref.current?.play();

    expect(mix.play).toHaveBeenCalled();
    expect(lanes).toHaveLength(TRACKS.length);
    for (const lane of lanes) {
      expect(
        lane.play,
        "a lane holds one track's raw audio; playing it is the bug",
      ).not.toHaveBeenCalled();
    }
  });

  it("loads the rendered mix, not a track's own file", () => {
    const { mix } = setup();
    expect(mix.url).toContain("mix.wav");
  });

  it("seeks the mix", () => {
    const { ref, mix, lanes } = setup();
    ref.current?.seekTo(12.5);
    expect(mix.setTime).toHaveBeenCalledWith(12.5);
    for (const lane of lanes) {
      expect(lane.setTime).not.toHaveBeenCalled();
    }
  });

  it("reports time and duration from the mix", () => {
    const { ref } = setup();
    expect(ref.current?.getDuration()).toBe(60);
    expect(ref.current?.getCurrentTime()).toBe(0);
  });

  /**
   * A cold start has no head, so there is no mix to play. The transport
   * has to be a no-op rather than a crash — the fifth blocker on #155's
   * revised plan.
   */
  it("does nothing, quietly, when there is no mix", () => {
    const { ref, mix, lanes } = setup(null);
    expect(() => {
      ref.current?.play();
      ref.current?.togglePlay();
      ref.current?.seekTo(5);
      ref.current?.seekBy(1);
      ref.current?.pause();
    }).not.toThrow();

    // The player exists but was never given anything to load, so it
    // has no duration — and a seek against no duration must not move
    // it.
    expect(mix.url).toBeNull();
    expect(mix.setTime).not.toHaveBeenCalled();
    for (const lane of lanes) {
      expect(lane.play).not.toHaveBeenCalled();
    }
  });
});
