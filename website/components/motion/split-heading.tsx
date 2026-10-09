"use client";

import { useRef, type ElementType } from "react";

import { gsap, useGSAP, motionOk, NO_PREFERENCE, REVEAL_START } from "@/lib/gsap";

import { Words } from "./words";

interface SplitHeadingProps {
  text: string;
  /** `h1` for a page title, `h2` for a section. */
  as?: ElementType;
  className?: string;
  /** Rise when scrolled to, rather than on mount. */
  onScroll?: boolean;
  delay?: number;
}

/**
 * A heading whose words rise into place, one after another.
 *
 * Page titles use it on mount — they are above the fold, and waiting
 * for a scroll would leave the top of the page blank. The words are in
 * the server's HTML either way; the rise is applied after hydration and
 * only when motion is welcome.
 */
export function SplitHeading({
  text,
  as: Tag = "h2",
  className,
  onScroll = false,
  delay = 0,
}: SplitHeadingProps) {
  const ref = useRef<HTMLElement>(null);

  useGSAP(
    () => {
      const mm = motionOk();
      mm.add(NO_PREFERENCE, () => {
        gsap.from(ref.current?.querySelectorAll(".word") ?? [], {
          yPercent: 110,
          opacity: 0,
          duration: 0.7,
          stagger: 0.045,
          delay,
          scrollTrigger: onScroll
            ? { trigger: ref.current, start: REVEAL_START, once: true }
            : undefined,
        });
      });
      return () => mm.revert();
    },
    { scope: ref },
  );

  return (
    <Tag ref={ref} className={className}>
      <Words text={text} />
    </Tag>
  );
}
