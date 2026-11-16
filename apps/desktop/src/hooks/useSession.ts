/**
 * useSession: the one way the window changes the session, and the reads it
 * shows (ADR 0001, #713).
 *
 * Two operations change the session:
 *
 *  - `edit(name, args, options)`: a command that returns a new head (a
 *    track's gain, a marker, a template, a cut, a load). The head it
 *    returns is adopted inside the same turn. The user's held lane controls
 *    keep their claims until the track read after the edit has landed.
 *  - `move(request)`: undo, redo, Set as head, Accept B, opening a project,
 *    Save As, a punch-in's reconcile; and the assistant's own
 *    `agent://node-created`, which the module hears itself. The session
 *    moved by itself, so held controls are let go of, every read is made
 *    again, and undo and redo skip steps a compaction removed (#595).
 *
 * Both are mutations in one TanStack Query scope, `'session'`, so they run
 * one at a time in the order they were made (#498). An edit sent without
 * this could overtake the one before it in the backend, or have its answer
 * adopted out of order, which is a head that goes backwards. A turn that
 * fails does not stop the ones behind it. The commands live in
 * `lib/sessionCommands.ts`, which only this module imports.
 *
 * What an edit or a move changes is a table, not a handler's memory: each
 * entry of `lib/sessionCommands.ts` names its `reads`, a turn first cancels
 * the reads in flight that it can change (a read requested before a write
 * must not land after it), and invalidates them when it settles, from that
 * table. A turn is over when its command has been answered and its head
 * adopted: the reads follow without holding up the next edit.
 *
 * A held lane control is a mutation's `variables` (`lib/heldControls.ts`):
 * the edit's own while it is in the line, then a `confirm` mutation's,
 * pending until the first track read asked for after the answer has landed,
 * which is when the value goes back to the session (#596). The slider's
 * input value before its commit is a draft kept here.
 *
 * The head, the path taken and the undo rule stay in the window (#568); they
 * are `useSessionHead`, which only this hook imports. A click on a node in
 * the graph is a preview and moves none of it (#522).
 */

import {
  useMutation,
  useMutationState,
  useQuery,
  useQueryClient,
  type QueryClient,
} from "@tanstack/react-query";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type { SavedTrail } from "../lib/headTrail";
import {
  overlay,
  lastHeld,
  holdKey,
  trackKey,
  type ControlName,
  type ControlValue,
  type Hold,
} from "../lib/heldControls";
import {
  edits,
  moves,
  type Edit,
  type EditAnswer,
  type EditArgs,
  type EditName,
} from "../lib/sessionCommands";
import {
  EMPTY_TRACKS,
  HEAD_READS,
  PROJECT_READS,
  READS,
  readTracks,
  type ReadName,
} from "../lib/sessionReads";
import {
  getNode,
  onAgentDone,
  onMarkerChanged,
  onNodeCreated,
  onToolCall,
  onToolCallEnd,
  type CopyReport,
  type NodeId,
  type ProjectInfo,
  type TrackSummary,
} from "../lib/tauri-bridge";
import { useSessionHead } from "./useSessionHead";

export type { EditArgs, EditName };

/**
 * The agent's tool that deletes history (`crates/tools/src/tool/compact.rs`).
 * It makes no node, so no `node-created` follows it: its call ending is
 * the only word the window gets that nodes may be gone.
 */
const COMPACT_TOOL = "compact_session";

const WRITE_KEY = ["session", "write"] as const;
/** The wait for the read that confirms an edit's held control. */
const CONFIRM_KEY = ["session", "confirm"] as const;
/** Matches both: every mutation that can hold a control. */
const HOLDING_KEY = ["session"] as const;

/** The session moved by itself; see `move`. */
export type MoveRequest =
  | { kind: "undo" }
  | { kind: "redo" }
  /** "Set as head" in the graph. */
  | { kind: "setHead"; node: NodeId }
  /** A/B compare's Accept B. */
  | { kind: "acceptB"; b: NodeId }
  /**
   * Open a project. `restore` runs in the turn, after the project is open
   * and before the head is reset: it puts the view back and answers the
   * path the project was saved with (`getHeadTrail`).
   */
  | {
      kind: "open";
      path: string;
      restore: (info: ProjectInfo) => Promise<SavedTrail | null>;
    }
  /** Copy the project and carry on in the copy; `adopted` runs in the turn. */
  | { kind: "saveAs"; dest: string; adopted?: () => void }
  /**
   * A command that was applied outside the line (a punch-in) has landed:
   * read the head it left. Held controls are let go of only while
   * `isCurrent()` still holds (the project is the one it was asked in).
   */
  | { kind: "reconcile"; isCurrent: () => boolean };

export type MoveAnswer<M extends MoveRequest> = M extends { kind: "undo" | "redo" }
  ? boolean
  : M extends { kind: "setHead" }
    ? void
    : M extends { kind: "acceptB" }
      ? NodeId
      : M extends { kind: "open" }
        ? ProjectInfo
        : M extends { kind: "saveAs" }
          ? CopyReport
          : NodeId | null;

/** What a lane control the user sets is held as: the track's position and the control. */
export interface HeldInput {
  index: number;
  control: ControlName;
  value: ControlValue;
}

export interface EditOptions {
  /**
   * The lane control this edit commits. It is the user's until the track
   * read after the edit has landed.
   */
  hold?: HeldInput;
}

/**
 * A turn in the session's scope: what `mutate` is given, and what held
 * controls read. `seq` orders holds across turns and confirmations: for one
 * control the newest wins.
 */
type Turn =
  | { type: "edit"; name: EditName; args: unknown[]; hold?: Hold; epoch: number; seq: number }
  | { type: "move"; request: MoveRequest; epoch: number; seq: number }
  | { type: "forget"; epoch: number; seq: number };

/**
 * Waiting, outside the line, for the first track read asked for after an
 * edit was answered. The edit's control is the user's until it has landed.
 */
interface Confirm {
  type: "confirm";
  hold: Hold;
  epoch: number;
  seq: number;
  /** `dataUpdateCount` of the track query when the read was asked for. */
  after: number;
}

/** What a turn answers, and whether it moved the session. */
interface Outcome {
  answer: unknown;
  /** Let go of every held control (the session moved by itself). */
  release: boolean;
}

export interface UseSessionOptions {
  /**
   * A track read that failed after a turn. The turn itself is not failed by
   * it: the command was answered, and the read is a separate matter.
   */
  onReadError?: (error: unknown) => void;
}

export interface UseSessionResult {
  /**
   * The track list as the lanes draw it: the session's own, with the
   * controls the user holds laid over it (#596).
   */
  tracks: TrackSummary[];
  /**
   * The session's own track list, with nothing held over it: what the
   * session holds as of the last read, which a commit compares against.
   */
  sessionTracks: TrackSummary[];
  /**
   * A number that goes up whenever the session changed under whatever the
   * window had rendered of it: an edit adopted a head, the assistant made a
   * node, a project opened, a punch landed. What a render was made at is
   * compared with it, so a mix rendered before one of these is not the
   * session's mix any more, with no handler left to clear it (ADR 0001).
   * Undo, redo, Set as head and Accept B do not count: the mix of the node
   * they leave is still that node's, and `mixIsStale` says it is not the
   * one shown.
   */
  edition: number;
  /** Latest known head node id: read once at mount, then kept current. */
  head: NodeId | null;
  /** Render the latest head node to a temp WAV. Throws if no head. */
  renderHead: () => Promise<string>;
  /**
   * The heads the user has been on this session, oldest first, not
   * including `head`: the path drawn over the history graph.
   */
  trail: readonly NodeId[];
  /**
   * Send a command that changes the session and adopt the head it returns,
   * as one turn in the session's line. Resolves with the command's answer
   * once the turn is over; rejects when the command does, and the edits
   * behind it still run.
   */
  edit: <K extends EditName>(
    name: K,
    args: EditArgs<K>,
    options?: EditOptions,
  ) => Promise<EditAnswer<K>>;
  /**
   * Move the session: one turn in the line, read when it is its turn. A
   * move that happened releases the user's held controls and reads again
   * what it changed. Rejects when the command is refused.
   */
  move: <M extends MoveRequest>(request: M) => Promise<MoveAnswer<M>>;
  /** The user is moving a lane control: it is theirs until the session confirms it. */
  holdInput: (input: HeldInput) => void;
  /**
   * A release, key-up or blur whose value needed no commit: it is what the
   * session holds, or what a commit already on its way will leave it with.
   */
  dropInput: (index: number, control: ControlName) => void;
  /**
   * The last value an edit sent for a control that has not been confirmed:
   * what the session will hold once the commits in line are applied.
   */
  sentValue: (index: number, control: ControlName) => ControlValue | undefined;
}

export function useSession(options: UseSessionOptions = {}): UseSessionResult {
  const client = useQueryClient();
  const sessionHead = useSessionHead();
  const { head, renderHead, trail, setHeadLocal, resetHead, reconcileHead } = sessionHead;
  const { peekUndo, peekRedo, stepBack, stepForward, forgetRemoved: forget } = sessionHead;

  const onReadErrorRef = useRef(options.onReadError);
  useEffect(() => {
    onReadErrorRef.current = options.onReadError;
  }, [options.onReadError]);

  // ---- reads --------------------------------------------------------------

  // Fresh `TrackSummary` and `clips` references on every adopted read, even
  // for identical data. Consumers re-sync on the identity of the list and of
  // each track's `clips` (App's lane memo, Timeline's lane state, ClipStrip
  // and AutomationLane drafts): a refused edit followed by an identical read
  // must still hand back new references, or a dragged chip stays where it was
  // dropped while the session holds the old position.
  const { data: sessionTracks = EMPTY_TRACKS } = useQuery({
    queryKey: READS.tracks,
    queryFn: readTracks,
    structuralSharing: false,
  });
  const tracksRef = useRef<TrackSummary[]>(sessionTracks);
  useEffect(() => {
    tracksRef.current = sessionTracks;
  }, [sessionTracks]);

  /**
   * Ask again for the reads a turn can change: the table's answer, at the
   * turn's end. Not awaited by the turn, so the next one is not held up; the
   * track read's failure is reported, and `confirm` waits for its success.
   */
  const settle = useCallback(
    (reads: readonly ReadName[]): void => {
      for (const name of reads) {
        // A read in flight that has no data yet is shared by `invalidate`
        // and not asked for again, so it is cancelled first: whatever it
        // is about to answer was asked for before the change.
        void client.cancelQueries({ queryKey: READS[name] });
        if (name === "tracks") {
          const before = client.getQueryState(READS.tracks)?.errorUpdateCount ?? 0;
          void client.invalidateQueries({ queryKey: READS.tracks }).then(() => {
            const state = client.getQueryState(READS.tracks);
            if (state?.status === "error" && state.errorUpdateCount > before) {
              onReadErrorRef.current?.(state.error);
            }
          });
        } else {
          void client.invalidateQueries({ queryKey: READS[name] });
        }
      }
    },
    [client],
  );

  // ---- held controls ------------------------------------------------------

  // Bumped when the session moves by itself: a hold made before it is no
  // longer held, whatever turn it belongs to.
  const epochRef = useRef(0);
  const seqRef = useRef(0);
  const [epoch, setEpoch] = useState(0);
  const [edition, setEdition] = useState(0);
  const newEdition = useCallback(() => setEdition((n) => n + 1), []);
  // The input values of sliders being dragged, not yet committed. Read at
  // render, so a list that arrives meanwhile cannot overwrite them; setting
  // one draws nothing, because the slider already shows its own value.
  const draftsRef = useRef(new Map<string, Hold>());
  const [draftVersion, bumpDrafts] = useState(0);
  const holdOf = useCallback((input: HeldInput): Hold | null => {
    const track = trackKey(tracksRef.current, input.index);
    return track === null ? null : { track, control: input.control, value: input.value };
  }, []);

  const release = useCallback(() => {
    epochRef.current += 1;
    draftsRef.current.clear();
    setEpoch(epochRef.current);
    bumpDrafts((n) => n + 1);
  }, []);

  const pending = useMutationState({
    filters: { mutationKey: HOLDING_KEY, status: "pending" },
    select: (mutation) => mutation.state.variables as Turn | Confirm | undefined,
  });

  /** The holds of the edits not yet confirmed, oldest first. */
  const sentHolds = useMemo(() => sentHoldsOf(pending, epoch), [pending, epoch]);
  const tracks = useMemo(
    () => {
      const drawn = overlay(sessionTracks, [...sentHolds, ...draftsRef.current.values()]);
      // A control handed back is a new list, even when it draws what the
      // session's own list does: the timeline copies what it is handed into
      // its lanes, and must go back to the session's value.
      return drawn === sessionTracks ? [...sessionTracks] : drawn;
    },
    // `draftVersion` says a draft was let go of; the drafts are read from the ref.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [sessionTracks, sentHolds, draftVersion],
  );

  const holdInput = useCallback(
    (input: HeldInput) => {
      const hold = holdOf(input);
      if (hold) draftsRef.current.set(holdKey(hold), hold);
    },
    [holdOf],
  );
  const dropInput = useCallback((index: number, control: ControlName) => {
    const track = trackKey(tracksRef.current, index);
    if (track === null) return;
    if (draftsRef.current.delete(holdKey({ track, control }))) {
      bumpDrafts((n) => n + 1);
    }
  }, []);
  const sentValue = useCallback(
    (index: number, control: ControlName): ControlValue | undefined => {
      const track = trackKey(tracksRef.current, index);
      if (track === null) return undefined;
      const holds = sentHoldsOf(
        client
          .getMutationCache()
          .findAll({ mutationKey: HOLDING_KEY, status: "pending" })
          .map((m) => m.state.variables as Turn | Confirm | undefined),
        epochRef.current,
      );
      return lastHeld(holds, track, control);
    },
    [client],
  );

  // ---- the turns ----------------------------------------------------------

  // What the history graph draws may have changed whenever the store may
  // have lost nodes (#602).
  const forgetAndShow = useCallback(async (): Promise<NodeId[]> => {
    try {
      return await forget();
    } finally {
      // After the read, not before: the graph is then read from a store that
      // is settled.
      void client.invalidateQueries({ queryKey: READS.graph });
    }
  }, [forget, client]);

  const moveSession = useCallback(
    async (request: MoveRequest): Promise<Outcome> => {
      switch (request.kind) {
        case "undo":
          // A step to a node the store no longer has is dropped and the
          // next one tried (#595). A pass that goes round again has taken
          // the step it tried off the path, so this ends.
          for (;;) {
            // Where the user came from, read from the latest state and not
            // from this render: a key pressed right after an edit must not
            // undo to where the render before it was (#398). It is read
            // when it is the turn's turn (#498): the head to leave is the
            // one the edit before it made.
            const step = peekUndo();
            if (!step) return { answer: false, release: false };
            // The node's stored parent is right only while no edit has
            // returned to an earlier state, so it is asked for only when
            // the trail has nothing left.
            const to = step.to ?? (await getNode(step.from)).parent;
            if (!to) return { answer: false, release: false };
            try {
              await moves.setHeadTo(to);
            } catch (err) {
              // Refused. If the node is gone, the path has now forgotten it:
              // try the next step. Whether it is gone is asked of the
              // store's nodes, not read from the error's text. A stored
              // parent that is gone is not on the path, so forgetting cannot
              // take it away, and it is an error like any other.
              if (step.to !== null && (await forgetAndShow()).includes(to)) continue;
              throw err;
            }
            stepBack(step.from, to);
            return { answer: true, release: true };
          }
        case "redo":
          // A removed step is skipped, as undo skips one (#595).
          for (;;) {
            const step = peekRedo();
            if (!step) return { answer: false, release: false };
            try {
              await moves.setHeadTo(step.to);
            } catch (err) {
              if ((await forgetAndShow()).includes(step.to)) continue;
              throw err;
            }
            stepForward(step.from, step.to);
            return { answer: true, release: true };
          }
        case "setHead":
          await moves.setHeadTo(request.node);
          setHeadLocal(request.node);
          return { answer: undefined, release: true };
        case "acceptB": {
          const accepted = await moves.acceptB(request.b);
          setHeadLocal(accepted);
          return { answer: accepted, release: true };
        }
        case "open": {
          const info = await moves.openProject(request.path);
          const saved = await request.restore(info);
          // Always, including `null`: the project just opened has its own
          // head and its own history, and neither the old project's head
          // nor the path taken through it may carry over (#398).
          resetHead(info.head ?? null, saved);
          newEdition();
          return { answer: info, release: true };
        }
        case "saveAs": {
          const report = await moves.saveProjectAs(request.dest);
          request.adopted?.();
          return { answer: report, release: true };
        }
        case "reconcile": {
          // The response may arrive after a later edit or project switch.
          // The head is read inside the line; a head read that fails or
          // names a newer one is not adopted over it.
          const current = await reconcileHead();
          if (current !== null) newEdition();
          return { answer: current, release: current !== null && request.isCurrent() };
        }
      }
    },
    [
      peekUndo,
      peekRedo,
      stepBack,
      stepForward,
      setHeadLocal,
      resetHead,
      reconcileHead,
      forgetAndShow,
      newEdition,
    ],
  );

  const runTurn = useCallback(
    async (turn: Turn): Promise<Outcome> => {
      // A read requested before a write must not land after it. What a turn
      // can change is asked for again when it settles (`settle`), so a read
      // that is overtaken here is not needed.
      for (const name of readsOf(turn)) {
        await client.cancelQueries({ queryKey: READS[name] });
      }
      switch (turn.type) {
        case "edit": {
          const spec = edits[turn.name] as unknown as Edit<unknown[], unknown>;
          const answer = await spec.run(...turn.args);
          const adopted = spec.head(answer);
          if (adopted) {
            setHeadLocal(adopted);
            newEdition();
          }
          return { answer, release: false };
        }
        case "move":
          return moveSession(turn.request);
        case "forget":
          await forgetAndShow();
          return { answer: undefined, release: false };
      }
    },
    [client, moveSession, setHeadLocal, forgetAndShow, newEdition],
  );

  // What `useMutation` runs is fixed when a turn is made; these are read when
  // it runs, so a turn made a moment ago still sees the latest handlers.
  const turnRef = useRef({ runTurn, release, settle });
  useEffect(() => {
    turnRef.current = { runTurn, release, settle };
  }, [runTurn, release, settle]);

  const { mutateAsync: confirm } = useMutation<void, unknown, Confirm>({
    mutationKey: CONFIRM_KEY,
    mutationFn: ({ after }) => nextTrackRead(client, after),
  });

  const { mutate, mutateAsync } = useMutation<Outcome, unknown, Turn>({
    mutationKey: WRITE_KEY,
    scope: { id: "session" },
    mutationFn: (turn) => turnRef.current.runTurn(turn),
    // Settled, not succeeded: a turn the session refused is read back too,
    // so the control goes back to the session's value (#596), and a read
    // after an error is how the window learns where the session is. The
    // turn does not wait for it: the next edit goes out at once, and the
    // read confirms the control when it lands.
    onSettled: (outcome, _error, turn) => {
      const { release: letGo, settle: ask } = turnRef.current;
      if (outcome?.release) letGo();
      const after = client.getQueryState(READS.tracks)?.dataUpdateCount ?? 0;
      ask(readsOf(turn));
      if (turn.type === "edit" && turn.hold) {
        const { hold, epoch: held, seq } = turn;
        confirm({ type: "confirm", hold, epoch: held, seq, after }).catch(() => undefined);
      }
    },
  });

  const edit = useCallback(
    <K extends EditName>(
      name: K,
      args: EditArgs<K>,
      editOptions?: EditOptions,
    ): Promise<EditAnswer<K>> => {
      const hold = editOptions?.hold ? holdOf(editOptions.hold) : null;
      if (hold) draftsRef.current.delete(holdKey(hold));
      return mutateAsync({
        type: "edit",
        name,
        args,
        hold: hold ?? undefined,
        epoch: epochRef.current,
        seq: ++seqRef.current,
      }).then((outcome) => outcome.answer as EditAnswer<K>);
    },
    [mutateAsync, holdOf],
  );
  const move = useCallback(
    <M extends MoveRequest>(request: M): Promise<MoveAnswer<M>> =>
      mutateAsync({
        type: "move",
        request,
        epoch: epochRef.current,
        seq: ++seqRef.current,
      }).then((outcome) => outcome.answer as MoveAnswer<M>),
    [mutateAsync],
  );

  // ---- what the session does by itself --------------------------------------

  // A node the assistant created moved the session by itself, as an undo
  // does: what the user had set on a lane belongs to where it was (#596), and
  // every read is made again. Not a turn in the line: the assistant's work
  // is one command that lasts as long as the model does, reported through
  // events, and a line that waited for it would hold up every edit. Edits
  // during its turn stay allowed (the backend's store lock orders them). The
  // head follows the same event in `useSessionHead`.
  //
  // And when its compaction ends, forget what it deleted, in the line: an
  // undo pressed meanwhile waits for it, so it is never sent to a node that
  // is gone. A dry run deletes nothing, and the store then answers with every
  // node the path names. A turn's end is the word on a label the agent set:
  // `name_node` moves no head and makes no node (#601), so nothing else
  // announces it (#602).
  useEffect(() => {
    let cancelled = false;
    const unlisten: (() => void)[] = [];
    const keep = (listening: Promise<() => void>): void => {
      listening
        .then((fn) => {
          if (cancelled) fn();
          else unlisten.push(fn);
        })
        .catch(() => undefined);
    };
    const compactions = new Set<string>();
    keep(
      onNodeCreated(() => {
        turnRef.current.release();
        newEdition();
        turnRef.current.settle(HEAD_READS);
      }),
    );
    keep(
      onToolCall((name, id) => {
        if (name === COMPACT_TOOL) compactions.add(id);
      }),
    );
    keep(
      onToolCallEnd((id, ok) => {
        if (!compactions.delete(id) || !ok) return;
        // `forgetRemoved` never rejects, and `mutate` reports nothing of it.
        mutate({ type: "forget", epoch: epochRef.current, seq: ++seqRef.current });
      }),
    );
    keep(onAgentDone(() => void client.invalidateQueries({ queryKey: READS.graph })));
    keep(onMarkerChanged(() => void client.invalidateQueries({ queryKey: READS.markers })));
    return () => {
      cancelled = true;
      for (const fn of unlisten) fn();
    };
  }, [client, mutate, newEdition]);

  return {
    tracks,
    sessionTracks,
    edition,
    head,
    renderHead,
    trail,
    edit,
    move,
    holdInput,
    dropInput,
    sentValue,
  };
}

/** The reads a turn can change: the table's answer for it. */
function readsOf(turn: Turn): readonly ReadName[] {
  switch (turn.type) {
    case "edit":
      return edits[turn.name].reads;
    case "forget":
      return [];
    case "move":
      // A different project's reads are none of the last one's.
      return turn.request.kind === "open" || turn.request.kind === "saveAs"
        ? PROJECT_READS
        : HEAD_READS;
  }
}

/**
 * The holds of the turns and confirmations still pending, oldest first, that
 * were made since the session last moved by itself (`epoch`).
 */
function sentHoldsOf(pending: readonly (Turn | Confirm | undefined)[], epoch: number): Hold[] {
  return pending
    .flatMap((turn) =>
      (turn?.type === "edit" || turn?.type === "confirm") && turn.hold && turn.epoch === epoch
        ? [turn]
        : [],
    )
    .sort((a, b) => a.seq - b.seq)
    .map((turn) => turn.hold as Hold);
}

/**
 * Resolves when a track read asked for after `after` successes ago has
 * landed in the cache. A read that failed, or was overtaken before it
 * landed, confirms nothing: the control stays the user's until one does.
 */
function nextTrackRead(client: QueryClient, after: number): Promise<void> {
  const landed = () => {
    const state = client.getQueryState(READS.tracks);
    return state?.status === "success" && state.dataUpdateCount > after;
  };
  if (landed()) return Promise.resolve();
  return new Promise((resolve) => {
    const unsubscribe = client.getQueryCache().subscribe(() => {
      if (!landed()) return;
      unsubscribe();
      resolve();
    });
  });
}
