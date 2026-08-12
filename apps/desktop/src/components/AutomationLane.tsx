/**
 * AutomationLane — draw and edit a clip's volume envelope (#95).
 *
 * The curve, the persistence and the render integration all existed
 * before this component: `Clip.volume_envelope` round-trips through the
 * session, `set_clip_envelope` writes it, the engine interpolates it per
 * frame, and #76 taught it to survive cuts. The only missing piece was
 * being able to see or touch it, so this is the interaction layer for a
 * feature that was otherwise finished and unreachable.
 *
 * Coordinates
 * -----------
 * x is time in seconds across the whole track, so the lane lines up with
 * the waveform above it. Envelope points are stored per *clip*, relative
 * to that clip's start, so `clip.start_sec` is added on the way out and
 * subtracted on the way in. Getting that backwards would silently move
 * every curve on a track that had been cut.
 *
 * y is dB, drawn top-down over [MAX_DB, MIN_DB].
 *
 * One gesture, one node
 * ---------------------
 * Dragging emits nothing. The write happens on pointer release, so a
 * drag across the lane is one undo step rather than one per mouse-move.
 * That is not a nicety: every write appends a session node, and a
 * pixel-rate write would put hundreds of them in the DAG per gesture.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import type { ClipSummary, EnvelopePoint } from "../lib/tauri-bridge";

/** Vertical range of the lane. Matches the backend's accepted range. */
export const MIN_DB = -60;
export const MAX_DB = 12;

const LANE_HEIGHT = 56;
const HIT_RADIUS_PX = 7;

export interface AutomationLaneProps {
  clips: ClipSummary[];
  /** Total timeline duration in seconds; 0 while the audio is loading. */
  duration: number;
  /**
   * Commit a clip's whole curve. Called once per finished gesture with
   * points relative to that clip's start.
   */
  onCommit?: (clipIndex: number, points: EnvelopePoint[]) => void;
  /** Lane label, for the accessible name of the editing surface. */
  trackName: string;
}

interface DragState {
  clipIndex: number;
  pointIndex: number;
}

/** dB → y pixel, clamped to the lane. */
export function dbToY(db: number, height = LANE_HEIGHT): number {
  const t = (MAX_DB - db) / (MAX_DB - MIN_DB);
  return Math.max(0, Math.min(1, t)) * height;
}

/** y pixel → dB, clamped to the lane's range. */
export function yToDb(y: number, height = LANE_HEIGHT): number {
  const t = height > 0 ? y / height : 0;
  const db = MAX_DB - Math.max(0, Math.min(1, t)) * (MAX_DB - MIN_DB);
  // Two decimals: a pixel is worth ~1.3 dB here, so more precision is
  // noise, and round numbers read better in the node label.
  return Math.round(db * 100) / 100;
}

/**
 * The polyline for one clip, in absolute seconds.
 *
 * An empty envelope draws a flat line at 0 dB across the clip rather
 * than nothing. Drawing nothing is what made the feature undiscoverable
 * in the first place — there was no affordance to click on.
 */
export function clipPolyline(clip: ClipSummary): EnvelopePoint[] {
  const end = clip.start_sec + clip.length_sec;
  if (clip.volume_envelope.length === 0) {
    return [
      { time_sec: clip.start_sec, gain_db: 0 },
      { time_sec: end, gain_db: 0 },
    ];
  }
  const pts = clip.volume_envelope
    .map((p) => ({
      time_sec: clip.start_sec + p.time_sec,
      gain_db: p.gain_db,
    }))
    .sort((a, b) => a.time_sec - b.time_sec);
  // Hold the first and last values out to the clip edges, which is what
  // the engine's interpolation does — a curve that starts at 2 s is
  // still at its first value from 0 s.
  const first = pts[0];
  const last = pts[pts.length - 1];
  const head =
    first.time_sec > clip.start_sec
      ? [{ time_sec: clip.start_sec, gain_db: first.gain_db }]
      : [];
  const tail =
    last.time_sec < end ? [{ time_sec: end, gain_db: last.gain_db }] : [];
  return [...head, ...pts, ...tail];
}

export function AutomationLane({
  clips,
  duration,
  onCommit,
  trackName,
}: AutomationLaneProps) {
  const surfaceRef = useRef<HTMLDivElement>(null);
  // Local copy so a drag can move a point without a round trip per
  // frame. Replaced wholesale whenever the session says otherwise.
  const [draft, setDraft] = useState<ClipSummary[]>(clips);
  const [drag, setDrag] = useState<DragState | null>(null);

  useEffect(() => {
    setDraft(clips);
  }, [clips]);

  const toSeconds = useCallback(
    (clientX: number): number => {
      const rect = surfaceRef.current?.getBoundingClientRect();
      if (!rect || rect.width === 0 || duration <= 0) return 0;
      const t = ((clientX - rect.left) / rect.width) * duration;
      return Math.max(0, Math.min(duration, t));
    },
    [duration],
  );

  const toDb = useCallback((clientY: number): number => {
    const rect = surfaceRef.current?.getBoundingClientRect();
    if (!rect || rect.height === 0) return 0;
    return yToDb(clientY - rect.top, rect.height);
  }, []);

  const xOf = (sec: number) => (duration > 0 ? (sec / duration) * 100 : 0);

  /** Commit `clipIndex`'s current draft curve. */
  const commit = useCallback(
    (clipIndex: number, next: ClipSummary[]) => {
      const clip = next[clipIndex];
      if (!clip) return;
      onCommit?.(clipIndex, clip.volume_envelope);
    },
    [onCommit],
  );

  const addPoint = useCallback(
    (clipIndex: number, e: React.MouseEvent) => {
      const clip = draft[clipIndex];
      if (!clip) return;
      const abs = toSeconds(e.clientX);
      const rel = Math.max(
        0,
        Math.min(clip.length_sec, abs - clip.start_sec),
      );
      const point: EnvelopePoint = { time_sec: rel, gain_db: toDb(e.clientY) };
      const next = draft.map((c, i) =>
        i === clipIndex
          ? {
              ...c,
              volume_envelope: [...c.volume_envelope, point].sort(
                (a, b) => a.time_sec - b.time_sec,
              ),
            }
          : c,
      );
      setDraft(next);
      commit(clipIndex, next);
    },
    [draft, toSeconds, toDb, commit],
  );

  const removePoint = useCallback(
    (clipIndex: number, pointIndex: number) => {
      const next = draft.map((c, i) =>
        i === clipIndex
          ? {
              ...c,
              volume_envelope: c.volume_envelope.filter(
                (_, j) => j !== pointIndex,
              ),
            }
          : c,
      );
      setDraft(next);
      commit(clipIndex, next);
    },
    [draft, commit],
  );

  /**
   * Keyboard editing, because a curve reachable only by mouse is the
   * same class of unreachable this ticket is about.
   *
   * Arrows nudge — 1 dB vertically, 1% of the timeline horizontally,
   * with Shift for a tenth of each. Delete/Backspace removes. Each
   * keystroke is its own gesture and so its own node; that is right
   * for a deliberate nudge in a way it would not be for a drag.
   */
  const onPointKey = useCallback(
    (e: React.KeyboardEvent, clipIndex: number, pointIndex: number) => {
      const clip = draft[clipIndex];
      const point = clip?.volume_envelope[pointIndex];
      if (!clip || !point) return;

      if (e.key === "Delete" || e.key === "Backspace") {
        e.preventDefault();
        removePoint(clipIndex, pointIndex);
        return;
      }

      const dbStep = e.shiftKey ? 0.1 : 1;
      const secStep = (e.shiftKey ? 0.001 : 0.01) * duration;
      let { time_sec: t, gain_db: db } = point;
      switch (e.key) {
        case "ArrowUp":
          db = Math.min(MAX_DB, db + dbStep);
          break;
        case "ArrowDown":
          db = Math.max(MIN_DB, db - dbStep);
          break;
        case "ArrowLeft":
          t = Math.max(0, t - secStep);
          break;
        case "ArrowRight":
          t = Math.min(clip.length_sec, t + secStep);
          break;
        default:
          return;
      }
      e.preventDefault();
      const next = draft.map((c, i) =>
        i === clipIndex
          ? {
              ...c,
              volume_envelope: c.volume_envelope.map((p, j) =>
                j === pointIndex ? { time_sec: t, gain_db: db } : p,
              ),
            }
          : c,
      );
      setDraft(next);
      commit(clipIndex, next);
    },
    [draft, duration, removePoint, commit],
  );

  // Drag is bound at the window so the pointer can leave the lane
  // mid-gesture without the point sticking — the same reason the
  // waveform's own selection drag does it there.
  useEffect(() => {
    if (!drag) return;
    const onMove = (e: PointerEvent) => {
      setDraft((prev) =>
        prev.map((c, i) => {
          if (i !== drag.clipIndex) return c;
          const abs = toSeconds(e.clientX);
          const rel = Math.max(0, Math.min(c.length_sec, abs - c.start_sec));
          return {
            ...c,
            volume_envelope: c.volume_envelope.map((p, j) =>
              j === drag.pointIndex
                ? { time_sec: rel, gain_db: toDb(e.clientY) }
                : p,
            ),
          };
        }),
      );
    };
    const onUp = () => {
      setDrag(null);
      // Read the draft through the setter so the commit sees the last
      // move rather than the state this effect closed over.
      setDraft((prev) => {
        commit(drag.clipIndex, prev);
        return prev;
      });
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    return () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
    };
  }, [drag, toSeconds, toDb, commit]);

  return (
    <div
      data-testid="automation-lane"
      style={{
        display: "flex",
        borderBottom: "1px solid var(--border)",
        background: "var(--surface)",
      }}
    >
      <div
        style={{
          width: 132,
          flexShrink: 0,
          background: "var(--surface-elev)",
          borderRight: "1px solid var(--border)",
          display: "flex",
          alignItems: "center",
          padding: "0 12px",
          fontFamily: "var(--font-mono)",
          fontSize: 9,
          letterSpacing: "0.05em",
          textTransform: "uppercase",
          color: "var(--text-dim)",
        }}
      >
        volume
      </div>
      {/* Curve and handles are separate layers on purpose.
          A single SVG would need `preserveAspectRatio="none"` to let x
          run in percentage-like units, and that squashes circles into
          ellipses. The curve does not care about aspect; the handles
          do, so they are HTML positioned over the top. */}
      <div
        ref={surfaceRef}
        data-testid="automation-surface"
        role="group"
        aria-label={`${trackName} volume automation`}
        style={{
          flex: 1,
          position: "relative",
          height: LANE_HEIGHT,
          cursor: "crosshair",
        }}
      >
        <svg
          width="100%"
          height={LANE_HEIGHT}
          viewBox={`0 0 100 ${LANE_HEIGHT}`}
          preserveAspectRatio="none"
          style={{ display: "block", position: "absolute", inset: 0 }}
        >
          {/* 0 dB reference, so "louder" and "quieter" are readable. */}
          <line
            x1={0}
            x2={100}
            y1={dbToY(0)}
            y2={dbToY(0)}
            stroke="var(--border-strong)"
            strokeDasharray="1 1"
            vectorEffect="non-scaling-stroke"
          />
          {draft.map((clip, clipIndex) => (
            <polyline
              key={clipIndex}
              data-testid={`automation-curve-${clipIndex}`}
              points={clipPolyline(clip)
                .map((p) => `${xOf(p.time_sec)},${dbToY(p.gain_db)}`)
                .join(" ")}
              vectorEffect="non-scaling-stroke"
              fill="none"
              stroke="var(--accent)"
              strokeWidth={1.5}
            />
          ))}
        </svg>

        {draft.map((clip, clipIndex) => (
          <div key={clipIndex}>
            {/* Clickable band for this clip. Under the handles, so a
                click on a handle drags it instead of adding another. */}
            <div
              data-testid={`automation-band-${clipIndex}`}
              onClick={(e) => addPoint(clipIndex, e)}
              style={{
                position: "absolute",
                left: `${xOf(clip.start_sec)}%`,
                width: `${xOf(clip.length_sec)}%`,
                top: 0,
                height: LANE_HEIGHT,
              }}
            />
            {clip.volume_envelope.map((p, pointIndex) => (
              <button
                type="button"
                key={pointIndex}
                data-testid={`automation-point-${clipIndex}-${pointIndex}`}
                aria-label={`Automation point at ${(
                  clip.start_sec + p.time_sec
                ).toFixed(2)} seconds, ${p.gain_db.toFixed(1)} dB`}
                onPointerDown={(e) => {
                  e.stopPropagation();
                  setDrag({ clipIndex, pointIndex });
                }}
                onDoubleClick={(e) => {
                  e.stopPropagation();
                  removePoint(clipIndex, pointIndex);
                }}
                onKeyDown={(e) => onPointKey(e, clipIndex, pointIndex)}
                style={{
                  position: "absolute",
                  left: `${xOf(clip.start_sec + p.time_sec)}%`,
                  top: dbToY(p.gain_db),
                  width: HIT_RADIUS_PX * 2,
                  height: HIT_RADIUS_PX * 2,
                  marginLeft: -HIT_RADIUS_PX,
                  marginTop: -HIT_RADIUS_PX,
                  borderRadius: "50%",
                  background: "var(--accent)",
                  border: "1px solid var(--surface)",
                  padding: 0,
                  cursor: "grab",
                }}
              />
            ))}
          </div>
        ))}
      </div>
    </div>
  );
}
