/**
 * The ruler has to describe the window you are looking at (#323).
 *
 * It used to divide the file's duration into six ticks and know
 * nothing of the zoom, so at 500 px/s on a 3-second file — waveform
 * ~1500px, pane ~775px — it still read 0:00 → 0:03 across the visible
 * width. Observed live: the waveform re-rendered across a dozen zoom
 * steps while the ruler did not move a pixel.
 *
 * The click mapping is the part that mattered most. It turned x into
 * `pct * duration` against the whole file, so adding a marker while
 * zoomed wrote a timestamp the user never pointed at — a silent data
 * error, not a cosmetic one.
 */

import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { Ruler } from "../components/Ruler";

function labels(): string[] {
  return Array.from(screen.getByTestId("ruler").querySelectorAll("span"))
    .map((el) => el.textContent ?? "")
    .filter(Boolean);
}

/** The tick area — the ruler's second child, beside the sidebar spacer. */
function tickArea(): HTMLElement {
  return screen.getByTestId("ruler").children[1] as HTMLElement;
}

/** Give the tick area a real width, which jsdom does not. */
function pin(width = 800, left = 100) {
  tickArea().getBoundingClientRect = () =>
    ({ left, width, right: left + width, top: 0, bottom: 20, height: 20, x: left, y: 0, toJSON: () => ({}) }) as DOMRect;
}

describe("the ruler and the zoom", () => {
  it("relabels when the window narrows", () => {
    const wide = render(<Ruler duration={180} />);
    const whole = labels();
    wide.unmount();

    render(<Ruler duration={180} view={{ start: 0, end: 3 }} />);
    const zoomed = labels();

    expect(zoomed.join(" ")).not.toBe(whole.join(" "));
    // A three-second window has to reach sub-second ticks; six labels
    // across three minutes cannot say anything about three seconds.
    expect(zoomed.some((l) => /\./.test(l))).toBe(true);
  });

  it("starts from the scroll offset, not from zero", () => {
    render(<Ruler duration={180} view={{ start: 60, end: 72 }} />);
    // Every label must lie inside the window. The old strip always
    // began at 0:00 however far the pane had been scrolled.
    for (const l of labels()) {
      const [m, s] = l.split(":");
      const t = Number(m) * 60 + Number.parseFloat(s);
      expect(t).toBeGreaterThanOrEqual(60);
      expect(t).toBeLessThanOrEqual(72);
    }
    expect(labels().length).toBeGreaterThan(1);
  });

  it("keeps every label distinct at any zoom", () => {
    for (const view of [
      { start: 0, end: 0.5 },
      { start: 12.25, end: 12.75 },
      { start: 0, end: 3 },
      { start: 100, end: 400 },
    ]) {
      const { unmount } = render(<Ruler duration={600} view={view} />);
      const ls = labels();
      expect(new Set(ls).size, `${JSON.stringify(view)} → ${ls.join(" ")}`).toBe(
        ls.length,
      );
      unmount();
    }
  });

  it("puts ticks on round numbers rather than on the window edge", () => {
    // Six evenly spaced ticks from an arbitrary start read as noise:
    // 1:03.7, 1:05.7, 1:07.7… A ruler is useful because its ticks are
    // round, so they are multiples of the interval, not offsets.
    render(<Ruler duration={600} view={{ start: 61.3, end: 73.3 }} />);
    // 12 seconds across six ticks is a 2-second interval, and 62 is
    // the first multiple of 2 at or after 61.3.
    expect(labels()[0]).toBe("1:02");
    expect(labels()[1]).toBe("1:04");
  });
});

describe("adding a marker from the ruler", () => {
  it("lands where the cursor is, at the visible window", () => {
    const onAddMarker = vi.fn();
    render(
      <Ruler duration={600} view={{ start: 60, end: 72 }} onAddMarker={onAddMarker} />,
    );
    pin(800, 100);

    // Halfway across a window of 60 → 72 is 66 seconds.
    fireEvent.click(screen.getByTestId("ruler"), { clientX: 500 });

    expect(onAddMarker).toHaveBeenCalledTimes(1);
    expect(onAddMarker.mock.calls[0][0]).toBeCloseTo(66, 5);
  });

  it("still maps against the whole file when all of it is visible", () => {
    const onAddMarker = vi.fn();
    render(<Ruler duration={600} onAddMarker={onAddMarker} />);
    pin(800, 100);

    fireEvent.click(screen.getByTestId("ruler"), { clientX: 500 });

    expect(onAddMarker.mock.calls[0][0]).toBeCloseTo(300, 5);
  });

  it("clamps a click in the sidebar spacer to the window start", () => {
    const onAddMarker = vi.fn();
    render(
      <Ruler duration={600} view={{ start: 60, end: 72 }} onAddMarker={onAddMarker} />,
    );
    pin(800, 100);

    // Left of the tick area: inside the strip, before its origin.
    fireEvent.click(screen.getByTestId("ruler"), { clientX: 10 });

    expect(onAddMarker.mock.calls[0][0]).toBeCloseTo(60, 5);
  });
});

describe("windows the ruler should not believe", () => {
  it("falls back to the whole file when the window is empty", () => {
    render(<Ruler duration={120} view={{ start: 5, end: 5 }} />);
    expect(labels()[0]).toBe("0:00");
    expect(labels().length).toBeGreaterThan(1);
  });

  it("falls back when the window is reversed", () => {
    render(<Ruler duration={120} view={{ start: 90, end: 10 }} />);
    expect(labels()[0]).toBe("0:00");
  });

  it("keeps the empty strip exactly as it was at duration 0", () => {
    // Not this ticket'''s to change: the row of 0:00s is what an empty
    // timeline looks like, and Ruler.test.tsx pins the count against a
    // past key collision.
    render(<Ruler duration={0} />);
    expect(labels()).toEqual(Array(7).fill("0:00"));
  });

  it("adds no marker when there is no audio", () => {
    const onAddMarker = vi.fn();
    render(<Ruler duration={0} onAddMarker={onAddMarker} />);
    fireEvent.click(screen.getByTestId("ruler"), { clientX: 400 });
    expect(onAddMarker).not.toHaveBeenCalled();
  });
});
