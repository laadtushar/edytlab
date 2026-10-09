/**
 * useSession — owns project lifecycle + head-node tracking.
 *
 * Responsibilities:
 *  - holds the currently opened {@link ProjectInfo} (path + head node).
 *  - subscribes to `agent://node-created` so the head pointer follows
 *    every agent-produced edit without an explicit `getSessionHead`
 *    round-trip after each turn — and reads `getSessionHead` once at
 *    mount, for the head that was already there before any event.
 *  - exposes a `renderPreview` helper that resolves the latest node id
 *    to a temp WAV path via the bridge.
 *  - keeps the path the user took (undo trail and redo list, #398)
 *    beside the head, in one reducer (`lib/headTrail.ts`), so undo and
 *    redo follow the steps taken rather than the node's first parent.
 *
 * Pure: the hook only schedules effects in `useEffect` and never
 * mutates state during render. The single `useEffect` that wires the
 * `onNodeCreated` listener returns its async unlisten so React can
 * detach it on unmount; this avoids the duplicated-listener leak that
 * would otherwise happen under StrictMode's double-mount.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import {
  headTrailReducer,
  initialHeadTrail,
  redoTarget,
  undoTarget,
  type HeadAction,
} from "../lib/headTrail";

import {
  getSessionHead,
  onNodeCreated,
  openProject as bridgeOpenProject,
  renderPreview as bridgeRenderPreview,
  type NodeId,
  type ProjectInfo,
} from "../lib/tauri-bridge";

export interface UseSessionResult {
  project: ProjectInfo | null;
  /** Latest known head node id: read once at mount (a returning user's
   * restored head), then kept current off the event stream. */
  head: NodeId | null;
  openProject: (path: string) => Promise<void>;
  /** Render the latest head node to a temp WAV. Throws if no head. */
  renderHead: () => Promise<string>;
  /**
   * The head moved on: an edit, an agent step, a click in the graph, an
   * A/B accept. The head it leaves goes on the undo trail and the redo
   * list is dropped, because this starts a new branch. Moving to the
   * head it already has changes nothing.
   *
   * This only changes the frontend's idea of the head; commands that
   * made the move on the backend have already done so, and the graph's
   * click is a preview. Every head move in the frontend must come
   * through here (or `resetHead`): a head moved any other way is one
   * undo does not know about (`lib/headTrail.ts`).
   */
  setHeadLocal: (nodeId: NodeId) => void;
  /**
   * A different project is open: its head replaces this one's and none of
   * the old path applies. `null` is a project with nothing in it yet.
   */
  resetHead: (nodeId: NodeId | null) => void;
  /**
   * The heads the user has been on this session, oldest first, not
   * including `head`. Undo goes to its last entry: the node's stored
   * parent is the wrong answer once an edit has returned to a state
   * reached before (see `lib/headTrail.ts`).
   */
  trail: readonly NodeId[];
  /**
   * Where undo would go, read from the latest state rather than the
   * latest render. A key pressed right after an edit can reach a handler
   * that closed over the render before it; this cannot be stale. `null`
   * when there is no head. `to` is `null` when the trail is spent: the
   * caller then asks the node for its parent.
   */
  peekUndo: () => { from: NodeId; to: NodeId | null } | null;
  /** Where redo would go, read the same way; `null` when there is nothing to redo. */
  peekRedo: () => { from: NodeId; to: NodeId } | null;
  /** Undo moved the backend from `from` to `to`: pop the trail, keep `from` for redo. */
  stepBack: (from: NodeId, to: NodeId) => void;
  /** Redo moved the backend from `from` to `to`: `from` goes back on the trail. */
  stepForward: (from: NodeId, to: NodeId) => void;
  /** Last error to bubble up from a session command, for surface in UI. */
  error: string | null;
}

export function useSession(): UseSessionResult {
  const [project, setProject] = useState<ProjectInfo | null>(null);
  // The head, the undo trail and the redo list, in one reducer. Held in a
  // ref as well as in state: undo and redo read it through `peekUndo` /
  // `peekRedo`, which must see a move made a moment ago even if React has
  // not rendered it yet (a key pressed right after an edit).
  const historyRef = useRef(initialHeadTrail);
  const [history, setHistory] = useState(initialHeadTrail);
  const { head, trail } = history;
  const dispatchHead = useCallback((action: HeadAction) => {
    const next = headTrailReducer(historyRef.current, action);
    if (next === historyRef.current) return;
    historyRef.current = next;
    setHistory(next);
  }, []);
  const [error, setError] = useState<string | null>(null);

  // Subscribe to node-created so the head pointer follows the agent's
  // edits. Cleanup detaches the listener — important under StrictMode.
  //
  // And read the head once, at mount. The subscription only reports
  // *changes*; a head that was already there when the app started —
  // every returning user's, since the backend reopens the project and
  // restores `HEAD` from disk — never arrives as an event. So a returning
  // user had no head at all, and "preview" silently did nothing (#332).
  //
  // The event wins a race: if an edit lands before this read resolves,
  // its head is newer than the one read, so the read only fills a head
  // that is still unknown. A rejection leaves it unknown, which is what a
  // fresh project's `NoSession` means and what the boot `listTracks`
  // does with a failure too.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    onNodeCreated((nodeId) => {
      dispatchHead({ type: "advance", head: nodeId });
    }).then((fn) => {
      if (cancelled) {
        fn();
      } else {
        unlisten = fn;
      }
    });
    getSessionHead()
      .then((restored) => {
        if (!cancelled) dispatchHead({ type: "fill", head: restored });
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [dispatchHead]);

  const openProject = useCallback(async (path: string) => {
    try {
      const info = await bridgeOpenProject(path);
      setProject(info);
      dispatchHead({ type: "reset", head: info.head });
      setError(null);
    } catch (e) {
      setError(String(e));
      throw e;
    }
  }, [dispatchHead]);

  const renderHead = useCallback(async (): Promise<string> => {
    if (!head) {
      throw new Error("no node to render");
    }
    return bridgeRenderPreview(head);
  }, [head]);

  const setHeadLocal = useCallback(
    (nodeId: NodeId) => dispatchHead({ type: "advance", head: nodeId }),
    [dispatchHead],
  );
  const resetHead = useCallback(
    (nodeId: NodeId | null) => dispatchHead({ type: "reset", head: nodeId }),
    [dispatchHead],
  );
  const stepBack = useCallback(
    (from: NodeId, to: NodeId) => dispatchHead({ type: "stepBack", from, to }),
    [dispatchHead],
  );
  const stepForward = useCallback(
    (from: NodeId, to: NodeId) => dispatchHead({ type: "stepForward", from, to }),
    [dispatchHead],
  );
  const peekUndo = useCallback(() => {
    const { head: from, trail: path } = historyRef.current;
    return from === null ? null : { from, to: undoTarget(path, from, null) };
  }, []);
  const peekRedo = useCallback(() => {
    const { head: from, redo } = historyRef.current;
    if (from === null) return null;
    const to = redoTarget(redo, from);
    return to === null ? null : { from, to };
  }, []);

  return {
    project,
    head,
    openProject,
    renderHead,
    setHeadLocal,
    resetHead,
    trail,
    peekUndo,
    peekRedo,
    stepBack,
    stepForward,
    error,
  };
}
