/**
 * Snapping cut boundaries to zero crossings (#161).
 *
 * The reason this matters is audible: cutting mid-waveform leaves a
 * step discontinuity, and a step is a broadband click. So the tests are
 * about *where the boundary lands*, and about the cases where snapping
 * would do more harm than good — no crossing nearby, a range that would
 * collapse, a point outside the buffer.
 */

import { describe, expect, it } from "vitest";

import {
  DEFAULT_SEARCH_WINDOW_SEC,
  snapRange,
  snapToZeroCrossing,
} from "../lib/zeroCrossing";

const SR = 48_000;

/** One cycle of a sine per `period` samples, so crossings are known. */
function sine(length: number, period: number): Float32Array {
  const out = new Float32Array(length);
  for (let i = 0; i < length; i++) {
    out[i] = Math.sin((2 * Math.PI * i) / period);
  }
  return out;
}

describe("snapToZeroCrossing", () => {
  it("moves a mid-waveform point onto the nearest crossing", () => {
    // 480 samples per cycle = 100 Hz at 48 kHz. Crossings every 240
    // samples: 0, 240, 480, …
    const samples = sine(SR, 480);
    const nearCrossing = 245 / SR; // 5 samples past the crossing at 240

    const snapped = snapToZeroCrossing(samples, SR, nearCrossing);

    expect(Math.round(snapped * SR)).toBe(240);
  });

  it("goes backwards when that is nearer", () => {
    const samples = sine(SR, 480);
    // 235 is 5 before the crossing at 240 and 235 after the one at 0.
    const snapped = snapToZeroCrossing(samples, SR, 235 / SR);
    expect(Math.round(snapped * SR)).toBe(240);
  });

  /**
   * A crossing far outside the window is not the edit the user asked
   * for. Leaving the point alone is the honest answer.
   */
  it("leaves the point alone when no crossing is in range", () => {
    // Constant DC: never crosses zero at all.
    const samples = new Float32Array(SR).fill(0.5);
    const t = 0.5;
    expect(snapToZeroCrossing(samples, SR, t)).toBe(t);
  });

  it("does not search past its window", () => {
    const samples = new Float32Array(SR).fill(0.5);
    // One crossing, deliberately just outside a 10 ms window.
    const far = Math.round(0.5 * SR) + Math.round(0.02 * SR);
    samples[far] = -0.5;
    const t = 0.5;
    expect(snapToZeroCrossing(samples, SR, t, DEFAULT_SEARCH_WINDOW_SEC)).toBe(t);
  });

  it("treats an exact zero as a crossing", () => {
    const samples = new Float32Array(1000).fill(0.4);
    samples[500] = 0;
    const snapped = snapToZeroCrossing(samples, SR, 505 / SR);
    expect(Math.round(snapped * SR)).toBe(500);
  });

  it("returns the time unchanged for an empty buffer or a point outside it", () => {
    expect(snapToZeroCrossing(new Float32Array(0), SR, 1)).toBe(1);
    expect(snapToZeroCrossing(sine(100, 20), SR, 999)).toBe(999);
  });
});

describe("snapRange", () => {
  it("snaps both edges", () => {
    const samples = sine(SR, 480);
    const snapped = snapRange(samples, SR, {
      start: 245 / SR,
      end: 725 / SR,
    });
    expect(Math.round(snapped.start * SR)).toBe(240);
    expect(Math.round(snapped.end * SR)).toBe(720);
  });

  /**
   * Two edges landing on the same crossing would leave an empty
   * selection. Silently emptying one is worse than leaving it a sample
   * off a crossing.
   */
  it("leaves the range alone rather than collapsing it", () => {
    const samples = sine(SR, 480);
    const range = { start: 239 / SR, end: 241 / SR };
    expect(snapRange(samples, SR, range)).toEqual(range);
  });
});
