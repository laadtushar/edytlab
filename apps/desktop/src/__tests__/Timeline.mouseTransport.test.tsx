/**
 * Play, pause and place the playhead with the mouse (#425).
 *
 * The transport was keyboard-only: Space toggled it, Home/End and the
 * arrows moved it, and nothing on screen did either. A click on a lane
 * only cleared the selection, so the one way to start playback from a
 * point was to press ←/→ until the playhead got there.
 *
 * What is asserted is which player each gesture reaches and with what
 * time: a button that toggled its own state, or a click that moved one
 * lane's playhead, would look right and play nothing.
 */

import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { createRef } from "react";
import { describe, expect, it, vi } from "vitest";

const { instances } = vi.hoisted(() => {
  const instances: {
    url: string | null;
    currentTime: number;
    playing: boolean;
    play: ReturnType<typeof vi.fn>;
    pause: ReturnType<typeof vi.fn>;
    setTime: ReturnType<typeof vi.fn>;
    /** Fire one of the player's events, as its media element would. */
    emit: (event: string, ...args: unknown[]) => void;
  }[] = [];
  return { instances };
});

vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  convertFileSrc: (path: string) => `asset://${path}`,
}));

vi.mock("wavesurfer.js", async () => {
  const { eventedPlayer } = await import("./wavesurferStub");
  return { default: { create: () => eventedPlayer(instances) } };
});

import { Timeline, type Selection, type TimelineHandle } from "../components/Timeline";

const PANE_WIDTH = 600;
/** Seconds per pixel at fit: a 60-second session across 600 px. */
const SEC_PER_PX = 60 / PANE_WIDTH;

const clip = (path: string) => ({
  start_sec: 0,
  length_sec: 60,
  source_path: path,
  volume_envelope: [],
});

/** Two tracks, the session 60 s long. */
const TRACKS = [
  { index: 0, name: "voice", audioPath: "/tmp/voice.wav", muted: false, clips: [clip("/tmp/voice.wav")] },
  { index: 1, name: "music", audioPath: "/tmp/music.wav", muted: false, clips: [clip("/tmp/music.wav")] },
];

/** jsdom lays nothing out; give every surface the pane's width. */
function pinWidth() {
  Object.defineProperty(HTMLElement.prototype, "getBoundingClientRect", {
    configurable: true,
    value() {
      return {
        left: 0,
        top: 0,
        right: PANE_WIDTH,
        bottom: 92,
        width: PANE_WIDTH,
        height: 92,
        x: 0,
        y: 0,
        toJSON: () => ({}),
      };
    },
  });
  Object.defineProperty(HTMLElement.prototype, "clientWidth", {
    configurable: true,
    value: PANE_WIDTH,
  });
}

const MIX = "/tmp/mix.wav";
const holding = (path: string) => instances.find((i) => i.url === `asset://${path}`);

async function mount({
  mixPath = MIX as string | null,
  selection = null as Selection | null,
} = {}) {
  pinWidth();
  instances.length = 0;
  const ref = createRef<TimelineHandle>();
  const onSelectionChange = vi.fn();
  const view = render(
    <Timeline
      ref={ref}
      tracks={TRACKS}
      mixPath={mixPath}
      selection={selection}
      onSelectionChange={onSelectionChange}
    />,
  );
  const button = screen.getByTestId("play-pause-button");
  if (mixPath) {
    // The mix loads onto one of the two players, which takes the
    // transport once the load settles; the button waits for that.
    await waitFor(() => expect(button).toBeEnabled());
  }
  return { ref, view, button, onSelectionChange, mix: mixPath ? holding(mixPath)! : null };
}

function surface(lane: number): HTMLElement {
  return screen.getAllByTestId("timeline-lane-surface")[lane];
}

function click(lane: number, x: number) {
  fireEvent.mouseDown(surface(lane), { button: 0, clientX: x, clientY: 40 });
  fireEvent.mouseUp(window, { clientX: x, clientY: 40 });
}

function drag(lane: number, from: number, to: number) {
  fireEvent.mouseDown(surface(lane), { button: 0, clientX: from, clientY: 40 });
  fireEvent.mouseMove(window, { clientX: to, clientY: 40 });
  fireEvent.mouseUp(window, { clientX: to, clientY: 40 });
}

/** What every lane's playhead says, in session seconds and pixels. */
function playheads() {
  return screen.getAllByTestId("timeline-playhead").map((h) => ({
    sec: Number(h.dataset.playheadSec),
    left: h.style.left,
  }));
}

describe("the play button", () => {
  it("is disabled, and plays nothing, until a mix has loaded", async () => {
    const { button } = await mount({ mixPath: null });

    expect(button).toBeDisabled();
    expect(button).toHaveAccessibleName("Play");
    fireEvent.click(button);
    for (const player of instances) {
      expect(player.play).not.toHaveBeenCalled();
    }
  });

  it("plays the mix, says so, and pauses it again", async () => {
    const { button, mix } = await mount();
    expect(button).toHaveAccessibleName("Play");

    fireEvent.click(button);

    expect(mix!.play).toHaveBeenCalledTimes(1);
    await screen.findByRole("button", { name: "Pause" });
    for (const player of instances.filter((p) => p !== mix)) {
      expect(player.play, "only the mix is ever played").not.toHaveBeenCalled();
    }

    fireEvent.click(button);

    expect(mix!.pause).toHaveBeenCalledTimes(1);
    await screen.findByRole("button", { name: "Play" });
  });

  it("follows the transport when Space starts it, and when the mix ends", async () => {
    const { ref, mix } = await mount();

    // App's Space handler calls exactly this.
    act(() => ref.current?.togglePlay());
    await screen.findByRole("button", { name: "Pause" });

    // Played to the end: the media element stops and says so.
    act(() => {
      mix!.playing = false;
      mix!.emit("finish");
    });
    await screen.findByRole("button", { name: "Play" });
  });

  it("stays on Pause through an A/B switch, while the old side stops", async () => {
    const { view, button, mix: a } = await mount({ mixPath: "/tmp/a.wav" });
    fireEvent.click(button);
    await screen.findByRole("button", { name: "Pause" });

    view.rerender(
      <Timeline tracks={TRACKS} mixPath="/tmp/b.wav" selection={null} onSelectionChange={vi.fn()} />,
    );

    // The crossfade ends with the outgoing side paused. That pause is
    // not the transport stopping: side B plays on.
    await waitFor(() => expect(a!.pause).toHaveBeenCalled());
    expect(holding("/tmp/b.wav")!.playing).toBe(true);
    expect(screen.getByTestId("play-pause-button")).toHaveAccessibleName("Pause");
  });
});

describe("a click on a lane", () => {
  it("puts every lane's playhead at the time under the pointer", async () => {
    const { mix } = await mount();

    click(0, 150);

    expect(mix!.setTime).toHaveBeenLastCalledWith(150 * SEC_PER_PX);
    await waitFor(() =>
      expect(playheads()).toEqual([
        { sec: 15, left: "150px" },
        { sec: 15, left: "150px" },
      ]),
    );
  });

  it("works on a lane that is not the first, which cannot select", async () => {
    const { mix, onSelectionChange } = await mount();

    click(1, 450);

    expect(mix!.setTime).toHaveBeenLastCalledWith(450 * SEC_PER_PX);
    await waitFor(() => expect(playheads().map((p) => p.sec)).toEqual([45, 45]));
    expect(onSelectionChange).not.toHaveBeenCalled();
  });

  it("clears the selection as well", async () => {
    const { mix, onSelectionChange } = await mount({ selection: { start: 10, end: 20 } });

    click(0, 450);

    expect(onSelectionChange).toHaveBeenCalledWith(null);
    expect(mix!.setTime).toHaveBeenLastCalledWith(450 * SEC_PER_PX);
  });

  it("while playing, seeks and plays on from there", async () => {
    const { button, mix } = await mount();
    fireEvent.click(button);
    await screen.findByRole("button", { name: "Pause" });

    click(0, 300);

    expect(mix!.setTime).toHaveBeenLastCalledWith(300 * SEC_PER_PX);
    expect(mix!.pause).not.toHaveBeenCalled();
    expect(mix!.playing).toBe(true);
    expect(screen.getByTestId("play-pause-button")).toHaveAccessibleName("Pause");
  });

  it("does nothing to the transport while there is no mix", async () => {
    const { onSelectionChange } = await mount({ mixPath: null });

    expect(() => click(0, 150)).not.toThrow();

    for (const player of instances) {
      expect(player.setTime).not.toHaveBeenCalled();
    }
    // A click still clears the selection; only the seek has nowhere to go.
    expect(onSelectionChange).toHaveBeenCalledWith(null);
  });
});

describe("a drag on a lane", () => {
  it("still selects, and does not move the playhead", async () => {
    const { mix, onSelectionChange } = await mount();

    drag(0, 0, 300);

    expect(onSelectionChange).toHaveBeenLastCalledWith({ start: 0, end: 30 });
    expect(mix!.setTime).not.toHaveBeenCalled();
  });

  it("fades its selection in when it mounts, and never eases its position", async () => {
    // The overlay sits over the canvas, not in it, and follows the
    // pointer: `left` and `width` are data, so only its arrival is eased.
    // (jsdom computes no animation; this is the class, not the motion.)
    await mount();

    fireEvent.mouseDown(surface(0), { button: 0, clientX: 0, clientY: 40 });
    fireEvent.mouseMove(window, { clientX: 300, clientY: 40 });

    const overlay = screen.getByTestId("timeline-selection-overlay");
    expect(overlay.className).toContain("fade-in-quick");
    expect(overlay.style.transition).toBe("");
    fireEvent.mouseUp(window, { clientX: 300, clientY: 40 });
  });

  it("on a lane that cannot select, draws no selection and seeks nowhere", async () => {
    const { mix, onSelectionChange } = await mount();

    fireEvent.mouseDown(surface(1), { button: 0, clientX: 0, clientY: 40 });
    fireEvent.mouseMove(window, { clientX: 300, clientY: 40 });
    expect(screen.queryByTestId("timeline-selection-overlay")).toBeNull();
    fireEvent.mouseUp(window, { clientX: 300, clientY: 40 });

    expect(onSelectionChange).not.toHaveBeenCalled();
    expect(mix!.setTime).not.toHaveBeenCalled();
  });
});
