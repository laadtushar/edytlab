/**
 * A lane's <audio> element never loads.
 *
 * Lanes are pictures; only the mix player plays. In WebKitGTK every media
 * element with a source is a GStreamer pipeline that starts streaming as
 * soon as the source is set, and an edit swaps every lane's source at
 * once. Tearing those pipelines down mid-stream deadlocked the webview in
 * a native run: the main thread waited in `gst_pad_pause_task` while the
 * lanes' source threads blocked on full queues, and the window froze
 * after the assistant's third edit.
 *
 * So a lane's element is created with `preload="none"`, and the lane gives
 * WaveSurfer a duration so it decodes without waiting for metadata the
 * element will never load.
 */

import { render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

interface Created {
  options: { media?: HTMLMediaElement; barWidth?: number };
  loads: unknown[][];
}

const { created } = vi.hoisted(() => ({ created: [] as Created[] }));

vi.mock("wavesurfer.js", () => ({
  default: {
    create: (options: Created["options"]) => {
      const entry: Created = { options, loads: [] };
      created.push(entry);
      return {
        on: vi.fn(),
        un: vi.fn(),
        load: vi.fn((...args: unknown[]) => {
          entry.loads.push(args);
          return Promise.resolve();
        }),
        zoom: vi.fn(),
        play: vi.fn(),
        pause: vi.fn(),
        seekTo: vi.fn(),
        setTime: vi.fn(),
        setVolume: vi.fn(),
        setOptions: vi.fn(),
        isPlaying: vi.fn(() => false),
        destroy: vi.fn(),
        getDuration: () => 0,
        getCurrentTime: () => 0,
        getWrapper: () => document.createElement("div"),
        setScroll: vi.fn(),
        getScroll: () => 0,
      };
    },
  },
}));

import { Timeline } from "../components/Timeline";

describe("a lane's media element", () => {
  it("is created not to load, and the lane decodes without waiting for it", () => {
    created.length = 0;
    render(
      <Timeline
        tracks={[{ index: 0, name: "music", audioPath: "/tmp/music.wav", muted: false }]}
      />,
    );

    // The lane is the instance that draws bars; the others are the
    // players, which do load their elements, because they play.
    const lanes = created.filter((c) => c.options.barWidth !== undefined);
    expect(lanes).toHaveLength(1);
    const [lane] = lanes;
    const media = lane.options.media;
    expect(media).toBeInstanceOf(HTMLAudioElement);
    expect(media?.preload).toBe("none");
    for (const player of created.filter((c) => c !== lane)) {
      expect(player.options.media?.preload).not.toBe("none");
    }

    // load(url, peaks, duration): a duration, so WaveSurfer does not wait
    // for `loadedmetadata` from an element that never loads.
    expect(lane.loads).toHaveLength(1);
    const [, peaks, duration] = lane.loads[0];
    expect(peaks).toBeUndefined();
    expect(duration).toBeGreaterThan(0);
  });
});
