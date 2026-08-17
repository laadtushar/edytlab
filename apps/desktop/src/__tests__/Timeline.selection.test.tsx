/**
 * Selection is measured on the session axis, not on lane 0's own audio
 * (#171).
 *
 * The bug: selection was gated and drawn against the lane's decoded
 * `duration`, and export handed those seconds to `render_range` as
 * session-absolute time. On a 60-second session whose first track is a
 * 10-second clip, dragging across half the lane selected "0–5 s" and
 * exported 0–5 s *of the session* — a different span of different
 * audio.
 *
 * It stayed invisible because zoom defaults to 0, so every lane stretches
 * its own duration across the full pane width: a 10 s lane and a 60 s
 * session look identical, and the overlay looks plausible while meaning
 * something else. So these tests assert the seconds, not the pixels.
 */

import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

// A lane whose own audio is 10 seconds long, whatever the session says.
// `decode` fires as soon as it is subscribed to, which is what a real
// wavesurfer does once the file is in — the lane needs it to learn its
// own duration at all.
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
      isPlaying: vi.fn(() => false),
      destroy: vi.fn(),
      getDuration: () => 10,
      getCurrentTime: () => 0,
    }),
  },
}));

import { Timeline } from "../components/Timeline";

const PANE_WIDTH = 600;

/** A session 60 s long whose first track holds a 10 s clip. */
const MISMATCHED = [
  {
    index: 0,
    name: "voice",
    audioPath: "/tmp/voice.wav",
    muted: false,
    clips: [
      { start_sec: 0, length_sec: 10, source_path: "/tmp/voice.wav", volume_envelope: [] },
    ],
  },
  {
    index: 1,
    name: "music",
    audioPath: "/tmp/music.wav",
    muted: false,
    clips: [
      { start_sec: 0, length_sec: 60, source_path: "/tmp/music.wav", volume_envelope: [] },
    ],
  },
];

/**
 * jsdom gives every element a zero-size rect, so the drag maths would
 * divide by nothing. Pin the wrapper's geometry.
 */
function pinWidth() {
  Object.defineProperty(HTMLElement.prototype, "getBoundingClientRect", {
    configurable: true,
    value() {
      return {
        left: 0,
        top: 0,
        right: PANE_WIDTH,
        bottom: 92,
        width: PANE_WIDTH,
        height: 92,
        x: 0,
        y: 0,
        toJSON: () => ({}),
      };
    },
  });
  Object.defineProperty(HTMLElement.prototype, "clientWidth", {
    configurable: true,
    value: PANE_WIDTH,
  });
}

function dragAcross(fromPx: number, toPx: number) {
  // The wrapper that carries `onMouseDown`, i.e. the parent of the
  // wavesurfer container.
  const surface = screen.getAllByTestId("timeline-lane-waveform")[0]
    .parentElement as HTMLElement;
  fireEvent.mouseDown(surface, { button: 0, clientX: fromPx, clientY: 40 });
  fireEvent.mouseMove(window, { clientX: toPx, clientY: 40 });
  fireEvent.mouseUp(window, { clientX: toPx, clientY: 40 });
}

describe("Timeline selection axis", () => {
  it("reports session seconds when the first lane is shorter than the session", () => {
    pinWidth();
    const onSelectionChange = vi.fn();
    render(
      <Timeline
        tracks={MISMATCHED}
        selection={null}
        onSelectionChange={onSelectionChange}
      />,
    );

    // Drag across the first half of the pane.
    dragAcross(0, PANE_WIDTH / 2);

    expect(onSelectionChange).toHaveBeenCalled();
    const calls = onSelectionChange.mock.calls;
    const sel = calls[calls.length - 1]?.[0];
    expect(sel).not.toBeNull();
    // Half of a 60-second session is 30 s. Lane 0's own audio is 10 s,
    // so the old behaviour reported 5 — and exported 0–5 s of a
    // different track.
    expect(sel.start).toBeCloseTo(0, 3);
    expect(sel.end).toBeCloseTo(30, 1);
  });

  it("still measures against the audio itself for a single-file session", () => {
    pinWidth();
    const onSelectionChange = vi.fn();
    // No clip metadata: the only length available is the decoded one,
    // and for one loaded file it is also the session's length.
    render(
      <Timeline
        tracks={[{ index: 0, name: "solo", audioPath: "/tmp/solo.wav", muted: false }]}
        selection={null}
        onSelectionChange={onSelectionChange}
      />,
    );

    dragAcross(0, PANE_WIDTH / 2);

    const calls = onSelectionChange.mock.calls;
    const sel = calls[calls.length - 1]?.[0];
    expect(sel).not.toBeNull();
    expect(sel.end).toBeCloseTo(5, 1);
  });
});
