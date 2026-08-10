/**
 * The ruler turns a click into a marker time.
 *
 * The click handler sits on the outer strip, which includes the 132px
 * spacer that lines the ticks up with the waveform lanes. The position
 * was measured against the *inner* tick area. Clicking anywhere in that
 * spacer therefore produced a negative fraction, and `add_marker` takes
 * an `f64` with no validation — so the marker went into the session at a
 * negative timestamp, where nothing can display it and nothing removes
 * it.
 */

import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { Ruler } from "../components/Ruler";

const SIDEBAR = 132;
const TICKS = 400;

/** Give the tick area a real box; jsdom reports zeroes otherwise. */
function stubGeometry() {
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
    function (this: HTMLElement) {
      // Only the inner tick area carries a ref and gets measured.
      return {
        left: SIDEBAR,
        top: 0,
        width: TICKS,
        height: 20,
        right: SIDEBAR + TICKS,
        bottom: 20,
        x: SIDEBAR,
        y: 0,
        toJSON: () => ({}),
      } as DOMRect;
    },
  );
}

describe("Ruler", () => {
  it("clicking the sidebar spacer does not create a marker before zero", () => {
    stubGeometry();
    const onAddMarker = vi.fn();
    render(<Ruler duration={60} onAddMarker={onAddMarker} />);

    // A click 20px in — inside the spacer, left of the tick area.
    fireEvent.click(screen.getByTestId("ruler"), { clientX: 20 });

    if (onAddMarker.mock.calls.length > 0) {
      const t = onAddMarker.mock.calls[0][0];
      expect(t, `a click on the sidebar produced ${t}s`).toBeGreaterThanOrEqual(
        0,
      );
    }
  });

  it("clicking past the right edge does not exceed the duration", () => {
    stubGeometry();
    const onAddMarker = vi.fn();
    render(<Ruler duration={60} onAddMarker={onAddMarker} />);

    fireEvent.click(screen.getByTestId("ruler"), {
      clientX: SIDEBAR + TICKS + 50,
    });

    if (onAddMarker.mock.calls.length > 0) {
      const t = onAddMarker.mock.calls[0][0];
      expect(t, `a click past the end produced ${t}s`).toBeLessThanOrEqual(60);
    }
  });

  it("clicking inside the tick area maps to the right time", () => {
    stubGeometry();
    const onAddMarker = vi.fn();
    render(<Ruler duration={60} onAddMarker={onAddMarker} />);

    // Halfway across the tick area.
    fireEvent.click(screen.getByTestId("ruler"), {
      clientX: SIDEBAR + TICKS / 2,
    });

    expect(onAddMarker).toHaveBeenCalledTimes(1);
    expect(onAddMarker.mock.calls[0][0]).toBeCloseTo(30, 5);
  });

  it("renders every tick when the duration is zero", () => {
    // All seven ticks compute t === 0, so they collided on `key={t}` and
    // React kept only one of them.
    const { container } = render(<Ruler duration={0} />);
    const labels = container.querySelectorAll("span");
    expect(labels.length).toBe(7);
  });
});
