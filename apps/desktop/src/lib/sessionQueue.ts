/**
 * One line for everything that changes the session (#498).
 *
 * Each command the app sends is its own task in the backend, and the
 * ones that edit the session wait on the store lock. While a long tool
 * holds it they all wait, and the lock wakes them in no promised order
 * (on Windows a `std` mutex is not first in, first out). Two edits sent
 * at once could apply the wrong way round, so that the older fader value
 * landed last and the refresh showed it, and their answers could cross,
 * so that the head moved to a node older than the one it was on.
 *
 * So an edit is not sent until the one before it has been answered, and
 * the work that follows an answer (adopting the head it names) is part of
 * the same turn, so the next edit goes out after it, not before.
 *
 * What is queued: any command that changes the session or moves the head
 * (mixer, markers, transcript cuts, undo and redo, loads, opening a
 * project). What is not: reads, and the agent's turn, which is one
 * command that lasts as long as the model does and reports its edits
 * through events; a line that waited for it would hold up every edit.
 *
 * A turn that fails does not stop the line. The error reaches whoever
 * queued it, as it would have without a line, and the next turn still
 * runs.
 */

/** Run `turn` once everything queued before it has finished, however it finished. */
export type InSessionOrder = <T>(turn: () => Promise<T>) => Promise<T>;

export interface SessionQueue {
  /** See {@link InSessionOrder}. Resolves, or rejects, as `turn` does. */
  run: InSessionOrder;
}

const nothing = () => undefined;

export function createSessionQueue(): SessionQueue {
  // What the next turn waits for. It never rejects: a turn that failed
  // has told its own caller, and must not decide whether the next runs.
  let tail: Promise<void> = Promise.resolve();
  return {
    run: (turn) => {
      const result = tail.then(turn);
      tail = result.then(nothing, nothing);
      return result;
    },
  };
}
