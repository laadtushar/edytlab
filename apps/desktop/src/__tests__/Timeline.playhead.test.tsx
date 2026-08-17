/**
 * The playhead is drawn on the session axis, by the lane (#155, step 3).
 *
 * WaveSurfer's own cursor cannot do this. `setTime` clamps to the
 * lane's media duration, so a 10-second lane asked to show t=30 pins at
 * 10 and a lane with no audio pins at 0. And at zoom 0 every lane
 * stretches its own duration across the full width, so the same x means
 * a different time on every lane.
 *
 * So the test asserts the two things that follow from doing it
 * properly: every lane puts the playhead at the same place, and that
 * place is the session's fraction rather than the lane's.
 */

import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

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
      getDuration: () => 10,
      getCurrentTime: () => 0,
    }),
  },
}));

import { Timeline } from "../components/Timeline";

const PANE_WIDTH = 600;
/** The wrapper's horizontal padding, which the overlay also offsets by. */
const PAD = 12;

function pinWidth() {
  Object.defineProperty(HTMLElement.prototype, "clientWidth", {
    configurable: true,
    value: PANE_WIDTH,
  });
}

/** Lane 0 is 10 s of a 60 s session; lane 1 runs the whole length. */
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

describe("playhead", () => {
  it("draws exactly one playhead per lane", () => {
    pinWidth();
    render(<Timeline tracks={MISMATCHED} />);
    expect(screen.getAllByTestId("timeline-playhead")).toHaveLength(
      MISMATCHED.length,
    );
  });

  it("sits at the same place on every lane, on the session axis", () => {
    pinWidth();
    // A Timeline with no transport publishes 0; drive it directly by
    // rendering a lane-level playhead through the public prop instead.
    render(<Timeline tracks={MISMATCHED} />);

    const heads = screen.getAllByTestId("timeline-playhead");
    expect(heads).toHaveLength(2);
    // Both lanes agree, which is the property that was missing: lane 0
    // is a sixth of the session and would otherwise draw t=0 at the
    // same pixel as t=0 but t=5 six times too far along.
    expect(heads[0].style.left).toBe(heads[1].style.left);
    expect(heads[0].style.left).toBe(`${PAD}px`);
  });

  /**
   * Halfway through a 60-second session is the middle of the pane on
   * *every* lane — including the one whose own audio ended 20 seconds
   * ago. That is exactly the case a clamped cursor gets wrong.
   */
  it("places a mid-session time by the session's fraction", () => {
    pinWidth();
    const { container } = render(
      <TimelineWithPlayhead seconds={30} tracks={MISMATCHED} />,
    );
    const heads = Array.from(
      container.querySelectorAll<HTMLElement>(
        "[data-testid='timeline-playhead']",
      ),
    );
    expect(heads).toHaveLength(2);
    for (const h of heads) {
      expect(h.style.left).toBe(`${PANE_WIDTH / 2 + PAD}px`);
    }
  });

  it("clamps past the end rather than running off the pane", () => {
    pinWidth();
    const { container } = render(
      <TimelineWithPlayhead seconds={999} tracks={MISMATCHED} />,
    );
    const head = container.querySelector<HTMLElement>(
      "[data-testid='timeline-playhead']",
    );
    expect(head?.style.left).toBe(`${PANE_WIDTH + PAD}px`);
  });
});

/**
 * The lane takes `playheadSec` directly; Timeline normally supplies it
 * from the transport. Rendering the lane through Timeline with an
 * explicit value is the only way to test a position without a real
 * player, and it exercises the same prop the transport feeds.
 */
function TimelineWithPlayhead({
  seconds,
  tracks,
}: {
  seconds: number;
  tracks: typeof MISMATCHED;
}) {
  return <Timeline tracks={tracks} playheadSec={seconds} />;
}
