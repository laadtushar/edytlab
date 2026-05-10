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
    const pct = (e.clientX - rect.left) / rect.width;
    onAddMarker(pct * duration);
  };

  // Render 5-8 ticks regardless of duration.
  const tickCount = 6;
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
        {ticks.map(({ t, pct }) => (
          <span
            key={t}
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
            {fmtTimecode(t)}
          </span>
        ))}
      </div>
    </div>
  );
}

function fmtTimecode(sec: number): string {
  if (sec === 0) return "0:00";
  const m = Math.floor(sec / 60);
  const s = Math.floor(sec % 60);
  return `${m}:${String(s).padStart(2, "0")}`;
}
