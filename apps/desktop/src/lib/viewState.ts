/**
 * Turning view state into something to apply, and back (#156).
 *
 * `view.json` records where the user was: head, zoom, selection,
 * playhead. Reading it back is not quite a straight assignment, and the
 * awkward parts are worth keeping out of `App.tsx` where they would be
 * three conditionals inside an effect:
 *
 * * **Every field is optional and each is independently missing.** A
 *   project saved by an older build has a head and no zoom; one that
 *   was never played has no playhead. Applying `undefined` as a value
 *   would reset the very things it failed to restore.
 * * **A stale head must not be forced.** The head in `view.json` can
 *   name a node that no longer exists — a project folder copied without
 *   `.audiograph/`, or a store rebuilt. Restoring a head is a request,
 *   and the caller checks it.
 * * **Nonsense is ignored rather than propagated.** A negative zoom or
 *   an inverted selection reads as a corrupt file, and the honest
 *   response is to leave that part of the view alone.
 */

import type { Selection } from "../components/Timeline";
import type { ViewState } from "./tauri-bridge";

/** What a caller should actually do, with only the parts worth doing. */
export interface ViewToApply {
  head?: string;
  zoomPxPerSec?: number;
  selection?: Selection | null;
  playheadSec?: number;
}

/** Upper bound mirrors the timeline's own zoom ceiling. */
const MAX_ZOOM_PX_PER_SEC = 2000;

/**
 * Filter a stored view down to the parts that are present and sane.
 *
 * Returns an object whose absent keys mean "leave this alone", which is
 * the distinction that matters: `{}` is a valid result and means the
 * file had nothing usable in it.
 */
export function viewToApply(view: ViewState | null | undefined): ViewToApply {
  const out: ViewToApply = {};
  if (!view) return out;

  if (typeof view.head === "string" && view.head.length > 0) {
    out.head = view.head;
  }

  const zoom = view.zoom_px_per_sec;
  if (typeof zoom === "number" && Number.isFinite(zoom) && zoom >= 0) {
    // 0 is meaningful — it is the auto-fit sentinel — so the guard is
    // `>= 0` rather than a truthiness check.
    out.zoomPxPerSec = Math.min(zoom, MAX_ZOOM_PX_PER_SEC);
  }

  const sel = view.selection;
  if (Array.isArray(sel) && sel.length === 2) {
    const [start, end] = sel;
    if (
      Number.isFinite(start) &&
      Number.isFinite(end) &&
      start >= 0 &&
      end > start
    ) {
      out.selection = { start, end };
    }
  }

  const play = view.playhead_sec;
  if (typeof play === "number" && Number.isFinite(play) && play >= 0) {
    out.playheadSec = play;
  }

  return out;
}

/**
 * Build the record to persist.
 *
 * Written on the way out rather than accumulated, so what is saved is
 * always the whole current view and never a half-updated one.
 */
export function viewToSave(current: {
  head: string | null;
  zoomPxPerSec: number;
  selection: Selection | null;
  playheadSec: number;
}): ViewState {
  return {
    head: current.head,
    zoom_px_per_sec: current.zoomPxPerSec,
    selection: current.selection
      ? [current.selection.start, current.selection.end]
      : null,
    playhead_sec: current.playheadSec,
  };
}
