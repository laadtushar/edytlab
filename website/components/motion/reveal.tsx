"use client";

import { useRef, type ElementType, type ReactNode } from "react";

import { gsap, useGSAP, motionOk, NO_PREFERENCE, REVEAL_START } from "@/lib/gsap";

type Direction = "up" | "down" | "left" | "right" | "none";

interface RevealProps {
  children: ReactNode;
  className?: string;
  /** Seconds to wait after the trigger fires. */
  delay?: number;
  direction?: Direction;
  /** How far it travels, in px. */
  distance?: number;
  duration?: number;
  /** Render as something other than a div — `ul`, `section`, `dl`. */
  as?: ElementType;
  /** Blur-in as well as fade. Reads as depth; use sparingly. */
  blur?: boolean;
}

const offset: Record<Direction, { x?: number; y?: number }> = {
  up: { y: 1 },
  down: { y: -1 },
  left: { x: 1 },
  right: { x: -1 },
  none: {},
};

/**
 * The workhorse: fade a block in as it arrives.
 *
 * Written so the *rendered* state is the finished state — the from-state
 * is applied by GSAP, never by CSS. That ordering is what keeps the page
 * readable when JavaScript has not run, when it fails, or when the
 * reader has asked for reduced motion: in all three the content is
 * simply there. A CSS `opacity: 0` default would turn any of those into
 * a blank page.
 */
export function Reveal({
  children,
  className,
  delay = 0,
  direction = "up",
  distance = 24,
  duration = 0.7,
  as: Tag = "div",
  blur = false,
}: RevealProps) {
  const ref = useRef<HTMLElement>(null);

  useGSAP(
    () => {
      const mm = motionOk();
      mm.add(NO_PREFERENCE, () => {
        const { x = 0, y = 0 } = offset[direction];
        gsap.from(ref.current, {
          opacity: 0,
          x: x * distance,
          y: y * distance,
          filter: blur ? "blur(8px)" : undefined,
          duration,
          delay,
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
