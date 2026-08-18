/**
 * Reading `view.json` back (#156).
 *
 * The failure mode this guards against is subtle: applying a *missing*
 * field as a value. A project saved by an older build has a head and no
 * zoom, and restoring `undefined` as the zoom would reset the timeline
 * while claiming to restore it. So an absent key means "leave this
 * alone", and `{}` is a valid, meaningful result.
 */

import { describe, expect, it } from "vitest";

import { viewToApply, viewToSave } from "../lib/viewState";

describe("viewToApply", () => {
  it("says nothing at all for an absent or empty file", () => {
    expect(viewToApply(null)).toEqual({});
    expect(viewToApply(undefined)).toEqual({});
    expect(viewToApply({})).toEqual({});
  });

  it("restores what is there and stays silent about what is not", () => {
    const out = viewToApply({ head: "abc123", zoom_px_per_sec: 80 });
    expect(out).toEqual({ head: "abc123", zoomPxPerSec: 80 });
    // Not `selection: undefined` — the key must be absent, or a caller
    // spreading this would clear the selection it never restored.
    expect("selection" in out).toBe(false);
    expect("playheadSec" in out).toBe(false);
  });

  /** Zero is the auto-fit sentinel, not "no zoom recorded". */
  it("treats a zoom of 0 as a real value", () => {
    expect(viewToApply({ zoom_px_per_sec: 0 }).zoomPxPerSec).toBe(0);
  });

  it("ignores a nonsensical zoom rather than applying it", () => {
    expect("zoomPxPerSec" in viewToApply({ zoom_px_per_sec: -5 })).toBe(false);
    expect("zoomPxPerSec" in viewToApply({ zoom_px_per_sec: NaN })).toBe(false);
  });

  it("caps an absurd zoom at the timeline's own ceiling", () => {
    expect(viewToApply({ zoom_px_per_sec: 99_999 }).zoomPxPerSec).toBe(2000);
  });

  it("restores a selection as a range", () => {
    expect(viewToApply({ selection: [1.5, 4.25] }).selection).toEqual({
      start: 1.5,
      end: 4.25,
    });
  });

  /**
   * An inverted or empty selection reads as a corrupt file. Leaving the
   * selection alone is better than restoring one that cannot be dragged
   * back off.
   */
  it("ignores an inverted, empty or negative selection", () => {
    expect("selection" in viewToApply({ selection: [4, 1] })).toBe(false);
    expect("selection" in viewToApply({ selection: [2, 2] })).toBe(false);
    expect("selection" in viewToApply({ selection: [-1, 3] })).toBe(false);
  });

  it("ignores a malformed selection shape", () => {
    // A hand-edited file, or one from a format that changed.
    expect(
      "selection" in viewToApply({ selection: [1] as unknown as [number, number] }),
    ).toBe(false);
  });

  it("restores a playhead, including 0", () => {
    expect(viewToApply({ playhead_sec: 0 }).playheadSec).toBe(0);
    expect(viewToApply({ playhead_sec: 12.5 }).playheadSec).toBe(12.5);
    expect("playheadSec" in viewToApply({ playhead_sec: -3 })).toBe(false);
  });

  it("ignores an empty head rather than restoring a blank one", () => {
    expect("head" in viewToApply({ head: "" })).toBe(false);
  });
});

describe("viewToSave", () => {
  it("writes the whole current view, not a patch", () => {
    expect(
      viewToSave({
        head: "abc",
        zoomPxPerSec: 60,
        selection: { start: 1, end: 2 },
        playheadSec: 3,
      }),
    ).toEqual({
      head: "abc",
      zoom_px_per_sec: 60,
      selection: [1, 2],
      playhead_sec: 3,
    });
  });

  it("records the absence of a selection explicitly", () => {
    // `null`, not omitted: clearing a selection is a thing that
    // happened and has to survive a reopen.
    expect(
      viewToSave({
        head: null,
        zoomPxPerSec: 0,
        selection: null,
        playheadSec: 0,
      }).selection,
    ).toBeNull();
  });

  it("round-trips through viewToApply", () => {
    const saved = viewToSave({
      head: "deadbeef",
      zoomPxPerSec: 120,
      selection: { start: 0.5, end: 9 },
      playheadSec: 4,
    });
    expect(viewToApply(saved)).toEqual({
      head: "deadbeef",
      zoomPxPerSec: 120,
      selection: { start: 0.5, end: 9 },
      playheadSec: 4,
    });
  });
});
