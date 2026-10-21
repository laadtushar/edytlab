/**
 * useSession — owns head-node tracking and the session's line of edits.
 *
 * Responsibilities:
 *  - subscribes to `agent://node-created` so the head pointer follows
 *    every agent-produced edit without an explicit `getSessionHead`
 *    round-trip after each turn — and reads `getSessionHead` once at
 *    mount, for the head that was already there before any event.
 *  - exposes a `renderPreview` helper that resolves the latest node id
 *    to a temp WAV path via the bridge.
 *  - keeps the path the user took (undo trail and redo list, #398)
 *    beside the head, in one reducer (`lib/headTrail.ts`), so undo and
 *    redo follow the steps taken rather than the node's first parent.
 *  - saves that path with the project after every move (#453) and reads
 *    it back when the project opens, so it survives a restart. The save
 *    is not a turn in the line below and nothing waits for it
 *    (`lib/trailWriter.ts`).
 *  - forgets the steps a compaction deleted (#595): when the agent's
 *    `compact_session` ends, and whenever undo or redo is refused, it asks
 *    the store which nodes it still has (`forgetRemoved`), so undo is not
 *    pointed at one that is gone.
 *  - holds the one line every session-changing command goes through
 *    (`lib/sessionQueue.ts`, #498), so edits reach the backend, and their
 *    heads are adopted, in the order the user made them.
 *  - counts the moments the history graph has to be read again
 *    (`graphVersion`, #602): the one trigger the open graph follows, so it
 *    is not left to each caller to remember.
 *
 * It does not open projects. `App` does, as a turn in the line (it also
 * restores the view in the same turn), and tells the hook through
 * `resetHead`. A hook method that opened one in the line would deadlock
 * when called from inside a turn, and nothing needed it (#498).
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
  readSavedTrail,
  redoTarget,
  undoTarget,
  type HeadAction,
  type SavedTrail,
} from "../lib/headTrail";

import { createSessionQueue, type InSessionOrder } from "../lib/sessionQueue";
import { createTrailWriter } from "../lib/trailWriter";

import {
  getGraph,
  getHeadTrail,
  getSessionHead,
  onAgentDone,
  onNodeCreated,
  onToolCall,
  onToolCallEnd,
  renderPreview as bridgeRenderPreview,
  saveHeadTrail,
  type NodeId,
} from "../lib/tauri-bridge";

/**
 * The agent's tool that deletes history (`crates/tools/src/tool/compact.rs`).
 * It makes no node, so no `node-created` follows it: its call ending is
 * the only word the window gets that nodes may be gone.
 */
const COMPACT_TOOL = "compact_session";

/**
 * The node ids in a `get_graph` answer, or `null` for an answer that is
 * not one (a mock that does not know the command): nothing can be said to
 * be gone from that.
 */
function graphNodeIds(raw: unknown): Set<NodeId> | null {
  const nodes = (raw as { nodes?: unknown } | null | undefined)?.nodes;
  if (!Array.isArray(nodes)) return null;
  const ids = new Set<NodeId>();
  for (const n of nodes) {
    const id = (n as { id?: unknown } | null)?.id;
    if (typeof id === "string") ids.add(id);
  }
  return ids;
}

export interface UseSessionResult {
  /** Latest known head node id: read once at mount (a returning user's
   * restored head), then kept current off the event stream. */
  head: NodeId | null;
  /** Render the latest head node to a temp WAV. Throws if no head. */
  renderHead: () => Promise<string>;
  /**
   * The head moved on: an edit, an agent step, a click in the graph, an
   * A/B accept. The head it leaves goes on the undo trail and the redo
   * list is dropped, because this starts a new branch. Moving to the
   * head it already has changes nothing.
   *
   * This only changes the frontend's idea of the head; commands that
   * made the move on the backend have already done so. A click on a node
   * in the graph is not one: it is a preview, kept apart from the head
   * (`useGraphPreview`, #522), because the store did not move and the
   * saved path must end where the store does. Every head move in the
   * frontend must come through here (or `resetHead`): a head moved any
   * other way is one undo does not know about (`lib/headTrail.ts`).
   */
  setHeadLocal: (nodeId: NodeId) => void;
  /**
   * A different project is open: its head replaces this one's and none of
   * the old path applies. `null` is a project with nothing in it yet.
   *
   * `saved` is the path that project was saved with (`getHeadTrail`). It
   * is used if it ends at `nodeId`, and ignored if it ends anywhere else:
   * then it is not the path to this head, and undo falls back to the
   * node's parent (#453).
   */
  resetHead: (nodeId: NodeId | null, saved?: SavedTrail | null) => void;
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
  /**
   * Drop from the path every entry naming a node the store no longer has,
   * and save what is left (#595). A compaction deletes history, and the
   * path can name what it deleted: the redo list, and a step that returned
   * to an earlier state (mute, then unmute, leaves the mute off the head's
   * chain). Undo pointed at one would be refused on every press.
   *
   * Asks the store for its nodes (`get_graph`) and resolves to the ids it
   * dropped: none when the path names no missing node, or when the store
   * cannot answer. It never rejects. It is not a turn in the session's line
   * and queues nothing, so undo and redo call it from inside theirs, after
   * a refusal; the hook runs it in the line itself when `compact_session`
   * ends.
   */
  forgetRemoved: () => Promise<NodeId[]>;
  /**
   * A number that goes up whenever what the history graph draws may have
   * changed, and only then (#602): hand it to `GraphView` as `refreshKey`
   * and the open graph is read again, once. It goes up when
   *
   * - the head moves: an edit, undo, redo, "Set as head", an agent step,
   *   and a project opening, which counts even if its head is the one the
   *   last had;
   * - the agent's turn ends, because `name_node` sets a label and moves
   *   nothing (#601), so no node is created for anything to hear of;
   * - `forgetRemoved` runs, because it runs when nodes may be gone. That
   *   is when the agent's `compact_session` ends (it makes no node
   *   either), and when the store refuses an undo or redo.
   *
   * A click on a node in the graph is a preview and moves none of these
   * (#522), so it does not read the graph again.
   */
  graphVersion: number;
  /**
   * Run a command that changes the session or moves the head once every
   * one queued before it has finished (#498). The command's whole turn
   * goes in: the call, and adopting the head it returns. An edit sent
   * without this can overtake the one before it in the backend, or have
   * its answer adopted out of order, which is a head that goes backwards.
   * A turn that fails does not stop the ones behind it.
   */
  inOrder: InSessionOrder;
}

export function useSession(): UseSessionResult {
  // The head, the undo trail and the redo list, in one reducer. Held in a
  // ref as well as in state: undo and redo read it through `peekUndo` /
  // `peekRedo`, which must see a move made a moment ago even if React has
  // not rendered it yet (a key pressed right after an edit).
  const historyRef = useRef(initialHeadTrail);
  const [history, setHistory] = useState(initialHeadTrail);
  const { head, trail } = history;
  // One per mounted session, made once: the line is state, so it must not
  // be rebuilt by a render.
  const [queue] = useState(createSessionQueue);
  // The one trigger for reading the history graph again. A render with
  // more than one cause for it (a head move and a turn ending together)
  // reads the graph once, because `GraphView` sees only the number.
  const [graphVersion, setGraphVersion] = useState(0);
  const graphChanged = useCallback(() => setGraphVersion((n) => n + 1), []);
  // Saves the path with the project, outside the line above: no edit waits
  // for the disk. `send` reads the bridge when it runs, not now.
  const [writer] = useState(() => createTrailWriter((path) => saveHeadTrail(path)));
  const dispatchHead = useCallback(
    (action: HeadAction) => {
      const before = historyRef.current;
      const next = headTrailReducer(before, action);
      // Every head move comes through here, so this is the one place the
      // graph hears of them (#602). A project opening is one even when it
      // is on the same head: the nodes behind it are another project's.
      if (next.head !== before.head || action.type === "reset") graphChanged();
      if (next === before) return;
      historyRef.current = next;
      setHistory(next);
      // The path is saved when the user's steps change it. Not when it is
      // only being read (`fill`, `restore`): that is what is already
      // saved. And a different project drops what was waiting to be saved
      // for the old one.
      switch (action.type) {
        case "advance":
        case "stepBack":
        case "stepForward":
        case "drop":
          if (next.head !== null) writer.save(next);
          break;
        case "reset":
          writer.forget();
          break;
        default:
          break;
      }
    },
    [writer, graphChanged],
  );

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
  //
  // And the path saved with the project, read with it (#453). It applies
  // only to the head it ends at, so it is used once that head is known,
  // and only if no edit has made a path of its own meanwhile. A project
  // with no saved path, a corrupt one and a backend that cannot answer
  // all read as no path: undo then falls back to the node's parent.
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
    const saved = Promise.resolve()
      .then(() => getHeadTrail())
      .then(readSavedTrail, () => null);
    getSessionHead()
      .then(async (restored) => {
        if (cancelled) return;
        dispatchHead({ type: "fill", head: restored });
        const path = await saved;
        if (!cancelled && path) dispatchHead({ type: "restore", saved: path });
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      unlisten?.();
    };
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
    (nodeId: NodeId | null, saved?: SavedTrail | null) =>
      dispatchHead({ type: "reset", head: nodeId, saved }),
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
  const forget = useCallback(async (): Promise<NodeId[]> => {
    // What the path names now. A node an edit adds while the store is being
    // read is not among them, so it cannot be taken for one that is gone.
    const { trail: path, redo } = historyRef.current;
    const named = new Set([...path, ...redo]);
    if (named.size === 0) return [];
    let held: Set<NodeId> | null;
    try {
      held = graphNodeIds(await getGraph());
    } catch {
      return [];
    }
    // A list without the head is not the store this path is in (another
    // project opened meanwhile), and says nothing about it.
    const head = historyRef.current.head;
    if (held === null || head === null || !held.has(head)) return [];
    const gone = [...named].filter((id) => !held.has(id));
    if (gone.length > 0) dispatchHead({ type: "drop", nodes: gone });
    return gone;
  }, [dispatchHead]);
  const forgetRemoved = useCallback(async (): Promise<NodeId[]> => {
    try {
      return await forget();
    } finally {
      // It runs when nodes may be gone, whatever the path named and
      // however the store answered, and the graph is drawn from the store
      // and not from the path (#602). After the read, not before: the
      // graph is then read from a store that is settled.
      graphChanged();
    }
  }, [forget, graphChanged]);

  // When the agent's compaction ends, forget what it deleted, in the line:
  // an undo pressed meanwhile waits for it, so it is never sent to a node
  // that is gone. A dry run deletes nothing, and the store then answers
  // with every node the path names. `forgetRemoved` also says the graph is
  // to be read again, so a compaction needs nothing more for the view.
  useEffect(() => {
    let cancelled = false;
    const unlisten: (() => void)[] = [];
    const keep = (listening: Promise<() => void>) =>
      listening
        .then((fn) => {
          if (cancelled) fn();
          else unlisten.push(fn);
        })
        .catch(() => undefined);
    const compactions = new Set<string>();
    keep(
      onToolCall((name, id) => {
        if (name === COMPACT_TOOL) compactions.add(id);
      }),
    );
    keep(
      onToolCallEnd((id, ok) => {
        if (!compactions.delete(id) || !ok) return;
        void queue.run(forgetRemoved);
      }),
    );
    // A turn's end is the word on a label the agent set: `name_node` moves
    // no head and makes no node (#601), so nothing else announces it (#602).
    keep(onAgentDone(graphChanged));
    return () => {
      cancelled = true;
      for (const fn of unlisten) fn();
    };
  }, [queue, forgetRemoved, graphChanged]);

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
    head,
    renderHead,
    setHeadLocal,
    resetHead,
    trail,
    peekUndo,
    peekRedo,
    stepBack,
    stepForward,
    forgetRemoved,
    graphVersion,
    inOrder: queue.run,
  };
}
