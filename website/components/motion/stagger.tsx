"use client";

import { useRef, type ElementType, type ReactNode } from "react";

import { gsap, useGSAP, motionOk, NO_PREFERENCE, REVEAL_START } from "@/lib/gsap";

interface StaggerProps {
  children: ReactNode;
  className?: string;
  /** What to animate inside. Defaults to the direct children. */
  selector?: string;
  /** Gap between each child, in seconds. */
  each?: number;
  delay?: number;
  distance?: number;
  duration?: number;
  /** Start each child slightly small as well as low. */
  scale?: boolean;
  as?: ElementType;
}

/**
 * A list that arrives one item at a time.
 *
 * The stagger is small on purpose. Past roughly 0.1s per item a
 * twelve-card grid takes over a second to finish, and the last card
 * animating long after the reader has started reading the first is a
 * distraction rather than a flourish — so the default is tuned for
 * "the grid settles" rather than "each card performs".
 */
export function Stagger({
  children,
  className,
  selector = ":scope > *",
  each = 0.07,
  delay = 0,
  distance = 24,
  duration = 0.6,
  scale = false,
  as: Tag = "div",
}: StaggerProps) {
  const ref = useRef<HTMLElement>(null);

  useGSAP(
    () => {
      const mm = motionOk();
      mm.add(NO_PREFERENCE, () => {
        const items = ref.current?.querySelectorAll(selector);
        if (!items?.length) return;
        gsap.from(items, {
          opacity: 0,
          y: distance,
          scale: scale ? 0.94 : 1,
          duration,
          delay,
          stagger: each,
          scrollTrigger: { trigger: ref.current, start: REVEAL_START, once: true },
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
