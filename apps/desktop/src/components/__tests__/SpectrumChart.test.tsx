/**
 * The chart had a failure shared with the rest of the audit: a surface
 * that keeps reporting after it has stopped knowing.
 *
 * Analysing track A and then requesting a spectrum that comes back
 * empty — a silent region, a zero-length range, a failed analysis —
 * used to leave A's curve on screen, presented as the answer to the new
 * request, with nothing to distinguish stale from fresh.
 *
 * jsdom has no canvas: `getContext("2d")` returns null and the draw
 * effect bails, so none of this is observable without a stub. The stub
 * records calls rather than pixels — per #87's lesson, jsdom reports
 * zeroes for every geometry query, so anything measured through it
 * looks correct whether or not it is.
 */

import { render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { SpectrumChart } from "../SpectrumChart";

interface Call {
  fn: string;
  args: unknown[];
}

let calls: Call[] = [];
let originalGetContext: typeof HTMLCanvasElement.prototype.getContext;

/** Records every drawing call so assertions can be made on intent. */
function recordingContext(): CanvasRenderingContext2D {
  const record =
    (fn: string) =>
    (...args: unknown[]) => {
      calls.push({ fn, args });
    };
  return {
    setTransform: record("setTransform"),
    clearRect: record("clearRect"),
    fillRect: record("fillRect"),
    beginPath: record("beginPath"),
    moveTo: record("moveTo"),
    lineTo: record("lineTo"),
    stroke: record("stroke"),
    fillText: record("fillText"),
    set fillStyle(_v: string) {},
    set strokeStyle(_v: string) {},
    set lineWidth(_v: number) {},
    set lineJoin(_v: string) {},
    set font(_v: string) {},
  } as unknown as CanvasRenderingContext2D;
}

beforeEach(() => {
  calls = [];
  originalGetContext = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = vi.fn(() =>
    recordingContext(),
  ) as unknown as typeof HTMLCanvasElement.prototype.getContext;
});

afterEach(() => {
  HTMLCanvasElement.prototype.getContext = originalGetContext;
});

const CURVE = [
  { hz: 100, db: -20 },
  { hz: 1_000, db: -6 },
  { hz: 10_000, db: -60 },
];

/** Points plotted for the curve, excluding the fixed decade grid. */
function curvePoints(): Array<{ x: number; y: number }> {
  // The grid draws `beginPath / moveTo / lineTo / stroke` per line; the
  // curve is one `beginPath` followed by many `moveTo`/`lineTo`. Taking
  // everything after the last `beginPath` isolates it.
  const lastBegin = calls.map((c) => c.fn).lastIndexOf("beginPath");
  return calls
    .slice(lastBegin)
    .filter((c) => c.fn === "moveTo" || c.fn === "lineTo")
    .map((c) => ({ x: c.args[0] as number, y: c.args[1] as number }));
}

describe("SpectrumChart", () => {
  /**
   * The defect. The draw effect used to `return` on an empty `points`
   * *before* clearing, so the previous render stayed on the canvas.
   */
  it("clears the canvas when the new result is empty", () => {
    const { rerender } = render(<SpectrumChart points={CURVE} />);
    const drawnFirst = curvePoints().length;
    expect(drawnFirst).toBeGreaterThan(0);

    calls = [];
    rerender(<SpectrumChart points={[]} />);

    expect(
      calls.some((c) => c.fn === "clearRect"),
      "an empty result must clear the previous drawing, not leave it up",
    ).toBe(true);
  });

  /** Empty has to be distinguishable from broken. */
  it("says so when there is no data, rather than showing a blank box", () => {
    render(<SpectrumChart points={[]} />);
    const texts = calls.filter((c) => c.fn === "fillText").map((c) => c.args[0]);
    expect(texts).toContain("no spectrum data");
  });

  it("does not draw a curve when there are no points", () => {
    render(<SpectrumChart points={[]} />);
    // Only the decade grid and the dB gridlines should be stroked.
    const gridLines = 3 + 3;
    const strokes = calls.filter((c) => c.fn === "stroke").length;
    expect(strokes).toBeLessThanOrEqual(gridLines);
  });

  /**
   * `db` above 0 maps to a negative y and below the floor maps past
   * `height`. The canvas clips both, so the line silently leaves the
   * plot instead of riding its edge.
   */
  it("keeps out-of-range dB values inside the plot", () => {
    const height = 160;
    render(
      <SpectrumChart
        points={[
          { hz: 100, db: 40 },
          { hz: 1_000, db: -400 },
          { hz: 10_000, db: 0 },
        ]}
        height={height}
      />,
    );

    const ys = curvePoints().map((p) => p.y);
    expect(ys.length).toBeGreaterThan(0);
    for (const y of ys) {
      expect(y).toBeGreaterThanOrEqual(0);
      expect(y).toBeLessThanOrEqual(height);
    }
  });

  /**
   * `points[last].hz` of 0 made `x = hz / maxHz` a NaN and nothing drew.
   * `?? 22050` only guards `undefined`, not `0`.
   */
  it("survives a final point at 0 Hz", () => {
    render(<SpectrumChart points={[{ hz: 0, db: -30 }]} />);
    const xs = calls
      .filter((c) => c.fn === "moveTo" || c.fn === "lineTo")
      .map((c) => c.args[0] as number);
    expect(xs.every((x) => Number.isFinite(x))).toBe(true);
  });

  it("labels the canvas with the peak so a screen reader gets something", () => {
    const { getByRole } = render(<SpectrumChart points={CURVE} />);
    expect(getByRole("img")).toHaveAttribute(
      "aria-label",
      expect.stringContaining("1.0k"),
    );
  });

  it("reports no data in the label when there are no points", () => {
    const { getByRole } = render(<SpectrumChart points={[]} />);
    expect(getByRole("img")).toHaveAttribute(
      "aria-label",
      "Frequency spectrum, no data",
    );
  });
});
