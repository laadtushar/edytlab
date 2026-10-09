/**
 * The path the user took through the session's history (#398).
 *
 * Undo used to follow `node.parent`. But a node's id is a hash of its
 * state alone (`NodeId::from_state`), so an edit that returns to an
 * earlier state (mute, then unmute) lands on a node that already
 * exists, and that node keeps the parent it had the FIRST time it was
 * reached (`Store::append` skips the write for a known id). From there
 * "the parent" is the step before the original visit: undo skipped the
 * mute and went on to take back whatever came before it. The graph is
 * deliberately content-addressed, so the fix is not in the graph: undo
 * needs the path, and the path is kept here.
 *
 * One reducer owns the head, the undo trail and the redo list, so the
 * three cannot drift apart:
 *
 * - `trail`: the heads the user has been on, oldest first, not including
 *   the current one. An edit pushes the head it leaves; undo pops it.
 * - `redo`: where undo came from, most recent last. Undo pushes the head
 *   it leaves; redo pops it; any new head (an edit, an agent step, a
 *   jump in the graph) clears it, because it starts a new branch.
 *
 * Every frontend head move goes through `useSession`, which holds this
 * state: `setHeadLocal` for a head that moved on, `resetHead` for a
 * different project. A head moved any other way is a head the path does
 * not know about, and undo would go to the wrong place.
 *
 * It lives for the session. After a restart it is empty and undo falls
 * back to the node's parent, which is right whenever the history has not
 * revisited a state. Persisting the path belongs in the store (the view
 * file is disposable by design), so it is a follow-up.
 */

/** Long histories are bounded; the oldest steps fall off the far end. */
export const MAX_TRAIL = 500;

export interface HeadTrail {
  head: string | null;
  trail: string[];
  redo: string[];
}

export type HeadAction =
  /** An edit, an agent step, a jump in the graph: the head moved on. */
  | { type: "advance"; head: string }
  /** The head the backend already had, read once at mount. Fills a gap, never overrides. */
  | { type: "fill"; head: string | null }
  /** A different project: nothing of the old path applies. */
  | { type: "reset"; head: string | null }
  /** Undo moved from `from` to `to`: the trail's top, or the node's parent. */
  | { type: "stepBack"; from: string; to: string }
  /** Redo moved from `from` to `to`; `from` is where undo returns to. */
  | { type: "stepForward"; from: string; to: string };

export const initialHeadTrail: HeadTrail = { head: null, trail: [], redo: [] };

function push(stack: string[], id: string): string[] {
  const next = [...stack, id];
  return next.length > MAX_TRAIL ? next.slice(next.length - MAX_TRAIL) : next;
}

/**
 * Take off `stack` what a move from `from` to `to` has used up: entries
 * naming `from` (the selectors skip them, so they are spent too), then
 * the entry for `to` itself, if that is where the move came from. A
 * target that did not come off the stack (undo falling back to the
 * node's parent) leaves it alone.
 */
function popTo(stack: string[], from: string, to: string): string[] {
  let end = stack.length;
  while (end > 0 && stack[end - 1] === from) end--;
  if (end > 0 && stack[end - 1] === to) end--;
  return end === stack.length ? stack : stack.slice(0, end);
}

export function headTrailReducer(state: HeadTrail, action: HeadAction): HeadTrail {
  switch (action.type) {
    case "advance": {
      // The same head again (an edit that changed nothing, an event
      // echoing a move already made) is not a step, and must not throw
      // away the redo list.
      if (action.head === state.head) return state;
      return {
        head: action.head,
        trail: state.head ? push(state.trail, state.head) : state.trail,
        redo: [],
      };
    }
    case "fill":
      return state.head !== null || action.head === null
        ? state
        : { head: action.head, trail: [], redo: [] };
    case "reset":
      return { head: action.head, trail: [], redo: [] };
    case "stepBack":
      // `set_head_to` already succeeded, so the head is `to` whatever
      // this state believed the head was: the frontend mirrors the
      // backend, not the other way round.
      return {
        head: action.to,
        trail: popTo(state.trail, action.from, action.to),
        redo: push(state.redo, action.from),
      };
    case "stepForward":
      return {
        head: action.to,
        trail: push(state.trail, action.from),
        redo: popTo(state.redo, action.from, action.to),
      };
  }
}

/**
 * Where undo goes from `head`: the head the user was on before it, or,
 * when the trail has nothing left, the node's stored parent.
 *
 * Trailing entries equal to the current head are skipped. The reducer
 * does not produce them (consecutive heads on a path always differ), but
 * an undo "to" the head it is already on would change nothing, still pop,
 * and leave the user pressing the key with no visible effect.
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

/**
 * Where redo goes from `head`: the last place undo came from, if any.
 * Like `undoTarget`, it steps over entries naming the head itself.
 */
export function redoTarget(redo: readonly string[], head: string): string | null {
  for (let i = redo.length - 1; i >= 0; i--) {
    if (redo[i] !== head) return redo[i];
  }
  return null;
}
