import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

/**
 * Looping belongs to whatever is actually playing, which since #155 is
 * the mix player rather than lane 0. `audioprocess` handlers are
 * collected per instance so the test can fire the one that matters and
 * assert on the player that would make the sound.
 */
const { mockSetTime, instances } = vi.hoisted(() => {
  const instances: {
    handlers: Record<string, (...args: unknown[]) => void>;
    setTime: ReturnType<typeof vi.fn>;
  }[] = [];
  const mockSetTime = vi.fn();
  return { mockSetTime, instances };
});

vi.mock("wavesurfer.js", () => ({
  default: {
    create: () => {
      const handlers: Record<string, (...args: unknown[]) => void> = {};
      const entry = { handlers, setTime: mockSetTime };
      instances.push(entry);
      return {
        on: (event: string, cb: (...args: unknown[]) => void) => {
          // Several handlers share `audioprocess`; keep the last, which
          // is the loop one.
          handlers[event] = cb;
        },
        un: vi.fn(),
        load: vi.fn(() => Promise.resolve()),
        zoom: vi.fn(),
        play: vi.fn(),
        pause: vi.fn(),
        seekTo: vi.fn(),
        setTime: mockSetTime,
        setVolume: vi.fn(),
        setOptions: vi.fn(),
        isPlaying: vi.fn(() => false),
        destroy: vi.fn(),
        getDuration: () => 10,
        getCurrentTime: () => 6,
      };
    },
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

  /**
   * The loop wraps on the **mix** player. It used to be handled by lane
   * 0, which is not the thing making sound any more — a loop that only
   * wrapped a silent lane would be a loop nobody could hear.
   */
  it("wraps to selection.start when the mix plays past selection.end", () => {
    instances.length = 0;
    mockSetTime.mockClear();
    render(
      <Timeline
        audioPath={null}
        mixPath="/tmp/mix.wav"
        loop={true}
        onLoopChange={vi.fn()}
        selection={{ start: 2, end: 5 }}
      />,
    );

    // The mix player is created by the parent, after the lanes.
    const mix = instances[instances.length - 1];
    // getCurrentTime returns 6, which is past end = 5.
    mix.handlers["audioprocess"]?.();

    expect(mockSetTime).toHaveBeenCalledWith(2);
  });

  /** With nothing to play, looping is silent rather than a crash. */
  it("does nothing when there is no mix yet", () => {
    instances.length = 0;
    mockSetTime.mockClear();
    render(
      <Timeline
        audioPath={null}
        loop={true}
        onLoopChange={vi.fn()}
        selection={null}
      />,
    );
    const mix = instances[instances.length - 1];
    expect(() => mix.handlers["audioprocess"]?.()).not.toThrow();
    expect(mockSetTime).not.toHaveBeenCalled();
  });
});
