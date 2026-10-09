/**
 * A lane does not zoom audio it has not decoded yet.
 *
 * WaveSurfer's `zoom()` throws "No audio loaded" from the moment `load()`
 * starts until the new file decodes, and in that window the lane's
 * `duration` still holds the previous file's length. An edit that swaps a
 * lane's file and changes the session's length at the same moment moved
 * the zoom inside that window: the throw came out of an effect, so it took
 * the timeline down with it. A Playwright run caught it once
 * (`edited-names.spec.ts`: the clip chip never appeared, and the page
 * reported "No audio loaded").
 *
 * The mock keeps WaveSurfer's contract: no decoded data after `load()`,
 * and `zoom()` throwing until a decode.
 */

import { act, render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

interface Lane {
  /** Lanes draw bars; the players created alongside them do not. */
  isLane: boolean;
  decoded: object | null;
  onDecode: Array<() => void>;
  zooms: number[];
}

const { lanes } = vi.hoisted(() => ({ lanes: [] as Lane[] }));

vi.mock("wavesurfer.js", () => ({
  default: {
    create: (options: { barWidth?: number }) => {
      const lane: Lane = { isLane: options.barWidth !== undefined, decoded: null, onDecode: [], zooms: [] };
      lanes.push(lane);
      return {
        on: vi.fn((event: string, cb: () => void) => {
          if (event === "decode") lane.onDecode.push(cb);
        }),
        un: vi.fn(),
        load: vi.fn(() => {
          lane.decoded = null;
          return Promise.resolve();
        }),
        getDecodedData: () => lane.decoded,
        zoom: (pxPerSec: number) => {
          if (!lane.decoded) throw new Error("No audio loaded");
          lane.zooms.push(pxPerSec);
        },
        play: vi.fn(),
        pause: vi.fn(),
        seekTo: vi.fn(),
        setTime: vi.fn(),
        setVolume: vi.fn(),
        setOptions: vi.fn(),
        isPlaying: vi.fn(() => false),
        destroy: vi.fn(),
        // The same length before and after the edit: no new duration to
        // re-run the zoom, so only the decode can bring it back.
        getDuration: () => 10,
        getCurrentTime: () => 0,
        getWrapper: () => document.createElement("div"),
        setScroll: vi.fn(),
        getScroll: () => 0,
      };
    },
  },
}));

import { Timeline } from "../components/Timeline";

Object.defineProperty(HTMLElement.prototype, "clientWidth", {
  configurable: true,
  value: 600,
});

function track(audioPath: string, lengthSec: number) {
  return {
    index: 0,
    name: "music",
    audioPath,
    muted: false,
    clips: [{ start_sec: 0, length_sec: lengthSec, source_path: audioPath, volume_envelope: [] }],
  };
}

const lastZoom = (lane: Lane) => lane.zooms[lane.zooms.length - 1];

function decode(lane: Lane) {
  act(() => {
    lane.decoded = {};
    for (const cb of lane.onDecode) cb();
  });
}

describe("a lane whose file changes", () => {
  it("waits for the new audio before zooming, then zooms to the new scale", () => {
    lanes.length = 0;
    const { rerender } = render(<Timeline tracks={[track("/tmp/before.wav", 10)]} />);
    const [lane] = lanes.filter((l) => l.isLane);
    decode(lane);
    const before = lastZoom(lane);
    expect(before).toBeGreaterThan(0);

    // The edit lands in two renders, as it does in the app: the lane's
    // new file first, and the session's new length a moment later, while
    // that file is still loading.
    rerender(<Timeline tracks={[track("/tmp/after.wav", 10)]} />);
    expect(lane.decoded).toBeNull();
    expect(() => rerender(<Timeline tracks={[track("/tmp/after.wav", 20)]} />)).not.toThrow();
    expect(lastZoom(lane)).toBe(before);

    decode(lane);
    const after = lastZoom(lane);
    expect(after).toBeGreaterThan(0);
    expect(after).not.toBe(before);
  });
});
