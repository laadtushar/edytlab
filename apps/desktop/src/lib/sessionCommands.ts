/**
 * Every command that changes the session, and what each one changes
 * (ADR 0001, #713).
 *
 * Only `hooks/useSession.ts` may import this module
 * (`__tests__/sessionBoundary.test.ts`), and `tauri-bridge.ts` no longer
 * exports these commands. So a handler cannot send an edit that skips the
 * session's line, drop the head the edit returns, or forget the read it
 * leaves out of date: there is nothing to call but `useSession`'s `edit` and
 * `move`, and both take their rules from the entries below.
 *
 * An entry is a command and the answers it owes:
 *
 * - `head`: the head the command's answer names, or none. The type has no
 *   default, so a command added without saying what it returns does not
 *   compile (#232: a returned head nobody adopted was written to
 *   `view.json` and replayed over the nodes created since).
 * - `reads`: the reads (`lib/sessionReads.ts`) the command can change, which
 *   `useSession` invalidates when it settles.
 *
 * A new session-changing command is one entry here.
 */

import { invoke } from "@tauri-apps/api/core";

import { HEAD_READS, type ReadName } from "./sessionReads";
import type {
  BatchLoadResult,
  CopyReport,
  EnvelopePoint,
  NodeId,
  ProjectInfo,
} from "./tauri-bridge";

/** A command, what head its answer names, and which reads it can change. */
export interface Edit<A extends unknown[], R> {
  run: (...args: A) => Promise<R>;
  head: (answer: R) => NodeId | null | undefined;
  reads: readonly ReadName[];
}

// `R` sits on both sides of an entry (what `run` answers, what `head` reads),
// so no one type stands for them all but `any`.
// eslint-disable-next-line @typescript-eslint/no-explicit-any
type AnyEdit = Edit<never[], any>;

/** An edit whose answer is the new head, as most are. */
function headEdit<A extends unknown[]>(run: (...args: A) => Promise<string>): Edit<A, string> {
  return { run, head: (head) => head, reads: HEAD_READS };
}

export const edits = {
  // Mixer controls. Each appends one undoable node and answers the new head.
  // The backend validates ranges too: these are not the only guard.
  setTrackGain: headEdit((track: number, gainDb: number) =>
    invoke<string>("set_track_gain", { track, gainDb }),
  ),
  setTrackPan: headEdit((track: number, pan: number) =>
    invoke<string>("set_track_pan", { track, pan }),
  ),
  setTrackMuted: headEdit((track: number, muted: boolean) =>
    invoke<string>("set_track_muted", { track, muted }),
  ),
  setTrackSoloed: headEdit((track: number, soloed: boolean) =>
    invoke<string>("set_track_soloed", { track, soloed }),
  ),
  /** Resolves to the new head, or the unchanged one if nothing changed. */
  setSyncLock: headEdit((enabled: boolean) => invoke<string>("set_sync_lock", { enabled })),

  /**
   * Track-head actions (#161). Each appends an ordinary session node and
   * returns the new head, so they undo like anything else, which is why
   * `removeTrack` needs no confirmation.
   */
  renameTrack: headEdit((track: number, name: string) =>
    invoke<string>("rename_track", { track, name }),
  ),
  removeTrack: headEdit((track: number) => invoke<string>("remove_track", { track })),
  duplicateTrack: headEdit((track: number) => invoke<string>("duplicate_track", { track })),

  /**
   * Move one clip to a new start, in seconds from the top of the timeline.
   * The other clips stay put (`time_shift` is the whole-track version).
   * Clips are re-sorted by start afterwards, so a clip dragged past its
   * neighbour comes back at a different index.
   */
  moveClip: headEdit((track: number, clip: number, startSec: number) =>
    invoke<string>("move_clip", { track, clip, startSec }),
  ),
  /** Remove one clip, leaving a silent gap where it was. */
  removeClip: headEdit((track: number, clip: number) =>
    invoke<string>("remove_clip", { track, clip }),
  ),
  /**
   * Replace a clip's volume automation curve. An empty array clears it.
   * Points need not be sorted: the tool sorts, so dragging one past its
   * neighbour does not need the caller to reorder first.
   */
  setClipEnvelope: headEdit((track: number, clip: number, points: EnvelopePoint[]) =>
    invoke<string>("set_clip_envelope", { track, clip, points }),
  ),

  addMarker: headEdit((time: number, name: string) =>
    invoke<string>("add_marker", { time, name }),
  ),
  removeMarker: headEdit((id: string) => invoke<string>("remove_marker", { id })),
  /**
   * Rename and/or move a label (#203 §1). One call rather than a rename and
   * a move, so a drag-and-rename is one undo step. Omitted fields are left
   * as they are; a change that changes nothing appends no node and returns
   * the head unchanged.
   */
  updateMarker: headEdit(
    (id: string, patch: { name?: string; time?: number; start?: number; end?: number }) =>
      invoke<string>("update_marker", { id, ...patch }),
  ),

  /** Cut `[fromWord, toWord)` and the audio underneath. Answers the new head. */
  cutTranscriptWords: headEdit((track: number, fromWord: number, toWord: number) =>
    invoke<string>("cut_transcript_words", { track, fromWord, toWord }),
  ),
  applyTemplate: headEdit((name: string) => invoke<string>("apply_template", { name })),

  /**
   * Load files as tracks. A file the backend refused leaves no node, so with
   * nothing loaded the head has not moved and there is nothing to adopt.
   */
  batchLoad: {
    run: (paths: string[]) => invoke<BatchLoadResult>("batch_load", { paths }),
    head: (result) => (result.tracks_loaded > 0 ? result.last_node_id : null),
    reads: HEAD_READS,
  } satisfies Edit<[paths: string[]], BatchLoadResult>,

  /**
   * Set (or clear, when `label` is empty) a human-readable label on a node.
   * It moves no head and appends no node; only the history graph draws it.
   */
  renameNode: {
    run: (nodeId: string, label: string) => invoke<void>("rename_node", { nodeId, label }),
    head: () => null,
    reads: ["graph"],
  } satisfies Edit<[nodeId: string, label: string], void>,
  // Every entry has to say what head it returns and what it reads again: an
  // entry that does not is a type error here, which is what `headIsApplied`
  // used to scan `App.tsx` for.
} satisfies Record<string, AnyEdit>;

export type EditName = keyof typeof edits;
export type EditArgs<K extends EditName> = Parameters<(typeof edits)[K]["run"]>;
export type EditAnswer<K extends EditName> = Awaited<ReturnType<(typeof edits)[K]["run"]>>;

/**
 * The commands behind `move`: they carry the session somewhere it was not,
 * so held lane controls are let go of and every read is made again.
 */
export const moves = {
  /** Move the session head to `nodeId`. Answers the new head as hex. */
  setHeadTo: (nodeId: string) => invoke<string>("set_head_to", { nodeId }),
  /** A/B compare's Accept B: the head becomes `b`'s. */
  acceptB: (b: NodeId) => invoke<string>("accept_b", { b }),
  openProject: (path: string) => invoke<ProjectInfo>("open_project", { path }),
  /**
   * Copy the open project to `dest` and continue in the copy: Save As in the
   * sense a DAW means it, with the original left as it was. The copy is the
   * project at the moment of the click, whatever the agent commits while it
   * is made (#571), without the caches (#576).
   */
  saveProjectAs: (dest: string) => invoke<CopyReport>("save_project_as", { dest }),
};
