/**
 * Ruler — time-label strip rendered above the waveform lanes.
 *
 * Clicking anywhere on the ruler calls `onAddMarker(timeSec)` so the
 * parent can start the add-marker flow.  The left sidebar placeholder
 * keeps the tick area aligned with the waveform region of the lane.
 */

import { useRef } from "react";

interface RulerProps {
  duration: number;
  /** Offset in px matching the track sidebar width (132px). */
  sidebarWidth?: number;
  onAddMarker?: (timeSec: number) => void;
}

export function Ruler({ duration, sidebarWidth = 132, onAddMarker }: RulerProps) {
  const rulerRef = useRef<HTMLDivElement>(null);

  const handleClick = (e: React.MouseEvent<HTMLDivElement>) => {
    if (!duration || !rulerRef.current || !onAddMarker) return;
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
    onAddMarker(pct * duration);
  };

  // Render 5-8 ticks regardless of duration.
  const tickCount = 6;
  // The precision every label shares, chosen from the gap between
  // ticks. Shared rather than per-label so the strip reads as one
  // scale instead of a ragged mix of "0:01" and "0:01.5".
  const decimals = decimalsFor(duration / tickCount);
  const ticks = Array.from({ length: tickCount + 1 }, (_, i) => {
    const t = (i / tickCount) * duration;
    const pct = duration > 0 ? (t / duration) * 100 : (i / tickCount) * 100;
    return { t, pct };
  });

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
 * How many decimal places a tick needs so that adjacent ticks differ.
 *
 * The ruler draws a fixed number of ticks across whatever duration it
 * is given, so the interval between them shrinks with the file. Below
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
