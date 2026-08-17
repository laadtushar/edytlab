/**
 * Zoom to selection and fit to window (#161).
 *
 * These are the two most-used zoom verbs on any timeline and neither
 * existed: zoom was ± and reset only, so getting to a selected region
 * meant zooming and then scrolling to find it by hand.
 *
 * The assertion that matters is the *scale*: zoom-to-selection has to
 * ask for the pixels-per-second that makes the selection exactly fill
 * the pane. A button that merely changed the zoom by some amount would
 * look like it worked.
 */

import { fireEvent, render, screen } from "@testing-library/react";
import { createRef } from "react";
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
      isPlaying: vi.fn(() => false),
      destroy: vi.fn(),
      getDuration: () => 60,
      getCurrentTime: () => 0,
    }),
  },
}));

import { Timeline, type TimelineHandle } from "../components/Timeline";

const PANE_WIDTH = 600;

function pinWidth() {
  Object.defineProperty(HTMLElement.prototype, "clientWidth", {
    configurable: true,
    value: PANE_WIDTH,
  });
}

const TRACKS = [
  { index: 0, name: "voice", audioPath: "/tmp/voice.wav", muted: false },
];

describe("Timeline zoom verbs", () => {
  it("zooms so the selection exactly fills the pane", () => {
    pinWidth();
    const onZoomChange = vi.fn();
    render(
      <Timeline
        tracks={TRACKS}
        selection={{ start: 10, end: 15 }}
        onZoomChange={onZoomChange}
      />,
    );

    fireEvent.click(screen.getByTestId("zoom-to-selection-btn"));

    // A 5-second selection across a 600 px pane is 120 px/sec. Anything
    // else frames a different region.
    expect(onZoomChange).toHaveBeenCalledWith(120);
  });

  it("does nothing without a selection, and says so by being disabled", () => {
    pinWidth();
    const onZoomChange = vi.fn();
    render(
      <Timeline tracks={TRACKS} selection={null} onZoomChange={onZoomChange} />,
    );

    const btn = screen.getByTestId("zoom-to-selection-btn");
    expect(btn).toBeDisabled();
    fireEvent.click(btn);
    expect(onZoomChange).not.toHaveBeenCalled();
  });

  it("fit to window asks for auto-fit", () => {
    pinWidth();
    const onZoomChange = vi.fn();
    render(
      <Timeline
        tracks={TRACKS}
        selection={{ start: 10, end: 15 }}
        onZoomChange={onZoomChange}
      />,
    );

    fireEvent.click(screen.getByTestId("fit-to-window-btn"));
    // Zero is the auto-fit sentinel the lanes already understand.
    expect(onZoomChange).toHaveBeenCalledWith(0);
  });

  /**
   * The keyboard path goes through the imperative handle, which is what
   * App's Ctrl+E / Ctrl+F call. Worth pinning separately: a handle that
   * silently lost a method would leave the shortcuts dead while the
   * buttons kept working.
   */
  it("exposes both verbs on the imperative handle", () => {
    pinWidth();
    const onZoomChange = vi.fn();
    const ref = createRef<TimelineHandle>();
    render(
      <Timeline
        ref={ref}
        tracks={TRACKS}
        selection={{ start: 0, end: 30 }}
        onZoomChange={onZoomChange}
      />,
    );

    ref.current?.zoomToSelection();
    expect(onZoomChange).toHaveBeenCalledWith(20);

    ref.current?.fitToWindow();
    expect(onZoomChange).toHaveBeenLastCalledWith(0);
  });
});
