/**
 * MarkerLayer — absolutely-positioned marker flags overlaid on the
 * waveform area.
 *
 * Left-clicking a flag seeks to its time; right-clicking removes it.
 * The component is `pointerEvents: none` at the container level so
 * mouse events on the waveform itself pass through; individual flag
 * buttons re-enable pointer events.
 */

import type { Marker } from "../lib/tauri-bridge";

interface MarkerLayerProps {
  markers: Marker[];
  duration: number;
  /** Offset in px matching the track sidebar width. */
  sidebarWidth?: number;
  onSeek: (timeSec: number) => void;
  onRemove: (id: string) => void;
}

export function MarkerLayer({
  markers,
  duration,
  sidebarWidth = 132,
  onSeek,
  onRemove,
}: MarkerLayerProps) {
  if (!duration) return null;

  const getTimeSec = (m: Marker): number => {
    if (m.kind === "marker") return m.time_sec;
    return m.start_sec;
  };

  return (
    <div
      data-testid="marker-layer"
      style={{
        position: "absolute",
        inset: 0,
        left: sidebarWidth,
        pointerEvents: "none",
      }}
    >
      {markers.map((m) => {
        const t = getTimeSec(m);
        const pct = (t / duration) * 100;
        return (
          <div
            key={m.id}
            data-testid="marker-flag"
            style={{
              position: "absolute",
              left: `${pct}%`,
              top: 0,
              bottom: 0,
              display: "flex",
              flexDirection: "column",
              alignItems: "flex-start",
              pointerEvents: "auto",
            }}
          >
            {/* Vertical line */}
            <div
              style={{
                width: 1,
                flex: 1,
                background: "var(--accent)",
                opacity: 0.7,
              }}
            />
            {/* Label */}
            <button
              type="button"
              title={`Seek to ${m.name}; right-click to remove`}
              onClick={() => onSeek(t)}
              onContextMenu={(e) => {
                e.preventDefault();
                onRemove(m.id);
              }}
              style={{
                position: "absolute",
                top: 2,
                left: 3,
                background: "var(--accent)",
                color: "var(--onyx-0, #07080b)",
                fontSize: 9,
                fontFamily: "var(--font-mono)",
                padding: "1px 4px",
                borderRadius: 2,
                border: "none",
                cursor: "pointer",
                whiteSpace: "nowrap",
                textTransform: "uppercase",
                letterSpacing: "0.05em",
                lineHeight: 1.4,
              }}
            >
              {m.name}
            </button>
          </div>
        );
      })}
    </div>
  );
}
