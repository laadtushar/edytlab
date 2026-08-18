/**
 * Mixer controls on the track lanes (#94).
 *
 * The point of these is not that the sliders exist — it is that they
 * address the *session's* track index and that they persist. Before
 * this, the mute button flipped React state and nothing else: a mute
 * set here never reached the session, and a mute set by the agent
 * never reached the button.
 */

import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

const { mockSetVolume } = vi.hoisted(() => ({ mockSetVolume: vi.fn() }));

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
      setVolume: mockSetVolume,
      setOptions: vi.fn(),
      isPlaying: vi.fn(() => false),
      destroy: vi.fn(),
      getDuration: () => 10,
      getCurrentTime: () => 0,
    }),
  },
}));

import { Timeline } from "../components/Timeline";

const TRACKS = [
  {
    index: 0,
    name: "drums",
    audioPath: "/tmp/drums.wav",
    muted: false,
    gainDb: 0,
    pan: 0,
    soloed: false,
  },
  {
    // Session track 2 — track 1 has no audio and was filtered out
    // upstream, so this lane is the second drawn but the third stored.
    index: 2,
    name: "bass",
    audioPath: "/tmp/bass.wav",
    muted: true,
    gainDb: -6,
    pan: -0.5,
    soloed: false,
  },
];

function lanes() {
  return screen.getAllByTestId("timeline-lane-sidebar");
}

describe("Timeline mixer controls", () => {
  it("renders gain, pan, mute and solo per lane", () => {
    render(<Timeline tracks={TRACKS} audioPath="/tmp/drums.wav" />);
    expect(screen.getAllByTestId("timeline-lane-gain")).toHaveLength(2);
    expect(screen.getAllByTestId("timeline-lane-pan")).toHaveLength(2);
    expect(screen.getAllByTestId("timeline-lane-mute")).toHaveLength(2);
    expect(screen.getAllByTestId("timeline-lane-solo")).toHaveLength(2);
  });

  it("shows each lane's own values from the session", () => {
    render(<Timeline tracks={TRACKS} audioPath="/tmp/drums.wav" />);
    const second = lanes()[1];
    expect(
      within(second).getByTestId("timeline-lane-gain-readout"),
    ).toHaveTextContent("-6.0 dB");
    // -0.5 reads as "L50", not as a bare number.
    expect(
      within(second).getByTestId("timeline-lane-pan-readout"),
    ).toHaveTextContent("L50");
    expect(within(second).getByTestId("timeline-lane-mute")).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });

  it("commits mute against the session track index, not the lane index", async () => {
    const onTrackMuteChange = vi.fn();
    render(
      <Timeline
        tracks={TRACKS}
        audioPath="/tmp/drums.wav"
        onTrackMuteChange={onTrackMuteChange}
      />,
    );
    await userEvent.click(
      within(lanes()[1]).getByTestId("timeline-lane-mute"),
    );
    // Lane 1, session track 2. Getting this wrong mutes the wrong track.
    expect(onTrackMuteChange).toHaveBeenCalledWith(2, false);
  });

  it("commits solo against the session track index", async () => {
    const onTrackSoloChange = vi.fn();
    render(
      <Timeline
        tracks={TRACKS}
        audioPath="/tmp/drums.wav"
        onTrackSoloChange={onTrackSoloChange}
      />,
    );
    await userEvent.click(
      within(lanes()[1]).getByTestId("timeline-lane-solo"),
    );
    expect(onTrackSoloChange).toHaveBeenCalledWith(2, true);
  });

  it("does not write a session node while the fader is still moving", () => {
    const onTrackGainChange = vi.fn();
    render(
      <Timeline
        tracks={TRACKS}
        audioPath="/tmp/drums.wav"
        onTrackGainChange={onTrackGainChange}
      />,
    );
    const fader = within(lanes()[0]).getByTestId("timeline-lane-gain");

    fireEvent.change(fader, { target: { value: "-3" } });
    fireEvent.change(fader, { target: { value: "-9" } });
    expect(onTrackGainChange).not.toHaveBeenCalled();

    // …but the readout follows, so the drag is not silent.
    expect(
      within(lanes()[0]).getByTestId("timeline-lane-gain-readout"),
    ).toHaveTextContent("-9.0 dB");

    fireEvent.pointerUp(fader);
    expect(onTrackGainChange).toHaveBeenCalledTimes(1);
    expect(onTrackGainChange).toHaveBeenCalledWith(0, -9);
  });

  it("commits pan on release and centres on double-click", () => {
    const onTrackPanChange = vi.fn();
    render(
      <Timeline
        tracks={TRACKS}
        audioPath="/tmp/drums.wav"
        onTrackPanChange={onTrackPanChange}
      />,
    );
    const pan = within(lanes()[0]).getByTestId("timeline-lane-pan");

    fireEvent.change(pan, { target: { value: "0.6" } });
    fireEvent.pointerUp(pan);
    expect(onTrackPanChange).toHaveBeenCalledWith(0, 0.6);
    expect(
      within(lanes()[0]).getByTestId("timeline-lane-pan-readout"),
    ).toHaveTextContent("R60");

    fireEvent.doubleClick(pan);
    expect(onTrackPanChange).toHaveBeenLastCalledWith(0, 0);
    expect(
      within(lanes()[0]).getByTestId("timeline-lane-pan-readout"),
    ).toHaveTextContent("C");
  });

  it("is reachable and adjustable from the keyboard", () => {
    const onTrackGainChange = vi.fn();
    render(
      <Timeline
        tracks={TRACKS}
        audioPath="/tmp/drums.wav"
        onTrackGainChange={onTrackGainChange}
      />,
    );
    const fader = within(lanes()[0]).getByTestId("timeline-lane-gain");
    fader.focus();
    expect(fader).toHaveFocus();
    expect(fader).toHaveAccessibleName("drums gain in decibels");

    // jsdom does not move a range input on arrow keys, so drive the
    // value change the browser would and assert the commit fires on
    // key release rather than needing a pointer.
    fireEvent.change(fader, { target: { value: "0.5" } });
    fireEvent.keyUp(fader, { key: "ArrowUp" });
    expect(onTrackGainChange).toHaveBeenCalledWith(0, 0.5);
  });

  it("reconciles from the session rather than keeping the local toggle", () => {
    const { rerender } = render(
      <Timeline
        tracks={TRACKS}
        audioPath="/tmp/drums.wav"
        onTrackMuteChange={vi.fn()}
      />,
    );
    // Optimistic flip.
    fireEvent.click(within(lanes()[0]).getByTestId("timeline-lane-mute"));
    expect(within(lanes()[0]).getByTestId("timeline-lane-mute")).toHaveAttribute(
      "aria-pressed",
      "true",
    );

    // The refresh says the session did not take it — e.g. the command
    // was rejected. The button must follow the session, not its own
    // earlier guess.
    rerender(
      <Timeline
        tracks={TRACKS.map((t) => ({ ...t }))}
        audioPath="/tmp/drums.wav"
        onTrackMuteChange={vi.fn()}
      />,
    );
    expect(within(lanes()[0]).getByTestId("timeline-lane-mute")).toHaveAttribute(
      "aria-pressed",
      "false",
    );
  });

  /**
   * A lane makes no sound (#155), so its volume is not a preview of
   * anything — it is pinned to zero so that a lane played by some
   * future change is silent rather than quietly wrong.
   *
   * This test used to assert the opposite: that a −6 dB fader set the
   * lane's volume to ~0.501. That was a real preview when lane 0 was
   * the transport. It is not one now — what you hear is the rendered
   * mix, and a fader move is audible after the next render, which the
   * status bar's stale-mix indicator is there to say.
   */
  it("leaves the lanes silent, whatever the fader says", () => {
    mockSetVolume.mockClear();
    render(
      <Timeline
        tracks={[{ ...TRACKS[0], gainDb: -6 }]}
        audioPath="/tmp/drums.wav"
      />,
    );
    const called = mockSetVolume.mock.calls.map((c) => c[0] as number);
    expect(called.length).toBeGreaterThan(0);
    expect(
      called.every((v) => v === 0),
      `a lane must never be audible; got ${JSON.stringify(called)}`,
    ).toBe(true);
  });
});
