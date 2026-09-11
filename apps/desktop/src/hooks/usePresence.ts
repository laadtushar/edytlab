/**
 * Keep a component mounted long enough to animate out (#236).
 *
 * #211 gave the overlays and the progress strip an *entry* animation and
 * stopped there. All four surfaces render `null` the instant their flag
 * flips, so closing one was still the single-frame blink the ticket set
 * out to remove, and the progress strip still yanked the timeline back
 * up when a batch ended — the exact relayout `strip-in` exists to
 * smooth. CSS alone cannot fix this: `animation-fill-mode: both` retains
 * the *final* frame of an entry animation and has nothing to say about
 * an element that is about to stop existing. Something has to hold the
 * mount open, and only JavaScript can.
 *
 * So: `open` goes false, `leaving` goes true for one `--dur-2`, the exit
 * keyframes play, and only then does `mounted` go false.
 *
 * ## Why both an `animationend` handler and a timer
 *
 * `animationend` is the exact signal — it unmounts on the frame the
 * animation actually finishes, whatever the duration resolved to. The
 * reduced-motion block sets `0.01ms` rather than `0` precisely so it
 * still fires, which is what lets the same code path serve both
 * settings.
 *
 * But an animation that never runs never ends, and there are ordinary
 * ways for that to happen: the element is `display: none`, the class
 * was renamed, a parent unmounted mid-flight, or — the one that bit
 * first — the test environment, where jsdom computes no animations at
 * all. Without the timer the overlay would simply stay on screen
 * forever, which is a far worse bug than the blink this replaces. The
 * timer is the floor and `animationend` is the fast path; whichever
 * arrives first wins.
 */

import { useCallback, useEffect, useRef, useState } from "react";

/**
 * How long a leave is held, in milliseconds.
 *
 * **Must equal `--dur-2` in `styles.css`** — the CSS plays the exit
 * animation and this decides when the element stops existing, so the
 * two drifting apart either truncates the animation or leaves a dead
 * element on screen after it. `motion.test.tsx` asserts they agree,
 * because nothing about a mismatch is a type error.
 */
export const LEAVE_MS = 200;

/**
 * Slack added to the timer so `animationend` gets a chance to be the
 * one that fires. Without it the timer and the animation race at the
 * same instant and the unmount can land a frame early, clipping the
 * last frame of the very animation this hook exists to show.
 */
const LEAVE_GRACE_MS = 40;

export interface Presence {
  /** Whether to render at all. Stays true through the exit animation. */
  mounted: boolean;
  /** True only while animating out — drives the `*-out` classes. */
  leaving: boolean;
  /**
   * Attach to the element carrying the exit animation. Ends the leave
   * as soon as the animation really finishes, rather than waiting out
   * the timer.
   */
  onAnimationEnd: (e: React.AnimationEvent) => void;
}

export function usePresence(open: boolean): Presence {
  const [mounted, setMounted] = useState(open);
  const [leaving, setLeaving] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const clear = () => {
    if (timer.current !== null) {
      clearTimeout(timer.current);
      timer.current = null;
    }
  };

  useEffect(() => {
    if (open) {
      // Re-opening during a leave has to cancel it, or the pending
      // timer unmounts the thing the user just reopened.
      clear();
      setMounted(true);
      setLeaving(false);
      return;
    }
    // Closing something that was never open is not a leave — it must
    // not mount anything, or "closed" becomes "present but
    // transparent", which still traps clicks and still takes focus.
    setMounted((wasMounted) => {
      if (!wasMounted) return false;
      setLeaving(true);
      clear();
      timer.current = setTimeout(() => {
        timer.current = null;
        setLeaving(false);
        setMounted(false);
      }, LEAVE_MS + LEAVE_GRACE_MS);
      return true;
    });
  }, [open]);

  // Unmounting mid-leave must not leave a timer holding a setState.
  useEffect(() => clear, []);

  const onAnimationEnd = useCallback((e: React.AnimationEvent) => {
    // Only the element's own exit animation ends the leave. Without
    // this, an animation bubbling up from a child — a spinner, a
    // progress bar — would unmount the overlay around it.
    if (e.target !== e.currentTarget) return;
    if (!e.animationName.endsWith("-out")) return;
    clear();
    setLeaving(false);
    setMounted(false);
  }, []);

  return { mounted, leaving, onAnimationEnd };
}
