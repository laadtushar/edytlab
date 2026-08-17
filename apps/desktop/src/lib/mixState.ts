/**
 * Whether the rendered mix still describes the session you are looking at.
 *
 * `render_preview` names its output after the node it rendered
 * (`edytlab-preview-{node_id}.wav`). That makes a stale preview path
 * *byte-identical* to a current one, so re-rendering a stale head returns
 * the same string, `setState` hits React's bailout, the load effect never
 * fires, and nothing reloads — silently, with no error. Recording which
 * node a mix came from is what makes "stale" expressible at all.
 *
 * Extracted here rather than left inline in `App.tsx` because the repo
 * already tests App's state rules this way (see `lib/undoRedo.ts`), and
 * because `absent` and `stale` are easy to conflate and worth pinning.
 */

export interface MixState {
  /** Path of the last rendered mix, or null if nothing has been rendered. */
  mixPath: string | null;
  /** The node that mix was rendered from. */
  mixNodeId: string | null;
}

/** No mix rendered. */
export const NO_MIX: MixState = { mixPath: null, mixNodeId: null };

/**
 * True when a mix exists but the session has moved on since it was made.
 *
 * Absent is deliberately **not** stale: before anything has been
 * rendered there is nothing to be out of date, and saying otherwise
 * would report a state the user cannot act on.
 */
export function mixIsStale(mix: MixState, head: string | null): boolean {
  return mix.mixPath !== null && mix.mixNodeId !== head;
}

/**
 * The mix state after an edit that advanced the session.
 *
 * Clearing rather than keeping: a mix of the previous node is not a
 * worse version of the current one, it is a recording of something else.
 */
export function afterSessionAdvanced(): MixState {
  return NO_MIX;
}
