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
 * The path is kept with the project (#453), so it survives a restart.
 * `useSession` saves it (`saveHeadTrail`, through `lib/trailWriter.ts`)
 * after every move the user makes, and reads it back as a project opens.
 * The backend keeps the file in the store's directory, not the webview, so
 * it outlives the window; the rule stays here. This reducer is the one
 * place that decides what a move does to the path, and nothing else
 * decides undo: the CLI has none, and the agent's `revert_to` reaches the
 * window as an ordinary step (#568). `restore`, and the optional `saved`
 * of `reset`, take a saved path only if it ends at the head the project
 * opened on. A path that ends anywhere else (a save lost to a crash, a
 * store edited by another client) is not the path to this head, and is
 * not used: undo then falls back to the node's parent, as it did before
 * the path was kept, which is right whenever the history has not
 * revisited a state.
 *
 * The store can also lose nodes the path names: `compact_session` deletes
 * every node off the head's chain, which can take the redo list and a step
 * that returned to an earlier state (mute, then unmute, leaves the mute
 * off the chain), #595. `drop` forgets them, and the path closes up over
 * the gap, as it does when the backend reads a saved path whose nodes are
 * gone.
 */

/** Long histories are bounded; the oldest steps fall off the far end. */
export const MAX_TRAIL = 500;

export interface HeadTrail {
  head: string | null;
  trail: string[];
  redo: string[];
}

/**
 * The path as the backend keeps it with the project: the same three
 * fields. `get_head_trail` answers with it and `save_head_trail` takes
 * it. The backend has already dropped what it cannot vouch for (ids that
 * are not node ids, nodes the project no longer has), and a project with
 * nothing saved answers with a null head and two empty lists.
 */
export type SavedTrail = HeadTrail;

/**
 * Read an answer from `get_head_trail`, whatever it is, as a saved path
 * or as nothing. A reply that is not the shape (a backend that does not
 * know the command, a mock that answers `undefined`) is no path, not an
 * error: the path is a convenience, and opening a project must not
 * depend on it.
 */
export function readSavedTrail(raw: unknown): SavedTrail | null {
  if (typeof raw !== "object" || raw === null) return null;
  const { head, trail, redo } = raw as Record<string, unknown>;
  if (typeof head !== "string" || head === "") return null;
  const ids = (list: unknown): string[] =>
    Array.isArray(list)
      ? list.filter((id): id is string => typeof id === "string" && id !== "")
      : [];
  return { head, trail: bounded(ids(trail)), redo: bounded(ids(redo)) };
}

export type HeadAction =
  /** An edit, an agent step, a jump in the graph: the head moved on. */
  | { type: "advance"; head: string }
  /** The head the backend already had, read once at mount. Fills a gap, never overrides. */
  | { type: "fill"; head: string | null }
  /**
   * A different project: nothing of the old path applies, except the one
   * saved with the project being opened, if it ends at `head`.
   */
  | { type: "reset"; head: string | null; saved?: SavedTrail | null }
  /**
   * The path saved with the project, read after its head was. Used only
   * if it ends at the head this state has and this state has no steps of
   * its own yet: an edit that landed first is newer than anything saved.
   */
  | { type: "restore"; saved: SavedTrail }
  /** Undo moved from `from` to `to`: the trail's top, or the node's parent. */
  | { type: "stepBack"; from: string; to: string }
  /** Redo moved from `from` to `to`; `from` is where undo returns to. */
  | { type: "stepForward"; from: string; to: string }
  /**
   * The store no longer has these nodes (a compaction removed them, #595).
   * Entries naming them go from the trail and the redo list. The head is
   * never one of them: the store does not remove the node it is on.
   */
  | { type: "drop"; nodes: readonly string[] };

export const initialHeadTrail: HeadTrail = { head: null, trail: [], redo: [] };

function bounded(stack: string[]): string[] {
  return stack.length > MAX_TRAIL ? stack.slice(stack.length - MAX_TRAIL) : stack;
}

function push(stack: string[], id: string): string[] {
  return bounded([...stack, id]);
}

/** `head` with `saved` as its path, if `saved` is the path to that head. */
function adopt(head: string | null, saved: SavedTrail | null | undefined): HeadTrail {
  return head !== null && saved?.head === head
    ? { head, trail: bounded(saved.trail), redo: bounded(saved.redo) }
    : { head, trail: [], redo: [] };
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

/**
 * `stack` without entries naming a node in `gone`, closed up over them:
 * where removing one leaves the same head twice in a row, it is kept once,
 * and an entry naming `head` is not left at the end, where it would be a
 * step to the head itself. A stack that names none of `gone` is returned
 * as it is.
 */
function closeUp(stack: string[], gone: ReadonlySet<string>, head: string | null): string[] {
  if (!stack.some((id) => gone.has(id))) return stack;
  const out: string[] = [];
  for (const id of stack) {
    if (!gone.has(id) && out[out.length - 1] !== id) out.push(id);
  }
  while (out.length > 0 && out[out.length - 1] === head) out.pop();
  return out;
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
      return adopt(action.head, action.saved);
    case "restore":
      return state.head === null ||
        state.trail.length > 0 ||
        state.redo.length > 0 ||
        action.saved.head !== state.head
        ? state
        : adopt(state.head, action.saved);
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
    case "drop": {
      const gone = new Set(action.nodes);
      if (state.head !== null) gone.delete(state.head);
      const trail = closeUp(state.trail, gone, state.head);
      const redo = closeUp(state.redo, gone, state.head);
      return trail === state.trail && redo === state.redo
        ? state
        : { head: state.head, trail, redo };
    }
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
