/**
 * Volume automation lane (#95).
 *
 * The two things worth pinning: points are stored per *clip* relative
 * to that clip's start while the lane draws in absolute track time —
 * getting that conversion backwards would silently move every curve on
 * a track that had been cut — and a drag writes exactly once, on
 * release, because every write appends a session node.
 */

import { fireEvent, render, screen } from "@testing-library/react";
import type { MockedFunction } from "vitest";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  AutomationLane,
  clipPolyline,
  dbToY,
  yToDb,
  MAX_DB,
  MIN_DB,
} from "../components/AutomationLane";
import type { ClipSummary, EnvelopePoint } from "../lib/tauri-bridge";

/** The lane sizes itself from the DOM, which jsdom does not lay out. */
function stubSurfaceRect(width = 1000, height = 56) {
  const surface = screen.getByTestId("automation-surface");
  surface.getBoundingClientRect = () =>
    ({ left: 0, top: 0, width, height, right: width, bottom: height }) as DOMRect;
  return surface;
}

const ONE_CLIP: ClipSummary[] = [
  { start_sec: 0, length_sec: 10, volume_envelope: [] },
];

/** A track cut in two: the second clip starts 4 s in. */
const TWO_CLIPS: ClipSummary[] = [
  {
    start_sec: 0,
    length_sec: 4,
    volume_envelope: [{ time_sec: 1, gain_db: -6 }],
  },
  {
    start_sec: 4,
    length_sec: 6,
    volume_envelope: [{ time_sec: 2, gain_db: 3 }],
  },
];

describe("dB and pixel mapping", () => {
  it("round-trips through the lane height", () => {
    for (const db of [MIN_DB, -20, 0, 6, MAX_DB]) {
      expect(yToDb(dbToY(db))).toBeCloseTo(db, 1);
    }
  });

  it("puts the loudest value at the top", () => {
    expect(dbToY(MAX_DB)).toBe(0);
    expect(dbToY(MIN_DB)).toBe(56);
  });

  it("clamps rather than drawing outside the lane", () => {
    expect(dbToY(999)).toBe(0);
    expect(dbToY(-999)).toBe(56);
    expect(yToDb(-50)).toBe(MAX_DB);
    expect(yToDb(9999)).toBe(MIN_DB);
  });
});

describe("clipPolyline", () => {
  it("draws an empty envelope as a flat line at 0 dB", () => {
    // Not as nothing: with nothing drawn there is no affordance to
    // click, which is how the feature stayed invisible.
    expect(clipPolyline(ONE_CLIP[0])).toEqual([
      { time_sec: 0, gain_db: 0 },
      { time_sec: 10, gain_db: 0 },
    ]);
  });

  it("offsets points by the clip's start", () => {
    const line = clipPolyline(TWO_CLIPS[1]);
    // The stored point is at 2 s into a clip that starts at 4 s.
    expect(line.some((p) => p.time_sec === 6 && p.gain_db === 3)).toBe(true);
  });

  it("holds the first and last values out to the clip edges", () => {
    // Which is what the engine's interpolation does — a curve whose
    // first point is at 1 s is still at that value from 0 s.
    const line = clipPolyline(TWO_CLIPS[0]);
    expect(line[0]).toEqual({ time_sec: 0, gain_db: -6 });
    expect(line[line.length - 1]).toEqual({ time_sec: 4, gain_db: -6 });
  });
});

describe("AutomationLane editing", () => {
  type Commit = (clipIndex: number, points: EnvelopePoint[]) => void;
  let onCommit: MockedFunction<Commit>;

  beforeEach(() => {
    onCommit = vi.fn<Commit>();
  });

  it("renders one curve per clip and one handle per stored point", () => {
    render(
      <AutomationLane
        clips={TWO_CLIPS}
        duration={10}
        trackName="drums"
        onCommit={onCommit}
      />,
    );
    expect(screen.getByTestId("automation-curve-0")).toBeInTheDocument();
    expect(screen.getByTestId("automation-curve-1")).toBeInTheDocument();
    expect(screen.getByTestId("automation-point-0-0")).toBeInTheDocument();
    expect(screen.getByTestId("automation-point-1-0")).toBeInTheDocument();
  });

  it("adds a point in clip-relative time", () => {
    render(
      <AutomationLane
        clips={TWO_CLIPS}
        duration={10}
        trackName="drums"
        onCommit={onCommit}
      />,
    );
    stubSurfaceRect();
    // Click at 60% across a 10 s timeline = 6 s absolute, inside clip 1
    // which starts at 4 s — so 2 s relative. Reporting 6 here would put
    // the point past the end of the clip.
    fireEvent.click(screen.getByTestId("automation-band-1"), {
      clientX: 600,
      clientY: 0,
    });
    expect(onCommit).toHaveBeenCalledTimes(1);
    const [clipIndex, points] = onCommit.mock.calls[0];
    expect(clipIndex).toBe(1);
    expect(points).toHaveLength(2);
    const added = points.find((p) => p.gain_db === MAX_DB);
    expect(added?.time_sec).toBeCloseTo(2, 5);
  });

  it("writes once on release, not on every pointer move", () => {
    render(
      <AutomationLane
        clips={TWO_CLIPS}
        duration={10}
        trackName="drums"
        onCommit={onCommit}
      />,
    );
    stubSurfaceRect();
    fireEvent.pointerDown(screen.getByTestId("automation-point-0-0"));

    for (const x of [100, 150, 200, 250]) {
      fireEvent.pointerMove(window, { clientX: x, clientY: 28 });
    }
    expect(onCommit).not.toHaveBeenCalled();

    fireEvent.pointerUp(window);
    expect(onCommit).toHaveBeenCalledTimes(1);
    const [clipIndex, points] = onCommit.mock.calls[0];
    expect(clipIndex).toBe(0);
    // 250/1000 of 10 s = 2.5 s absolute, clip 0 starts at 0.
    expect(points[0].time_sec).toBeCloseTo(2.5, 5);
    // Half-height is the midpoint of [MIN_DB, MAX_DB].
    expect(points[0].gain_db).toBeCloseTo((MIN_DB + MAX_DB) / 2, 1);
  });

  it("keeps a dragged point inside its own clip", () => {
    render(
      <AutomationLane
        clips={TWO_CLIPS}
        duration={10}
        trackName="drums"
        onCommit={onCommit}
      />,
    );
    stubSurfaceRect();
    // Drag clip 0's point far to the right, past the clip's 4 s end.
    fireEvent.pointerDown(screen.getByTestId("automation-point-0-0"));
    fireEvent.pointerMove(window, { clientX: 950, clientY: 28 });
    fireEvent.pointerUp(window);

    const [, points] = onCommit.mock.calls[0];
    expect(points[0].time_sec).toBe(4);
  });

  it("removes a point on double-click", () => {
    render(
      <AutomationLane
        clips={TWO_CLIPS}
        duration={10}
        trackName="drums"
        onCommit={onCommit}
      />,
    );
    fireEvent.doubleClick(screen.getByTestId("automation-point-0-0"));
    expect(onCommit).toHaveBeenCalledWith(0, []);
  });

  it("is editable from the keyboard", () => {
    render(
      <AutomationLane
        clips={TWO_CLIPS}
        duration={10}
        trackName="drums"
        onCommit={onCommit}
      />,
    );
    const handle = screen.getByTestId("automation-point-0-0");
    handle.focus();
    expect(handle).toHaveFocus();
    expect(handle).toHaveAccessibleName(
      "Automation point at 1.00 seconds, -6.0 dB",
    );

    fireEvent.keyDown(handle, { key: "ArrowUp" });
    expect(onCommit).toHaveBeenLastCalledWith(0, [
      { time_sec: 1, gain_db: -5 },
    ]);

    fireEvent.keyDown(handle, { key: "Delete" });
    expect(onCommit).toHaveBeenLastCalledWith(0, []);
  });

  it("reconciles from the session rather than keeping its draft", () => {
    const { rerender } = render(
      <AutomationLane
        clips={TWO_CLIPS}
        duration={10}
        trackName="drums"
        onCommit={onCommit}
      />,
    );
    fireEvent.doubleClick(screen.getByTestId("automation-point-0-0"));
    expect(
      screen.queryByTestId("automation-point-0-0"),
    ).not.toBeInTheDocument();

    // The refresh says the point is still there — e.g. the command was
    // rejected. The lane must follow the session, not its own guess.
    rerender(
      <AutomationLane
        clips={TWO_CLIPS.map((c) => ({ ...c }))}
        duration={10}
        trackName="drums"
        onCommit={onCommit}
      />,
    );
    expect(screen.getByTestId("automation-point-0-0")).toBeInTheDocument();
  });
});
