/**
 * The one axis every timeline row maps through (#344, #348).
 *
 * The numbers are the ones from #344: a 3-second session across a 763px
 * surface, then zoomed so 1.79–2.37 s fills it.
 */

import { describe, expect, it } from "vitest";

import {
  clampScrollSec,
  maxScrollSec,
  pctOf,
  pxPerSecFor,
  pxToSec,
  secToPx,
  visibleSpan,
  type Viewport,
} from "../lib/timelineViewport";

const WIDTH = 763;
const SESSION = 3;

const fit: Viewport = {
  durationSec: SESSION,
  widthPx: WIDTH,
  pxPerSec: pxPerSecFor(0, WIDTH, SESSION),
  scrollSec: 0,
};

const zoomedPxPerSec = WIDTH / (2.37 - 1.79);
const zoomed: Viewport = {
  durationSec: SESSION,
  widthPx: WIDTH,
  pxPerSec: zoomedPxPerSec,
  scrollSec: 1.79,
};

describe("the density", () => {
  it("is the user's zoom when there is one", () => {
    expect(pxPerSecFor(120, WIDTH, SESSION)).toBe(120);
  });

  it("fits the session across the surface at zoom 0", () => {
    expect(pxPerSecFor(0, WIDTH, SESSION)).toBeCloseTo(WIDTH / SESSION, 10);
  });

  it("is unknown, not infinite, before anything can be measured", () => {
    expect(pxPerSecFor(0, 0, SESSION)).toBe(0);
    expect(pxPerSecFor(0, WIDTH, 0)).toBe(0);
  });
});

describe("the scroll", () => {
  it("cannot move at fit", () => {
    expect(maxScrollSec(fit)).toBe(0);
    expect(clampScrollSec(2, fit)).toBe(0);
  });

  it("stops where the session's end meets the surface's right edge", () => {
    expect(maxScrollSec(zoomed)).toBeCloseTo(SESSION - (2.37 - 1.79), 10);
    expect(clampScrollSec(99, zoomed)).toBeCloseTo(maxScrollSec(zoomed), 10);
    expect(clampScrollSec(-1, zoomed)).toBe(0);
  });

  it("treats a non-number as the start", () => {
    expect(clampScrollSec(Number.NaN, zoomed)).toBe(0);
  });
});

describe("mapping", () => {
  it("at fit, spans the session", () => {
    expect(visibleSpan(fit)).toEqual({ start: 0, end: SESSION });
    expect(secToPx(1.5, fit)).toBeCloseTo(WIDTH / 2, 10);
  });

  it("zoomed, the surface's edges are the framed times", () => {
    const span = visibleSpan(zoomed)!;
    expect(span.start).toBeCloseTo(1.79, 10);
    expect(span.end).toBeCloseTo(2.37, 10);
    expect(secToPx(1.79, zoomed)).toBeCloseTo(0, 10);
    expect(secToPx(2.37, zoomed)).toBeCloseTo(WIDTH, 10);
  });

  /**
   * The drag #344 reported: 25–75% of the zoomed pane selected 25–75% of
   * the file, 0:00.77 → 0:02.23, instead of what was under the pointer.
   */
  it("reads the time under the pointer on the zoomed surface", () => {
    expect(pxToSec(WIDTH * 0.25, zoomed)).toBeCloseTo(1.79 + 0.25 * 0.58, 10);
    expect(pxToSec(WIDTH * 0.75, zoomed)).toBeCloseTo(1.79 + 0.75 * 0.58, 10);
  });

  it("keeps the pointer's time on the session", () => {
    expect(pxToSec(-50, fit)).toBe(0);
    expect(pxToSec(WIDTH + 50, fit)).toBe(SESSION);
  });

  /**
   * Zoomed out past fit, the session ends before the surface does. A span
   * clamped to the session would stretch its end to the right edge and
   * put that row on a different axis from the rest.
   */
  it("is not stretched when the session is narrower than the surface", () => {
    const narrow: Viewport = { ...fit, pxPerSec: 100 };
    expect(visibleSpan(narrow)).toEqual({ start: 0, end: WIDTH / 100 });
    expect(pctOf(SESSION, visibleSpan(narrow)!)).toBeCloseTo((SESSION * 100 * 100) / WIDTH, 10);
  });

  it("has no span before anything can be measured", () => {
    expect(visibleSpan({ ...fit, widthPx: 0 })).toBeNull();
    expect(visibleSpan({ ...fit, pxPerSec: 0 })).toBeNull();
  });

  it("puts a time as a percentage across a span", () => {
    expect(pctOf(1, { start: 0.75, end: 1.35 })).toBeCloseTo((0.25 / 0.6) * 100, 10);
    expect(pctOf(0.5, { start: 0.75, end: 1.35 })).toBeLessThan(0);
    expect(pctOf(1, { start: 1, end: 1 })).toBe(0);
  });
});
