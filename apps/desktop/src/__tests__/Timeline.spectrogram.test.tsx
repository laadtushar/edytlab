import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { Timeline } from "../components/Timeline";

vi.mock("wavesurfer.js", () => ({
  default: {
    create: vi.fn(() => ({
      load: vi.fn(),
      on: vi.fn(),
      un: vi.fn(),
      destroy: vi.fn(),
      zoom: vi.fn(),
      setVolume: vi.fn(),
      setOptions: vi.fn(),
      registerPlugin: vi.fn(),
      getDuration: vi.fn(() => 0),
      getCurrentTime: vi.fn(() => 0),
      isPlaying: vi.fn(() => false),
    })),
  },
}));
vi.mock("wavesurfer.js/dist/plugins/spectrogram.esm.js", () => ({
  default: { create: vi.fn(() => ({ destroy: vi.fn() })) },
}));
vi.mock("wavesurfer.js/dist/plugins/timeline.esm.js", () => ({
  default: { create: vi.fn(() => ({})) },
}));

describe("Timeline spectrogram toggle", () => {
  it("renders spectrogram button", () => {
    render(
      <Timeline
        src={null}
        selection={null}
        onSelectionChange={() => {}}
        zoom={1}
        loop={false}
        onLoopChange={() => {}}
        spectrogramEnabled={false}
        onSpectrogramChange={() => {}}
      />
    );
    expect(screen.getByTestId("spectrogram-btn")).toBeDefined();
  });

  it("calls onSpectrogramChange when button clicked", () => {
    const onChange = vi.fn();
    render(
      <Timeline
        src={null}
        selection={null}
        onSelectionChange={() => {}}
        zoom={1}
        loop={false}
        onLoopChange={() => {}}
        spectrogramEnabled={false}
        onSpectrogramChange={onChange}
      />
    );
    fireEvent.click(screen.getByTestId("spectrogram-btn"));
    expect(onChange).toHaveBeenCalledWith(true);
  });
});
