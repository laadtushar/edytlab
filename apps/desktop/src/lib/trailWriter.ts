/**
 * Saving the path the user took, without ever making anything wait for it
 * (#453).
 *
 * Every move of the head changes the path, and the path is kept with the
 * project (`save_head_trail`). The save writes a file and syncs it, and
 * the move that caused it is a turn in the session's line of edits
 * (`lib/sessionQueue.ts`): if the turn waited for the save, every edit
 * behind it would wait for the disk too. So the save is not part of any
 * turn. It is handed here and the caller goes on.
 *
 * Two rules keep the file right even so:
 *
 * - **One at a time.** Saves sent together could reach the backend and be
 *   written in either order, leaving the older path on disk after the
 *   newer. Nothing is sent while a save is in flight.
 * - **The newest wins.** What arrives meanwhile replaces whatever was
 *   waiting, because a save replaces the whole file: the ones in between
 *   would be written only to be written over. However fast the user
 *   edits, at most one save is in flight and one waits.
 *
 * A save that fails is dropped (the next move saves again, and a path that
 * is behind the head is not used when the project is opened: see
 * `lib/headTrail.ts`). A path is a convenience, so a failure is not shown.
 *
 * `forget` is for a different project opening: a save still waiting is
 * the old project's path, and is dropped. One already in flight cannot be
 * taken back, and the backend refuses a path whose head is not a node of
 * the project that is open.
 */

import type { SavedTrail } from "./headTrail";

export interface TrailWriter {
  /** Save `trail` as soon as nothing else is being saved. Returns at once. */
  save: (trail: SavedTrail) => void;
  /** Drop a save that has not been sent yet. */
  forget: () => void;
}

const nothing = () => undefined;

export function createTrailWriter(send: (trail: SavedTrail) => Promise<unknown>): TrailWriter {
  let sending = false;
  let waiting: SavedTrail | null = null;

  const pump = (): void => {
    if (sending || waiting === null) return;
    const next = waiting;
    waiting = null;
    sending = true;
    // Through a promise, so a `send` that throws instead of rejecting is
    // a failed save too, and cannot take the caller's turn down with it.
    Promise.resolve()
      .then(() => send(next))
      .then(nothing, nothing)
      .then(() => {
        sending = false;
        pump();
      });
  };

  return {
    save: (trail) => {
      waiting = trail;
      pump();
    },
    forget: () => {
      waiting = null;
    },
  };
}
