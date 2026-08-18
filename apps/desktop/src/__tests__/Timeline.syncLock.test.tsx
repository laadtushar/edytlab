/**
 * The sync-lock toggle (#170 §3).
 *
 * The acceptance asks for the state to be *visible*, not parked in a
 * menu — because sync-lock silently changes what the next cut does, and
 * a mode you cannot see is worse than no mode at all. So these tests
 * are about the control saying what it is: pressed when on, not pressed
 * when off, and reporting the state it is moving *to* when clicked.
 */

import { fireEvent, render, screen } from "@testing-library/react";
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
      setOptions: vi.fn(),
      isPlaying: vi.fn(() => false),
      destroy: vi.fn(),
      getDuration: () => 0,
      getCurrentTime: () => 0,
      getDecodedData: () => null,
    }),
  },
}));

import { Timeline } from "../components/Timeline";

const TRACKS = [
  {
    index: 0,
    name: "host",
    audioPath: "/tmp/host.wav",
    muted: false,
    clips: [
      {
        start_sec: 0,
        length_sec: 10,
        source_path: "/tmp/host.wav",
        volume_envelope: [],
      },
    ],
  },
  {
    index: 1,
    name: "guest",
    audioPath: "/tmp/guest.wav",
    muted: false,
    clips: [
      {
        start_sec: 0,
        length_sec: 10,
        source_path: "/tmp/guest.wav",
        volume_envelope: [],
      },
    ],
  },
];

describe("sync-lock toggle", () => {
  it("is off and says so when the session has it off", () => {
    render(<Timeline tracks={TRACKS} syncLock={false} />);
    const btn = screen.getByTestId("sync-lock-btn");
    expect(btn.getAttribute("aria-pressed")).toBe("false");
    expect(btn.getAttribute("aria-label")).toBe("Turn sync-lock on");
  });

  it("reads as pressed when the session has it on", () => {
    render(<Timeline tracks={TRACKS} syncLock={true} />);
    const btn = screen.getByTestId("sync-lock-btn");
    expect(btn.getAttribute("aria-pressed")).toBe("true");
    expect(btn.getAttribute("aria-label")).toBe("Turn sync-lock off");
  });

  it("asks for the opposite of what it currently shows", () => {
    const onChange = vi.fn();
    const { rerender } = render(
      <Timeline tracks={TRACKS} syncLock={false} onSyncLockChange={onChange} />,
    );
    fireEvent.click(screen.getByTestId("sync-lock-btn"));
    expect(onChange).toHaveBeenCalledWith(true);

    rerender(
      <Timeline tracks={TRACKS} syncLock={true} onSyncLockChange={onChange} />,
    );
    fireEvent.click(screen.getByTestId("sync-lock-btn"));
    expect(onChange).toHaveBeenLastCalledWith(false);
  });

  /**
   * The state is the session's, not the button's. Clicking must not
   * flip the visual on its own — the session says whether it took, and
   * undo past a sync-lock node has to be able to move it back.
   */
  it("does not change its own appearance without the session agreeing", () => {
    render(<Timeline tracks={TRACKS} syncLock={false} onSyncLockChange={vi.fn()} />);
    const btn = screen.getByTestId("sync-lock-btn");
    fireEvent.click(btn);
    expect(btn.getAttribute("aria-pressed")).toBe("false");
  });
});
