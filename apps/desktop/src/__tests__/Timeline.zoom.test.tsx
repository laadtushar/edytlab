import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

const { mockZoom } = vi.hoisted(() => {
  const mockZoom = vi.fn();
  return { mockZoom };
});

vi.mock("wavesurfer.js", () => ({
  default: {
    create: () => ({
      on: vi.fn(),
      un: vi.fn(),
      load: vi.fn(),
      zoom: mockZoom,
      play: vi.fn(),
      pause: vi.fn(),
      seekTo: vi.fn(),
      destroy: vi.fn(),
      getDuration: () => 60,
      getCurrentTime: () => 0,
      setVolume: vi.fn(),
      setOptions: vi.fn(),
      setTime: vi.fn(),
      isPlaying: vi.fn(() => false),
    }),
  },
}));

import { Timeline } from "../components/Timeline";

describe("Timeline zoom", () => {
  it("renders zoom controls", () => {
    render(<Timeline audioPath={null} />);
    expect(screen.getByTestId("zoom-in-btn")).toBeInTheDocument();
    expect(screen.getByTestId("zoom-out-btn")).toBeInTheDocument();
  });

  it("calls onZoomChange when zoom-in clicked", async () => {
    const onZoomChange = vi.fn();
    render(<Timeline audioPath={null} zoom={50} onZoomChange={onZoomChange} />);
    await userEvent.click(screen.getByTestId("zoom-in-btn"));
    expect(onZoomChange).toHaveBeenCalled();
    const newZoom = onZoomChange.mock.calls[0][0];
    expect(newZoom).toBeGreaterThan(50);
  });

  it("calls onZoomChange when zoom-out clicked", async () => {
    const onZoomChange = vi.fn();
    render(<Timeline audioPath={null} zoom={50} onZoomChange={onZoomChange} />);
    await userEvent.click(screen.getByTestId("zoom-out-btn"));
    expect(onZoomChange).toHaveBeenCalled();
    const newZoom = onZoomChange.mock.calls[0][0];
    expect(newZoom).toBeLessThan(50);
  });
});
