/**
 * The path the user took through the session's history.
 *
 * Undo used to follow `node.parent`. But a node's id is a hash of its
 * state alone, so an edit that returns to an earlier state (mute, then
 * unmute) lands on a node that already exists, and that node keeps the
 * parent it had the first time it was reached. From there "the parent"
 * is the step before the *original* visit: undo skipped the mute and
 * went on to take back whatever came before it. The graph is
 * deliberately content-addressed, so the fix is not in the graph: undo
 * needs the path, and the path is kept here.
 *
 * `trail` is the heads the user has been on, oldest first, not including
 * the current one. An edit pushes the head it leaves; undo pops; redo
 * pushes the head it leaves. It lives for the session: after a restart
 * it is empty and undo falls back to the node's parent, which is right
 * whenever the history has not revisited a state.
 */

/** Long histories are bounded; the oldest steps fall off the far end. */
export const MAX_TRAIL = 500;

export interface HeadTrail {
  head: string | null;
  trail: string[];
}

export type HeadAction =
  /** An edit, an agent step, a jump in the graph: the head moved on. */
  | { type: "advance"; head: string }
  /** The head the backend already had, read once at mount. Fills a gap, never overrides. */
  | { type: "fill"; head: string | null }
  /** A different project: nothing of the old path applies. */
  | { type: "reset"; head: string | null }
  /** Undo moved to `head`, which was the trail's top (or the node's parent). */
  | { type: "stepBack"; head: string }
  /** Redo moved to `head`; the head it left is where undo returns to. */
  | { type: "stepForward"; head: string };

export const initialHeadTrail: HeadTrail = { head: null, trail: [] };

function push(trail: string[], id: string): string[] {
  const next = [...trail, id];
  return next.length > MAX_TRAIL ? next.slice(next.length - MAX_TRAIL) : next;
}

export function headTrailReducer(state: HeadTrail, action: HeadAction): HeadTrail {
  switch (action.type) {
    case "advance": {
      if (action.head === state.head) return state;
      return {
        head: action.head,
        trail: state.head ? push(state.trail, state.head) : state.trail,
      };
    }
    case "fill":
      return state.head !== null || action.head === null
        ? state
        : { head: action.head, trail: [] };
    case "reset":
      return { head: action.head, trail: [] };
    case "stepBack": {
      // Pop the entry we are returning to. If the target did not come
      // from the trail (the fallback to the parent) there is nothing to
      // pop, and the trail is left alone.
      const top = state.trail[state.trail.length - 1];
      return {
        head: action.head,
        trail: top === action.head ? state.trail.slice(0, -1) : state.trail,
      };
    }
    case "stepForward":
      return {
        head: action.head,
        trail: state.head ? push(state.trail, state.head) : state.trail,
      };
  }
}

/**
 * Where undo goes from `head`: the head the user was on before it, or,
 * when the trail has nothing left, the node's stored parent.
 *
 * Entries equal to the current head are skipped. They cannot arise from
 * the reducer, but a trail that names the head would make undo a no-op
 * that still pops, which is worse than ignoring it.
 */
export function undoTarget(
  trail: readonly string[],
  head: string,
  parent: string | null,
): string | null {
  for (let i = trail.length - 1; i >= 0; i--) {
    if (trail[i] !== head) return trail[i];
  }
  return parent;
}
