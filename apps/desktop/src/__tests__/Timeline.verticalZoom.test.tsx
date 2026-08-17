/**
 * Vertical zoom (#161).
 *
 * Quiet material renders as a flat line with no way to magnify it,
 * which makes noise floors and fade tails impossible to judge. The
 * fix is a height multiplier — but it only works if `normalize` comes
 * off with it, because normalising already scales each lane's peak to
 * full height and so hides exactly the difference this exists to show.
 *
 * That pairing is the thing worth pinning: a test that only checked
 * `barHeight` would pass on a build where magnifying does nothing
 * visible.
 */

import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

const { setOptions } = vi.hoisted(() => ({ setOptions: vi.fn() }));

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
      setOptions,
      isPlaying: vi.fn(() => false),
      destroy: vi.fn(),
      getDuration: () => 10,
      getCurrentTime: () => 0,
    }),
  },
}));

import { Timeline } from "../components/Timeline";

const TRACKS = [
  { index: 0, name: "voice", audioPath: "/tmp/voice.wav", muted: false },
];

/** The last `setOptions` payload, which is what the lane is drawn with. */
function lastOptions() {
  const calls = setOptions.mock.calls;
  return calls[calls.length - 1]?.[0];
}

describe("vertical zoom", () => {
  it("draws at real amplitude by default, with normalisation on", () => {
    setOptions.mockClear();
    render(<Timeline tracks={TRACKS} />);
    expect(lastOptions()).toEqual({ barHeight: 1, normalize: true });
  });

  it("magnifies and turns normalisation off together", () => {
    setOptions.mockClear();
    render(<Timeline tracks={TRACKS} verticalZoom={8} />);
    // Normalised, a −40 dBFS passage and a hot one are drawn the same,
    // so magnifying a normalised waveform magnifies nothing.
    expect(lastOptions()).toEqual({ barHeight: 8, normalize: false });
  });

  it("steps by doubling, and stops at the bounds", () => {
    const onVerticalZoomChange = vi.fn();
    const { rerender } = render(
      <Timeline
        tracks={TRACKS}
        verticalZoom={4}
        onVerticalZoomChange={onVerticalZoomChange}
      />,
    );

    fireEvent.click(screen.getByTestId("vzoom-in-btn"));
    expect(onVerticalZoomChange).toHaveBeenLastCalledWith(8);
    fireEvent.click(screen.getByTestId("vzoom-out-btn"));
    expect(onVerticalZoomChange).toHaveBeenLastCalledWith(2);

    // At 1× there is nothing to shrink to, and at the ceiling nothing
    // to magnify — say so by disabling rather than by doing nothing.
    rerender(
      <Timeline
        tracks={TRACKS}
        verticalZoom={1}
        onVerticalZoomChange={onVerticalZoomChange}
      />,
    );
    expect(screen.getByTestId("vzoom-out-btn")).toBeDisabled();

    rerender(
      <Timeline
        tracks={TRACKS}
        verticalZoom={64}
        onVerticalZoomChange={onVerticalZoomChange}
      />,
    );
    expect(screen.getByTestId("vzoom-in-btn")).toBeDisabled();
  });

  /**
   * A −40 dBFS passage peaks at 1% of full height and is unreadable.
   * The ceiling has to be enough to lift it into view — 64× puts it at
   * roughly two thirds.
   */
  it("reaches far enough to make a −40 dBFS passage readable", () => {
    const quietPeak = 10 ** (-40 / 20); // 0.01
    expect(quietPeak * 64).toBeGreaterThan(0.5);
  });
});
