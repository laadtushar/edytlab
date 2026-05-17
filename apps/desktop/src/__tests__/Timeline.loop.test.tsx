import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

vi.mock("wavesurfer.js", () => ({
  default: {
    create: () => ({
      on: vi.fn(),
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
});
