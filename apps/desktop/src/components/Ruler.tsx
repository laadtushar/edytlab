/**
 * Ruler — time-label strip rendered above the waveform lanes.
 *
 * Draws the window that is actually on screen (`view`), not the whole
 * file. The strip used to divide the duration into six ticks and know
 * nothing of the zoom, so at 500 px/s on a 3-second file — where the
 * waveform is ~1500px scrolling inside a ~775px pane — it still read
 * 0:00 → 0:03 across the visible width, describing a view the user
 * was not looking at. A ruler that looks authoritative and is wrong
 * is worse than no ruler, because every timed edit is measured
 * against it.
 *
 * Clicking anywhere on the ruler calls `onAddMarker(timeSec)` so the
 * parent can start the add-marker flow.  The left sidebar placeholder
 * keeps the tick area aligned with the waveform region of the lane.
 */

import { useRef } from "react";

/** The span of audio the strip is drawn across, in seconds. */
export interface RulerView {
  start: number;
  end: number;
}

interface RulerProps {
  duration: number;
  /**
   * The window actually on screen, when the waveform is zoomed in far
   * enough to scroll. Omitted — or given as the whole file — means the
   * whole file is visible, which is what auto-fit shows and what this
   * strip used to assume unconditionally.
   */
  view?: RulerView | null;
  /** Offset in px matching the track sidebar width (132px). */
  sidebarWidth?: number;
  onAddMarker?: (timeSec: number) => void;
}

export function Ruler({
  duration,
  view,
  sidebarWidth = 132,
  onAddMarker,
}: RulerProps) {
  const rulerRef = useRef<HTMLDivElement>(null);

  // The window this strip describes. A view is only believed if it is
  // a real span — a degenerate or reversed one would put every label
  // in the same place, or off the strip entirely.
  const start = view && view.end > view.start ? view.start : 0;
  const end = view && view.end > view.start ? view.end : duration;
  const span = end - start;

  const handleClick = (e: React.MouseEvent<HTMLDivElement>) => {
    if (!duration || span <= 0 || !rulerRef.current || !onAddMarker) return;
    const rect = rulerRef.current.getBoundingClientRect();
    if (rect.width <= 0) return;
    // The handler is on the outer strip, which includes the sidebar
    // spacer, but the position is measured against the tick area. A
    // click in the spacer is therefore left of `rect.left` and lands
    // before zero; one past the right edge lands after the end. Both
    // used to go straight through to `add_marker`, which takes an f64
    // and validates nothing — so the marker entered the session at a
    // timestamp no view can render and no control can reach.
    const pct = Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width));
    // Against the visible window, not the whole file. Mapping a click
    // to `pct * duration` while the strip was showing a zoomed-in
    // window put the marker wherever that fraction of the *file*
    // happened to be — a marker silently written to the session at a
    // time the user never pointed at, which is a data error rather
    // than a cosmetic one.
    onAddMarker(start + pct * span);
  };

  // The precision every label shares, chosen from the gap between
  // ticks. Shared rather than per-label so the strip reads as one
  // scale instead of a ragged mix of "0:01" and "0:01.5".
  const interval = intervalFor(span);
  const decimals = decimalsFor(interval);
  const ticks = ticksIn(start, end, interval);

  return (
    <div
      data-testid="ruler"
      style={{
        display: "flex",
        height: 20,
        borderBottom: "1px solid var(--border)",
        background: "var(--surface-elev)",
        fontSize: 9,
        fontFamily: "var(--font-mono)",
        color: "var(--text-faint)",
        position: "relative",
        cursor: onAddMarker ? "crosshair" : "default",
      }}
      onClick={handleClick}
    >
      {/* Match sidebar width */}
      <div style={{ width: sidebarWidth, flexShrink: 0, borderRight: "1px solid var(--border)" }} />
      <div ref={rulerRef} style={{ flex: 1, position: "relative" }}>
        {ticks.map(({ t, pct }, i) => (
          <span
            // Index, not `t`: at duration 0 every tick is 0 and the keys
            // collide.
            key={i}
            style={{
              position: "absolute",
              left: `${pct}%`,
              transform: "translateX(-50%)",
              top: 3,
              letterSpacing: "0.05em",
              textTransform: "uppercase",
              fontSize: 8,
              userSelect: "none",
            }}
          >
            {fmtTimecode(t, decimals)}
          </span>
        ))}
      </div>
    </div>
  );
}

/**
 * Tick intervals the eye reads as round numbers.
 *
 * Every entry is exact at the precision `decimalsFor` gives it, which
 * is why 0.25 is not here: it would be labelled to one decimal and
 * print 0:00.3 for a tick standing at 0.25.
 */
const NICE_INTERVALS = [
  0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 900,
  1800, 3600,
];

/**
 * The gap between ticks, for a window of `span` seconds.
 *
 * The strip used to divide whatever duration it was given into six,
 * which is why it could not follow a zoom: six ticks across the pane
 * describe the whole file no matter how much of the file the pane is
 * showing. Choosing an interval instead, and then drawing every
 * multiple of it that falls inside the window, is what makes the
 * labels move when the window does.
 */
function intervalFor(span: number, target = 6): number {
  if (!Number.isFinite(span) || span <= 0) return 0;
  const ideal = span / target;
  return (
    NICE_INTERVALS.find((i) => i >= ideal) ??
    NICE_INTERVALS[NICE_INTERVALS.length - 1]
  );
}

/** How many ticks the empty strip draws. Preserved from #320. */
const EMPTY_TICKS = 7;

/** Every multiple of `interval` inside the window, with its position. */
function ticksIn(
  start: number,
  end: number,
  interval: number,
): { t: number; pct: number }[] {
  const span = end - start;
  // With no audio there is nothing to label, but the strip still
  // draws its row of 0:00s — that is what an empty timeline has always
  // looked like, and the count is what a regression test pins: all
  // seven ticks compute t === 0, so they once collided on `key={t}`
  // and React kept only one.
  if (interval <= 0 || span <= 0) {
    return Array.from({ length: EMPTY_TICKS }, (_, i) => ({
      t: 0,
      pct: (i / (EMPTY_TICKS - 1)) * 100,
    }));
  }
  const out: { t: number; pct: number }[] = [];
  const firstIndex = Math.ceil(start / interval - 1e-9);
  // Counted from the first index rather than accumulated, so a long
  // window cannot drift a tick off its own label. Capped because a
  // window far longer than the largest interval would otherwise draw
  // a label per hour for as long as the file lasts.
  for (let k = 0; out.length < 200; k++) {
    const t = (firstIndex + k) * interval;
    if (t > end + 1e-9) break;
    out.push({ t, pct: ((t - start) / span) * 100 });
  }
  return out;
}

/**
 * How many decimal places a tick needs so that adjacent ticks differ.
 *
 * The interval shrinks as the window narrows, so a zoomed-in view
 * reaches sub-second gaps the same way a short file always did. Below
 * one second per tick, a whole-second format prints the same label
 * twice: a 3-second file ticked every 0.5 s read
 *
 *   0:00  0:00  0:01  0:01  0:02  0:02
 *
 * which is not a ruler — it is six labels, four of which are lies
 * about where they sit. Found by loading a 3-second file into the
 * built app.
 *
 * One decimal covers down to 0.1 s per tick, two below that. More
 * than two would not fit the 8px type.
 */
function decimalsFor(intervalSec: number): number {
  if (!Number.isFinite(intervalSec) || intervalSec <= 0) return 0;
  if (intervalSec >= 1) return 0;
  if (intervalSec >= 0.1) return 1;
  return 2;
}

function fmtTimecode(sec: number, decimals = 0): string {
  const m = Math.floor(sec / 60);
  const rest = sec - m * 60;
  if (decimals === 0) {
    return `${m}:${String(Math.floor(rest)).padStart(2, "0")}`;
  }
  // `toFixed` can carry to 60 (59.97 at one decimal), which would
  // print "0:60.0" instead of rolling the minute over.
  const fixed = rest.toFixed(decimals);
  if (Number.parseFloat(fixed) >= 60) {
    return `${m + 1}:${(0).toFixed(decimals).padStart(decimals + 3, "0")}`;
  }
  return `${m}:${fixed.padStart(decimals + 3, "0")}`;
}
