/**
 * ClipStrip — the clips on a track, drawn as chips you can select,
 * move and delete (#103).
 *
 * Before this, a track split by an interior cut rendered as one
 * continuous lane. #83 gave the UI a *joined* waveform so the lane
 * wasn't blank, which was right and is also why the split became
 * invisible rather than obviously broken. The engine, tools and
 * persistence all understood clips; nothing on screen did.
 *
 * Gesture rules
 * -------------
 * The ticket's warning is that the lane surface already owns range
 * selection and loop dragging, and inferring intent from hit-testing
 * alone means a range drag can start moving audio.
 *
 * So this strip does not overlay the waveform. It is its own row above
 * it, and the waveform's pointer handlers are not touched at all — the
 * existing gestures keep working by construction rather than by
 * arbitration. That is a stronger guarantee than a modifier key, and it
 * costs a row of vertical space.
 *
 * Within the strip:
 *
 *   - pointerdown on a chip     → select it
 *   - move more than DRAG_SLOP  → begin a move; the chip follows
 *   - release                   → commit once, if it actually moved
 *   - Delete / Backspace        → remove the focused chip
 *   - arrows                    → nudge by 1% of the timeline
 *
 * A click that never exceeds DRAG_SLOP selects and writes nothing. That
 * matters because every write appends a session node, and a
 * select-by-accident that also rewrote the arrangement would be a
 * spurious undo step at best.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import type { ClipSummary } from "../lib/tauri-bridge";

/** Pointer travel, in px, before a press counts as a drag. */
const DRAG_SLOP = 3;

const STRIP_HEIGHT = 22;

/**
 * Motion — see docs/motion-audit.md, which called this the biggest one
 * in the app and deferred it out of the vocabulary change.
 *
 * A cut, a split or a paste rearranges this strip. Until now every clip
 * teleported to its new place, so the only way to know what an edit did
 * was to remember where things were and compare. A chip that *travels*
 * to its new position says which clip it was and how far it moved,
 * which is the whole "explaining a change" job.
 *
 * `left`/`width` are laid out rather than composited, and normally that
 * would be the wrong pair to animate. It is fine here and worth being
 * explicit about why:
 *
 *   - The chips are `position: absolute` inside a fixed-height box, so
 *     changing one lays out nothing else on the page.
 *   - There are a handful of them, not thousands.
 *   - It runs when the clip list changes — never during playback, never
 *     during a drag, never on the render path.
 *
 * The alternative, translating on the compositor, needs pixel
 * measurement because a percentage `translateX` resolves against the
 * element's own width rather than the container's. That is real
 * machinery, and a resize observer to keep it honest, to animate two
 * properties on ten elements once per edit.
 */
const CLIP_MOTION = `left var(--dur-3) var(--ease-in-out), width var(--dur-3) var(--ease-in-out)`;

export interface ClipStripProps {
  clips: ClipSummary[];
  /** Timeline duration in seconds; 0 while audio is still loading. */
  duration: number;
  selectedClip: number | null;
  onSelectClip: (clipIndex: number | null) => void;
  /** Commit a move. Called once per gesture, on release. */
  onMoveClip?: (clipIndex: number, startSec: number) => void;
  onRemoveClip?: (clipIndex: number) => void;
  trackName: string;
}

interface DragState {
  clipIndex: number;
  /** Pointer x where the press started. */
  originX: number;
  /** The clip's start when the press began. */
  originStart: number;
  moved: boolean;
}

/** Last path segment, for the chip label. */
export function clipLabel(clip: ClipSummary, index: number): string {
  const stem = clip.source_path.split(/[\\/]/).pop() ?? "";
  return stem || `clip ${index + 1}`;
}

export function ClipStrip({
  clips,
  duration,
  selectedClip,
  onSelectClip,
  onMoveClip,
  onRemoveClip,
  trackName,
}: ClipStripProps) {
  const stripRef = useRef<HTMLDivElement>(null);
  // Local starts so a drag can move a chip without a round trip per
  // frame. Replaced wholesale whenever the session says otherwise.
  const [draft, setDraft] = useState<ClipSummary[]>(clips);
  const [drag, setDrag] = useState<DragState | null>(null);

  // Whether a chip has ever been painted at a real position.
  //
  // Without this, the first paint animates every chip in from the left
  // edge — the browser transitions from the initial `left: 0` to the
  // computed one. That is a loading flourish nobody asked for, on the
  // surface that most needs to look settled, and it would fire again
  // every time the track is switched.
  const painted = useRef(false);
  useEffect(() => {
    painted.current = true;
  }, []);

  useEffect(() => {
    setDraft(clips);
  }, [clips]);

  const pxToSec = useCallback(
    (px: number): number => {
      const width = stripRef.current?.getBoundingClientRect().width ?? 0;
      if (width === 0 || duration <= 0) return 0;
      return (px / width) * duration;
    },
    [duration],
  );

  const xOf = (sec: number) => (duration > 0 ? (sec / duration) * 100 : 0);

  useEffect(() => {
    if (!drag) return;
    const onMove = (e: PointerEvent) => {
      const dx = e.clientX - drag.originX;
      if (!drag.moved && Math.abs(dx) < DRAG_SLOP) return;
      if (!drag.moved) setDrag({ ...drag, moved: true });
      const next = Math.max(0, drag.originStart + pxToSec(dx));
      setDraft((prev) =>
        prev.map((c, i) =>
          i === drag.clipIndex ? { ...c, start_sec: next } : c,
        ),
      );
    };
    const onUp = (e: PointerEvent) => {
      const dx = e.clientX - drag.originX;
      const moved = drag.moved || Math.abs(dx) >= DRAG_SLOP;
      setDrag(null);
      if (!moved) return; // a click, not a drag — selection only
      onMoveClip?.(drag.clipIndex, Math.max(0, drag.originStart + pxToSec(dx)));
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    return () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
    };
  }, [drag, pxToSec, onMoveClip]);

  const onKey = useCallback(
    (e: React.KeyboardEvent, clipIndex: number) => {
      const clip = draft[clipIndex];
      if (!clip) return;
      if (e.key === "Delete" || e.key === "Backspace") {
        e.preventDefault();
        onRemoveClip?.(clipIndex);
        return;
      }
      const step = (e.shiftKey ? 0.001 : 0.01) * duration;
      let next: number | null = null;
      if (e.key === "ArrowLeft") next = Math.max(0, clip.start_sec - step);
      if (e.key === "ArrowRight") next = clip.start_sec + step;
      if (next === null) return;
      e.preventDefault();
      setDraft((prev) =>
        prev.map((c, i) => (i === clipIndex ? { ...c, start_sec: next } : c)),
      );
      onMoveClip?.(clipIndex, next);
    },
    [draft, duration, onMoveClip, onRemoveClip],
  );

  return (
    <div
      data-testid="clip-strip"
      style={{
        display: "flex",
        background: "var(--surface)",
        borderBottom: "1px solid var(--border)",
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
        {draft.length === 1 ? "1 clip" : `${draft.length} clips`}
      </div>
      <div
        ref={stripRef}
        role="group"
        aria-label={`${trackName} clips`}
        style={{ flex: 1, position: "relative", height: STRIP_HEIGHT }}
      >
        {draft.map((clip, i) => {
          const selected = selectedClip === i;
          // The chip under the pointer must not ease. It is already
          // following the finger frame by frame, and a 320ms transition
          // on top of that turns a direct-manipulation drag into
          // something that trails behind the cursor and overshoots on
          // release — the exact feel this whole change exists to avoid.
          // Only the chips being *rearranged by an edit* travel.
          const dragging = drag?.clipIndex === i;
          return (
            <button
              type="button"
              key={i}
              data-testid={`clip-chip-${i}`}
              data-selected={selected}
              aria-pressed={selected}
              aria-label={`${clipLabel(clip, i)}, ${clip.start_sec.toFixed(
                2,
              )} to ${(clip.start_sec + clip.length_sec).toFixed(2)} seconds`}
              onPointerDown={(e) => {
                onSelectClip(i);
                setDrag({
                  clipIndex: i,
                  originX: e.clientX,
                  originStart: clip.start_sec,
                  moved: false,
                });
              }}
              onKeyDown={(e) => onKey(e, i)}
              style={{
                position: "absolute",
                left: `${xOf(clip.start_sec)}%`,
                width: `${xOf(clip.length_sec)}%`,
                top: 2,
                height: STRIP_HEIGHT - 4,
                // A one-sample clip must still be grabbable.
                minWidth: 8,
                background: selected
                  ? "var(--accent-soft)"
                  : "var(--surface-elev-2)",
                border: "1px solid",
                borderColor: selected
                  ? "var(--accent)"
                  : "var(--border-strong)",
                borderRadius: 3,
                color: selected ? "var(--accent)" : "var(--text-dim)",
                fontFamily: "var(--font-mono)",
                fontSize: 9,
                textAlign: "left",
                padding: "0 4px",
                overflow: "hidden",
                whiteSpace: "nowrap",
                textOverflow: "ellipsis",
                // The chip said `grab` and kept saying it while being
                // dragged, so the one moment the pointer was actually
                // holding something looked identical to hovering over
                // it (#211 phase 2).
                cursor: dragging ? "grabbing" : "grab",
                transition:
                  dragging || !painted.current ? undefined : CLIP_MOTION,
              }}
              data-motion={
                dragging || !painted.current ? "none" : "clip-travel"
              }
            >
              {clipLabel(clip, i)}
            </button>
          );
        })}
      </div>
    </div>
  );
}
