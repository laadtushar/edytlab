"use client";

import { useRef, type ElementType, type ReactNode } from "react";

import {
  gsap,
  useGSAP,
  motionOk,
  NO_PREFERENCE,
  REVEAL_START,
  ScrollTrigger,
} from "@/lib/gsap";

/**
 * The pieces of a run of prose: each top-level block on its own, except
 * that a list — or a container marked `data-cascade-items`, such as a
 * grid of cards — arrives an item at a time, and a table a row at a
 * time. A forty-line list revealed as one block would have finished
 * long before most of it is in view.
 */
const PROSE_PIECES = [
  ":scope > :not(ul, ol, table, [data-cascade-items])",
  ":scope > :is(ul, ol, [data-cascade-items]) > *",
  ":scope > table > thead",
  ":scope > table > tbody > tr",
].join(", ");

interface CascadeProps {
  children: ReactNode;
  className?: string;
  /** What to reveal inside. Defaults to the direct children. */
  selector?: string;
  /**
   * Reveal as prose: blocks one by one, lists and card grids by item.
   * A flag rather than an exported selector, so a server component can
   * ask for it — a value exported from a client module reaches a server
   * component as a reference, not as the string.
   */
  prose?: boolean;
  /** How far each piece rises, in px. */
  distance?: number;
  /** Gap between pieces that arrive together, in seconds. */
  each?: number;
  /** Render as something other than a div — `ul`, `article`. */
  as?: ElementType;
}

/** `REVEAL_START` as a fraction of the viewport, for the on-mount check. */
const REVEAL_LINE = 0.85;

/**
 * Long content that arrives a piece at a time, as each piece reaches
 * the screen.
 *
 * `Stagger` fires once for the whole list, when the top of the list
 * arrives. That is right for a grid that fits on one screen and wrong
 * for a changelog: by the time the reader gets to the fortieth bullet
 * it animated long ago, off-screen, for nobody. `ScrollTrigger.batch`
 * gives every piece its own trigger and groups the ones that cross the
 * line in the same frame, so a fast scroll reveals a screenful together
 * and a slow one reveals line by line.
 *
 * Same contract as `Reveal`: the markup is the finished state, and the
 * hidden state is applied here, after mount, only when motion is
 * welcome. Anything already above the reveal line at mount is left
 * alone — hiding it would blank text the reader may already be reading,
 * and a page restored mid-scroll would otherwise wait for a trigger that
 * has already gone past.
 */
export function Cascade({
  children,
  className,
  selector: ownSelector,
  prose = false,
  distance = 14,
  each = 0.06,
  as: Tag = "div",
}: CascadeProps) {
  const ref = useRef<HTMLElement>(null);
  const selector = ownSelector ?? (prose ? PROSE_PIECES : ":scope > *");

  useGSAP(
    () => {
      const root = ref.current;
      if (!root) return;

      const mm = motionOk();
      mm.add(NO_PREFERENCE, () => {
        const line = window.innerHeight * REVEAL_LINE;
        const pending = gsap.utils
          .toArray<HTMLElement>(root.querySelectorAll(selector))
          .filter((el) => el.getBoundingClientRect().top > line);
        if (!pending.length) return;

        gsap.set(pending, { opacity: 0, y: distance });
        ScrollTrigger.batch(pending, {
          start: REVEAL_START,
          once: true,
          onEnter: (batch) =>
            gsap.to(batch, {
              opacity: 1,
              y: 0,
              duration: 0.55,
              stagger: each,
              overwrite: true,
              // Leave nothing inline once it has landed: a lingering
              // `transform` makes every block its own stacking context.
              clearProps: "opacity,transform",
            }),
        });
      });
      return () => mm.revert();
    },
    { scope: ref },
  );

  return (
    <Tag ref={ref} className={className}>
      {children}
    </Tag>
  );
}
