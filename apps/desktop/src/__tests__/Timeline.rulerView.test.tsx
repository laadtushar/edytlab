/**
 * The lane has to tell the ruler what is on screen (#323).
 *
 * The numbers here are the ones from the report: a 3-second file at
 * 500 px/s draws ~1500px of waveform inside a ~775px pane, so a little
 * over 1.5 seconds is visible at a time.
 *
 * Measured from WaveSurfer, not from the lane's own elements —
 * WaveSurfer renders into a `.scroll` container inside a shadow root,
 * and that is what scrolls, not the wrapper this component owns. The
 * existing zoom-to-selection code sets `el.parentElement.scrollLeft`,
 * which is the wrapper, and so scrolls nothing; `getScroll`,
 * `getWidth` and `getWrapper` are the public way to ask.
 *
 * Both events are covered because neither is enough on its own.
 * `scroll` comes from the container's own scroll event, so zooming
 * without panning — the reported bug — emits nothing at all; `redraw`
 * follows every draw but not a pan.
 */

import { render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

const { handlers, state } = vi.hoisted(() => {
  const handlers = new Map<string, Set<(...a: unknown[]) => void>>();
  const state = { scroll: 0, total: 1500, visible: 775, duration: 3 };
  return { handlers, state };
});

function emit(event: string, ...args: unknown[]) {
  for (const fn of handlers.get(event) ?? []) fn(...args);
}

vi.mock("wavesurfer.js", () => ({
  default: {
    create: () => ({
      on: (event: string, fn: (...a: unknown[]) => void) => {
        if (!handlers.has(event)) handlers.set(event, new Set());
        handlers.get(event)!.add(fn);
      },
      un: (event: string, fn: (...a: unknown[]) => void) => {
        handlers.get(event)?.delete(fn);
      },
      load: vi.fn(() => Promise.resolve()),
      zoom: vi.fn(),
      play: vi.fn(),
      pause: vi.fn(),
      seekTo: vi.fn(),
      destroy: vi.fn(),
      getDuration: () => state.duration,
      getCurrentTime: () => 0,
      getScroll: () => state.scroll,
      getWidth: () => state.visible,
      getWrapper: () => ({ scrollWidth: state.total }),
      setVolume: vi.fn(),
      setOptions: vi.fn(),
      setTime: vi.fn(),
      isPlaying: vi.fn(() => false),
    }),
  },
}));

import { Timeline } from "../components/Timeline";

function labels(): string[] {
  return Array.from(screen.getByTestId("ruler").querySelectorAll("span"))
    .map((el) => el.textContent ?? "")
    .filter(Boolean);
}

function times(): number[] {
  return labels().map((l) => {
    const [m, s] = l.split(":");
    return Number(m) * 60 + Number.parseFloat(s);
  });
}

function reset() {
  handlers.clear();
  state.scroll = 0;
  state.total = 1500;
  state.visible = 775;
  state.duration = 3;
}

/**
 * Waits on the state under test, never on "some labels exist".
 *
 * The empty ruler already draws seven `0:00` ticks, so a precondition of
 * `times().length > 1` holds before anything has decoded. The tests here
 * used to wait on exactly that and then assert once, on whatever had
 * rendered by then. On a slow runner that was still the empty strip, and
 * the auto-fit test failed on Windows CI with `expected +0 to be close to
 * 3`. Delaying the decode by 150 ms reproduces that every time.
 *
 * So each assertion now sits inside the wait, and every wait requires a
 * label past zero, which the empty strip never has. A slow runner then
 * costs time instead of a verdict.
 */
function latest(): number {
  return Math.max(...times());
}

describe("the ruler follows the lane's viewport", () => {
  it("labels only the visible window once the waveform is zoomed", async () => {
    reset();
    render(<Timeline audioPath="/tmp/a.wav" zoom={500} />);
    emit("decode", 3);
    emit("redraw");

    // 1500px over 3s is 500 px/s, so a 775px pane shows ~1.55s.
    await waitFor(() => {
      expect(latest()).toBeGreaterThan(0);
      expect(latest()).toBeLessThanOrEqual(1.56);
    });
  });

  it("moves the labels when the pane is scrolled", async () => {
    reset();
    render(<Timeline audioPath="/tmp/a.wav" zoom={500} />);
    emit("decode", 3);
    emit("redraw");
    await waitFor(() => expect(latest()).toBeGreaterThan(0));

    // Scroll 500px — one second in at 500 px/s.
    state.scroll = 500;
    emit("scroll", 1, 2.55, 500, 1275);

    await waitFor(() => {
      expect(Math.min(...times())).toBeGreaterThanOrEqual(1);
      expect(latest()).toBeLessThanOrEqual(2.56);
    });
  });

  it("spans the whole file when all of it fits, which is auto-fit", async () => {
    reset();
    // Nothing to scroll: the drawn width is the pane width.
    state.total = 775;
    render(<Timeline audioPath="/tmp/a.wav" />);
    emit("decode", 3);
    emit("redraw");

    await waitFor(() => expect(latest()).toBeCloseTo(3, 5));
  });

  it("reports on redraw, because a zoom alone emits no scroll", async () => {
    // wavesurfer's scroll stream only updates from the container's own
    // scroll event. Zooming in without panning fires nothing, which is
    // exactly the case the ticket was filed for — so if the lane only
    // listened to `scroll`, the ruler would still never move.
    reset();
    render(<Timeline audioPath="/tmp/a.wav" zoom={500} />);
    emit("decode", 3);
    emit("redraw");
    // The 500 px/s window, rendered, before the zoom changes it. Reading
    // `times()` straight after the emit captured the empty strip instead.
    await waitFor(() => {
      expect(latest()).toBeGreaterThan(0.4);
      expect(latest()).toBeLessThanOrEqual(1.56);
    });

    state.total = 6000; // zoomed to 2000 px/s
    emit("redraw");

    await waitFor(() => {
      expect(latest()).toBeGreaterThan(0);
      expect(latest()).toBeLessThanOrEqual(0.4);
    });
  });
});
