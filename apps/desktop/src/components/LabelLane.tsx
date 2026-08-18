/**
 * The lane you type chapter names into (#203 §1).
 *
 * `MarkerLayer` draws flags *over* the waveform, which is right for
 * seeing where a mark is and wrong for working with one: the flags sit
 * on top of the audio, so every gesture there has to be shared with
 * selection and scrubbing. This is a lane of its own, below the tracks,
 * where a click means "label" and nothing else.
 *
 * What it supports, and why each is a direct gesture rather than a
 * sentence to the agent:
 *
 * * **Add** — double-click empty lane. Naming a chapter while listening
 *   is a dozen-times-an-episode action; a round trip through the model
 *   for each is absurd. Double rather than single because a single
 *   click in an empty lane is how you deselect, and a label per stray
 *   click would be maddening.
 * * **Rename** — double-click the chip, type, Enter. Escape reverts.
 * * **Move** — drag the chip. The time under the pointer is what it
 *   gets, so it lands where you looked.
 * * **Delete** — right-click the chip.
 *
 * Positions are seconds on the session axis, the same axis the timeline
 * and the ruler use, so a label lines up with the audio at any zoom.
 */

import { useEffect, useRef, useState } from "react";

import type { Marker } from "../lib/tauri-bridge";

export interface LabelLaneProps {
  labels: Marker[];
  /** Session length in seconds; the lane spans it exactly. */
  duration: number;
  /** Left offset matching the track sidebar, so 0s lines up. */
  sidebarWidth?: number;
  onAdd: (timeSec: number) => void;
  onRename: (id: string, name: string) => void;
  onMove: (id: string, timeSec: number) => void;
  onRemove: (id: string) => void;
  onSeek?: (timeSec: number) => void;
}

/** Where a label sits, whichever kind it is. */
function startOf(m: Marker): number {
  return m.kind === "marker" ? m.time_sec : m.start_sec;
}

export function LabelLane({
  labels,
  duration,
  sidebarWidth = 132,
  onAdd,
  onRename,
  onMove,
  onRemove,
  onSeek,
}: LabelLaneProps) {
  const laneRef = useRef<HTMLDivElement>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [dragging, setDragging] = useState<string | null>(null);
  // Where a drag is *shown* before it is committed, so the chip tracks
  // the pointer without a session node per mouse-move.
  const [dragSec, setDragSec] = useState<number | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (editing) inputRef.current?.select();
  }, [editing]);

  /** Pointer x → seconds on the session axis. */
  function secondsAt(clientX: number): number {
    const el = laneRef.current;
    if (!el || duration <= 0) return 0;
    const r = el.getBoundingClientRect();
    const frac = (clientX - r.left) / Math.max(1, r.width);
    return Math.min(duration, Math.max(0, frac * duration));
  }

  // Drag is tracked on `window`, not on the chip: releasing the button
  // outside the lane is a normal way to end a drag, and a chip-scoped
  // listener would simply never hear it and leave the label stuck to
  // the pointer.
  useEffect(() => {
    if (!dragging) return;
    const move = (e: MouseEvent) => setDragSec(secondsAt(e.clientX));
    const up = (e: MouseEvent) => {
      const t = secondsAt(e.clientX);
      const id = dragging;
      setDragging(null);
      setDragSec(null);
      const original = labels.find((l) => l.id === id);
      // A click that did not really move is a click, not an edit.
      if (original && Math.abs(startOf(original) - t) > 0.01) onMove(id, t);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
    return () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dragging, duration, labels]);

  function commitRename(id: string) {
    const name = draft.trim();
    setEditing(null);
    const original = labels.find((l) => l.id === id);
    if (name && original && name !== original.name) onRename(id, name);
  }

  return (
    <div
      data-testid="label-lane"
      style={{
        display: "flex",
        alignItems: "stretch",
        height: 28,
        borderTop: "1px solid var(--border)",
        background: "var(--surface-elev, rgba(255,255,255,0.02))",
        flexShrink: 0,
      }}
    >
      <div
        style={{
          width: sidebarWidth,
          flexShrink: 0,
          display: "flex",
          alignItems: "center",
          paddingLeft: 8,
          fontSize: 10,
          fontFamily: "var(--font-mono)",
          textTransform: "uppercase",
          letterSpacing: "0.08em",
          color: "var(--text-dim)",
          borderRight: "1px solid var(--border)",
        }}
      >
        Labels
      </div>

      <div
        ref={laneRef}
        data-testid="label-lane-track"
        onDoubleClick={(e) => {
          // Double-click rather than single: a single click in an empty
          // lane is how you *deselect*, and creating a label every time
          // someone clicks past a chip would be maddening.
          if (e.target !== e.currentTarget) return;
          onAdd(secondsAt(e.clientX));
        }}
        title="Double-click to add a label"
        style={{ position: "relative", flex: 1, cursor: "copy", overflow: "hidden" }}
      >
        {duration > 0 &&
          labels.map((m) => {
            const t = dragging === m.id && dragSec !== null ? dragSec : startOf(m);
            const pct = (t / duration) * 100;
            const isEditing = editing === m.id;
            return (
              <div
                key={m.id}
                data-testid="label-chip"
                data-label-name={m.name}
                style={{
                  position: "absolute",
                  left: `${pct}%`,
                  top: 3,
                  transform: "translateX(-1px)",
                  display: "flex",
                  alignItems: "center",
                  gap: 3,
                }}
              >
                <div
                  aria-hidden
                  style={{
                    width: 1,
                    height: 22,
                    background: "var(--accent)",
                    opacity: 0.8,
                    flexShrink: 0,
                  }}
                />
                {isEditing ? (
                  <input
                    ref={inputRef}
                    data-testid="label-chip-input"
                    value={draft}
                    onChange={(e) => setDraft(e.target.value)}
                    onBlur={() => commitRename(m.id)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") commitRename(m.id);
                      // Escape abandons the edit. Without it the only
                      // way out of a mistyped rename is to retype the
                      // original exactly.
                      if (e.key === "Escape") setEditing(null);
                      e.stopPropagation();
                    }}
                    style={{
                      fontSize: 10,
                      fontFamily: "var(--font-mono)",
                      padding: "1px 4px",
                      width: 110,
                      background: "var(--surface)",
                      color: "var(--text)",
                      border: "1px solid var(--accent)",
                      borderRadius: 2,
                    }}
                  />
                ) : (
                  <button
                    type="button"
                    data-testid="label-chip-button"
                    aria-label={`Label ${m.name}`}
                    onMouseDown={(e) => {
                      if (e.button !== 0) return;
                      setDragging(m.id);
                      setDragSec(startOf(m));
                    }}
                    onClick={() => onSeek?.(startOf(m))}
                    onDoubleClick={(e) => {
                      e.stopPropagation();
                      setDraft(m.name);
                      setEditing(m.id);
                    }}
                    onContextMenu={(e) => {
                      e.preventDefault();
                      onRemove(m.id);
                    }}
                    title={`${m.name} — drag to move, double-click to rename, right-click to delete`}
                    style={{
                      fontSize: 10,
                      fontFamily: "var(--font-mono)",
                      padding: "1px 5px",
                      background: "var(--accent)",
                      color: "var(--onyx-0, #07080b)",
                      border: "none",
                      borderRadius: 2,
                      whiteSpace: "nowrap",
                      cursor: dragging === m.id ? "grabbing" : "grab",
                      maxWidth: 160,
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                    }}
                  >
                    {m.name}
                  </button>
                )}
              </div>
            );
          })}
      </div>
    </div>
  );
}
