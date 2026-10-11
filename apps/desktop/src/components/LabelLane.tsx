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
 * * **Add** — double-click empty lane, type the name where the label
 *   will sit, Enter. Naming a chapter while listening is a
 *   dozen-times-an-episode action; a round trip through the model for
 *   each is absurd. Double rather than single because a single click in
 *   an empty lane is how you deselect, and a label per stray click
 *   would be maddening. The name is typed in the lane rather than asked
 *   for with `window.prompt`: a macOS webview answers a prompt with
 *   `null` (WKWebView only shows one when the host implements the
 *   panel, and neither wry nor the dialog plugin does), so there the
 *   double-click would have done nothing.
 * * **Rename** — double-click the chip, type, Enter. Escape reverts.
 * * **Move** — drag the chip. It moves by how far the pointer travelled
 *   from where the chip was grabbed, so it lands where it is shown
 *   while held; a press that barely moves is a click.
 * * **Delete** — right-click the chip.
 *
 * A region label (an Audacity label with a duration) is drawn as its chip
 * at the start plus a bar for its extent, and moves as a whole.
 *
 * Positions are seconds on the session axis, the same axis the timeline
 * and the ruler use, so a label lines up with the audio at any zoom.
 */

import { useEffect, useRef, useState } from "react";

import type { Marker } from "../lib/tauri-bridge";
import { pctOf, type TimeSpan } from "../lib/timelineViewport";

export interface LabelLaneProps {
  labels: Marker[];
  /** Session length in seconds. */
  duration: number;
  /**
   * The stretch of the session on screen, from the timeline's one axis
   * (#344). Absent or null is the whole session, which is what fit
   * shows.
   */
  view?: TimeSpan | null;
  /** Left offset matching the track sidebar, so 0s lines up. */
  sidebarWidth?: number;
  /** A label named `name`, at `timeSec`. The name is trimmed and not empty. */
  onAdd: (timeSec: number, name: string) => void;
  onRename: (id: string, name: string) => void;
  onMove: (id: string, timeSec: number) => void;
  onRemove: (id: string) => void;
  onSeek?: (timeSec: number) => void;
}

/** How far the pointer must travel, in pixels, before a press is a drag. */
const DRAG_PX = 3;

/** Room the box for a new label's name needs to the right of its flag. */
const ADD_BOX_PX = 130;

/** Where a label sits, whichever kind it is. */
function startOf(m: Marker): number {
  return m.kind === "marker" ? m.time_sec : m.start_sec;
}

export function LabelLane({
  labels,
  duration,
  view,
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
  // How the chip was grabbed. The chip is a button wider than the flag it
  // hangs from, so a press lands some way along from the label's own
  // time; a drag moves the label by how far the pointer travels from
  // there, and a press that never travels `DRAG_PX` is a click.
  const grab = useRef<{ x: number; offset: number; moved: boolean } | null>(null);
  // Set when a drag was released, so the click that follows it on the
  // chip is not also taken for a request to seek.
  const draggedRef = useRef(false);
  const inputRef = useRef<HTMLInputElement>(null);
  // A label being added: where it will go, and the name typed so far.
  // `flip`: too near the right edge to open the box to the flag's right,
  // where the lane would clip it and hide what is being typed.
  const [adding, setAdding] = useState<{ time: number; name: string; flip: boolean } | null>(
    null,
  );
  const addingRef = useRef<HTMLInputElement>(null);
  // Set when adding is finished, so the blur that follows Enter (or the
  // unmount that follows Escape) cannot finish it a second time.
  const addingDone = useRef(false);

  useEffect(() => {
    if (editing) inputRef.current?.select();
  }, [editing]);

  useEffect(() => {
    if (adding) addingRef.current?.focus();
    // Focus once, when the box opens; typing must not steal it back.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [adding === null]);

  // Drawn on the lanes' axis: at fit, the whole session; zoomed, only
  // what is on screen.
  const shown: TimeSpan = view ?? { start: 0, end: duration };

  /** Pointer x → seconds on the session axis. */
  function secondsAt(clientX: number): number {
    const el = laneRef.current;
    if (!el || duration <= 0) return 0;
    const r = el.getBoundingClientRect();
    const frac = (clientX - r.left) / Math.max(1, r.width);
    const t = shown.start + frac * (shown.end - shown.start);
    return Math.min(duration, Math.max(0, t));
  }

  // Drag is tracked on `window`, not on the chip: releasing the button
  // outside the lane is a normal way to end a drag, and a chip-scoped
  // listener would simply never hear it and leave the label stuck to
  // the pointer.
  useEffect(() => {
    if (!dragging) return;
    /** Where the dragged label would land with the pointer at `clientX`. */
    const landing = (clientX: number, offset: number) =>
      Math.min(duration, Math.max(0, secondsAt(clientX) - offset));
    const move = (e: MouseEvent) => {
      const g = grab.current;
      if (!g) return;
      if (!g.moved && Math.abs(e.clientX - g.x) < DRAG_PX) return;
      g.moved = true;
      setDragSec(landing(e.clientX, g.offset));
    };
    const up = (e: MouseEvent) => {
      const g = grab.current;
      grab.current = null;
      const id = dragging;
      setDragging(null);
      setDragSec(null);
      // A press that did not travel is a click, not an edit.
      if (!g?.moved) return;
      draggedRef.current = true;
      const t = landing(e.clientX, g.offset);
      const original = labels.find((l) => l.id === id);
      // Put back where it started is not an edit either.
      if (original && Math.abs(startOf(original) - t) > 0.01) onMove(id, t);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
    return () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dragging, duration, labels, shown.start, shown.end]);

  /** Finish adding: the label if there is a name and it was asked for. */
  function finishAdding(add: boolean) {
    if (addingDone.current || !adding) return;
    addingDone.current = true;
    const name = adding.name.trim();
    const time = adding.time;
    setAdding(null);
    if (add && name) onAdd(time, name);
  }

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
        background: "var(--surface-elev)",
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
          setEditing(null);
          addingDone.current = false;
          const r = e.currentTarget.getBoundingClientRect();
          setAdding({
            time: secondsAt(e.clientX),
            name: "",
            flip: e.clientX - r.left + ADD_BOX_PX > r.width,
          });
        }}
        title="Double-click to add a label"
        style={{ position: "relative", flex: 1, cursor: "copy", overflow: "hidden" }}
      >
        {duration > 0 &&
          labels.map((m) =>
            m.kind === "region" ? (
              // Under the chips, and out of the pointer's way: the bar
              // only says how long the label is.
              <div
                key={`extent-${m.id}`}
                data-testid="label-extent"
                aria-hidden
                style={{
                  position: "absolute",
                  left: `${pctOf(dragging === m.id && dragSec !== null ? dragSec : m.start_sec, shown)}%`,
                  width: `${((m.end_sec - m.start_sec) / Math.max(shown.end - shown.start, 1e-9)) * 100}%`,
                  bottom: 2,
                  height: 3,
                  background: "var(--accent)",
                  opacity: 0.4,
                  borderRadius: 1,
                  pointerEvents: "none",
                }}
              />
            ) : null,
          )}
        {duration > 0 && adding && (
          <div
            data-testid="label-draft"
            style={{
              position: "absolute",
              left: `${pctOf(adding.time, shown)}%`,
              top: 3,
              // Flipped, the box sits to the flag's left, its right edge
              // on the flag.
              transform: adding.flip ? "translateX(calc(-100% + 1px))" : "translateX(-1px)",
              flexDirection: adding.flip ? "row-reverse" : "row",
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
            <input
              ref={addingRef}
              data-testid="label-draft-input"
              aria-label="New label name"
              placeholder="Label name"
              value={adding.name}
              onChange={(e) => setAdding({ ...adding, name: e.target.value })}
              onBlur={() => finishAdding(true)}
              onKeyDown={(e) => {
                if (e.key === "Enter") finishAdding(true);
                // Escape abandons the label, as it abandons a rename.
                if (e.key === "Escape") finishAdding(false);
                // Typing a name is not a shortcut: the window listens
                // for keys, and Space or a letter must reach the box.
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
          </div>
        )}
        {duration > 0 &&
          labels.map((m) => {
            const t = dragging === m.id && dragSec !== null ? dragSec : startOf(m);
            const pct = pctOf(t, shown);
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
                      draggedRef.current = false;
                      grab.current = {
                        x: e.clientX,
                        offset: secondsAt(e.clientX) - startOf(m),
                        moved: false,
                      };
                      setDragging(m.id);
                      setDragSec(startOf(m));
                    }}
                    onClick={() => {
                      if (draggedRef.current) {
                        draggedRef.current = false;
                        return;
                      }
                      onSeek?.(startOf(m));
                    }}
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
                      color: "var(--accent-fg)",
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
