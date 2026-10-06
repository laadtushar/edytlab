/**
 * One head move at a time, and a word when one is slow (#373).
 *
 * Undo, redo and the history view move the head with `set_head_to`.
 * Ordinarily that answers at once. When the node it lands on had its
 * audio swept to keep the project under its cap (#98), the backend
 * first rebuilds that audio by replaying the edits that made it, which
 * on long history can take seconds. Until then the window said nothing:
 * Ctrl+Z read as the key doing nothing, and a second press queued a
 * second replay behind the first.
 *
 * `run` takes the whole move — reading the node, moving, refreshing —
 * and ignores another while one is in flight, so a held or repeated key
 * cannot stack replays. `pending` turns on only once a move has taken
 * longer than `delayMs`, so an ordinary undo never flashes it.
 */

import { useCallback, useEffect, useRef, useState } from "react";

/** Long enough that an undo answered from disk never shows it. */
export const HEAD_MOVE_PENDING_DELAY_MS = 150;

export interface HeadMove {
  /** A move has been running longer than the delay. */
  pending: boolean;
  /**
   * Run `move` unless one is already running. Resolves with its result,
   * or `undefined` when it was ignored.
   */
  run: <T>(move: () => Promise<T>) => Promise<T | undefined>;
}

export function useHeadMove(delayMs: number = HEAD_MOVE_PENDING_DELAY_MS): HeadMove {
  const inFlight = useRef(false);
  const mounted = useRef(true);
  const [pending, setPending] = useState(false);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const run = useCallback(
    async <T,>(move: () => Promise<T>): Promise<T | undefined> => {
      if (inFlight.current) return undefined;
      inFlight.current = true;
      const timer = setTimeout(() => {
        if (mounted.current) setPending(true);
      }, delayMs);
      try {
        return await move();
      } finally {
        clearTimeout(timer);
        inFlight.current = false;
        if (mounted.current) setPending(false);
      }
    },
    [delayMs],
  );

  return { pending, run };
}
