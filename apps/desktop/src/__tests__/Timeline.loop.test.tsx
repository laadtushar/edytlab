import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

const { mockSeekTo, mockCallbacks } = vi.hoisted(() => {
  const mockCallbacks: Record<string, (...args: unknown[]) => void> = {};
  const mockSeekTo = vi.fn();
  return { mockSeekTo, mockCallbacks };
});

vi.mock("wavesurfer.js", () => ({
  default: {
    create: () => ({
      on: (event: string, cb: (...args: unknown[]) => void) => {
        mockCallbacks[event] = cb;
      },
      un: vi.fn(),
      load: vi.fn(),
      zoom: vi.fn(),
      play: vi.fn(),
      pause: vi.fn(),
      seekTo: mockSeekTo,
      setTime: vi.fn(),
      setVolume: vi.fn(),
      isPlaying: vi.fn(() => false),
      destroy: vi.fn(),
      getDuration: () => 10,
      getCurrentTime: () => 6,
    }),
  },
}));

import { Timeline } from "../components/Timeline";

describe("Timeline loop toggle", () => {
  it("renders loop button", () => {
    render(
      <Timeline
        audioPath={null}
        loop={false}
        onLoopChange={vi.fn()}
        selection={{ start: 1, end: 5 }}
      />,
    );
    expect(screen.getByTestId("loop-btn")).toBeInTheDocument();
  });

  it("calls onLoopChange when loop button clicked", async () => {
    const onLoopChange = vi.fn();
    render(
      <Timeline
        audioPath={null}
        loop={false}
        onLoopChange={onLoopChange}
        selection={{ start: 1, end: 5 }}
      />,
    );
    await userEvent.click(screen.getByTestId("loop-btn"));
    expect(onLoopChange).toHaveBeenCalledWith(true);
  });

  it("seeks to selection.start when audioprocess fires past selection.end", () => {
    render(
      <Timeline
        audioPath={null}
        loop={true}
        onLoopChange={vi.fn()}
        selection={{ start: 2, end: 5 }}
      />,
    );
    // Fire the audioprocess callback (getCurrentTime returns 6, which is >= end=5)
    mockCallbacks["audioprocess"]?.();
    // seekTo should be called with start/duration = 2/10 = 0.2
    expect(mockSeekTo).toHaveBeenCalledWith(0.2);
  });
});
