/**
 * The snap toggle, and the case where snapping must not happen (#161).
 *
 * The interesting half is the second one. Selection is measured on the
 * session axis (#171), so this lane's samples are only the audio under
 * the cursor when the lane runs the length of the session. Snapping
 * against the wrong buffer would move the boundary to a crossing that
 * is not where the user is cutting — and it would do it invisibly.
 */

import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

const SR = 48_000;
const LANE_SECONDS = 10;

/** 100 Hz sine: zero crossings every 240 samples. */
function sine(length: number, period: number): Float32Array {
  const out = new Float32Array(length);
  for (let i = 0; i < length; i++) out[i] = Math.sin((2 * Math.PI * i) / period);
  return out;
}

const CHANNEL = sine(SR * LANE_SECONDS, 480);

vi.mock("wavesurfer.js", () => ({
  default: {
    create: () => ({
      on: vi.fn((event: string, cb: () => void) => {
        if (event === "decode") cb();
      }),
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
      getDuration: () => LANE_SECONDS,
      getCurrentTime: () => 0,
      getDecodedData: () => ({
        sampleRate: SR,
        getChannelData: () => CHANNEL,
      }),
    }),
  },
}));

import { Timeline } from "../components/Timeline";

const PANE_WIDTH = 1000;

function pinWidth() {
  Object.defineProperty(HTMLElement.prototype, "getBoundingClientRect", {
    configurable: true,
    value: () => ({
      left: 0,
      top: 0,
      right: PANE_WIDTH,
      bottom: 92,
      width: PANE_WIDTH,
      height: 92,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    }),
  });
  Object.defineProperty(HTMLElement.prototype, "clientWidth", {
    configurable: true,
    value: PANE_WIDTH,
  });
}

function dragAcross(fromPx: number, toPx: number) {
  const surface = screen.getAllByTestId("timeline-lane-waveform")[0]
    .parentElement as HTMLElement;
  fireEvent.mouseDown(surface, { button: 0, clientX: fromPx, clientY: 40 });
  fireEvent.mouseMove(window, { clientX: toPx, clientY: 40 });
  fireEvent.mouseUp(window, { clientX: toPx, clientY: 40 });
}

/** A lane exactly as long as the session — the snappable case. */
const MATCHED = [
  {
    index: 0,
    name: "voice",
    audioPath: "/tmp/voice.wav",
    muted: false,
    clips: [
      {
        start_sec: 0,
        length_sec: LANE_SECONDS,
        source_path: "/tmp/voice.wav",
        volume_envelope: [],
      },
    ],
  },
];

/** Lane 0 covers a sixth of the session — not snappable. */
const MISMATCHED = [
  MATCHED[0],
  {
    index: 1,
    name: "music",
    audioPath: "/tmp/music.wav",
    muted: false,
    clips: [
      {
        start_sec: 0,
        length_sec: LANE_SECONDS * 6,
        source_path: "/tmp/music.wav",
        volume_envelope: [],
      },
    ],
  },
];

function lastSelection(mock: ReturnType<typeof vi.fn>) {
  const calls = mock.mock.calls;
  return calls[calls.length - 1]?.[0];
}

/** Distance from a time to the nearest crossing, in samples. */
function offsetFromCrossing(t: number): number {
  const idx = Math.round(t * SR);
  return Math.min(idx % 240, 240 - (idx % 240));
}

describe("snap to zero crossings", () => {
  it("is off by default, and leaves the selection where it was dragged", () => {
    pinWidth();
    const onSelectionChange = vi.fn();
    render(
      <Timeline
        tracks={MATCHED}
        selection={null}
        onSelectionChange={onSelectionChange}
      />,
    );

    // One pixel is 480 samples here — an exact multiple of the 240-sample
    // crossing interval — so every whole pixel would land on a crossing
    // by accident and prove nothing. Drag to fractional pixels.
    dragAcross(101.3, 503.7);
    const sel = lastSelection(onSelectionChange);
    expect(sel).toBeTruthy();
    expect(offsetFromCrossing(sel.start)).toBeGreaterThan(0);
  });

  it("snaps both edges when the toggle is on", () => {
    pinWidth();
    const onSelectionChange = vi.fn();
    render(
      <Timeline
        tracks={MATCHED}
        selection={null}
        snapToZero
        onSelectionChange={onSelectionChange}
      />,
    );

    dragAcross(101.3, 503.7);
    const sel = lastSelection(onSelectionChange);
    expect(sel).toBeTruthy();
    expect(offsetFromCrossing(sel.start)).toBe(0);
    expect(offsetFromCrossing(sel.end)).toBe(0);
    expect(sel.end).toBeGreaterThan(sel.start);
  });

  /**
   * The lane's audio is not the audio at that session time, so the
   * crossing it would find belongs to a different waveform. Leaving the
   * selection alone is the only honest option.
   */
  it("does not snap when the lane is not the session axis", () => {
    pinWidth();
    const onSelectionChange = vi.fn();
    render(
      <Timeline
        tracks={MISMATCHED}
        selection={null}
        snapToZero
        onSelectionChange={onSelectionChange}
      />,
    );

    dragAcross(101.3, 503.7);
    const sel = lastSelection(onSelectionChange);
    expect(sel).toBeTruthy();
    // Sixty seconds across 1000 px: the dragged seconds are six times
    // the matched case, and untouched by snapping.
    expect(sel.start).toBeCloseTo((101.3 / PANE_WIDTH) * LANE_SECONDS * 6, 4);
  });

  it("reports its state on the toggle button", () => {
    pinWidth();
    const onSnapToZeroChange = vi.fn();
    const { rerender } = render(
      <Timeline tracks={MATCHED} onSnapToZeroChange={onSnapToZeroChange} />,
    );

    const btn = screen.getByTestId("snap-zero-btn");
    expect(btn).toHaveAttribute("aria-pressed", "false");
    fireEvent.click(btn);
    expect(onSnapToZeroChange).toHaveBeenCalledWith(true);

    rerender(
      <Timeline
        tracks={MATCHED}
        snapToZero
        onSnapToZeroChange={onSnapToZeroChange}
      />,
    );
    expect(screen.getByTestId("snap-zero-btn")).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });
});
