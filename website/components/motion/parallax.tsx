"use client";

import { useRef, type ReactNode } from "react";

import { gsap, useGSAP, motionOk, NO_PREFERENCE } from "@/lib/gsap";

interface ParallaxProps {
  children: ReactNode;
  className?: string;
  /**
   * How far it drifts across the whole scroll pass, in px. Negative
   * rises against the scroll, positive lags behind it.
   */
  distance?: number;
}

/**
 * Scrub-linked drift: the element moves with the scrollbar rather than
 * on a timer.
 *
 * `scrub: true` and not a number, so the element is exactly where the
 * scroll position says it is. Smoothed scrubbing looks nicer in
 * isolation and feels like lag when the reader flicks a trackpad, which
 * is the common case on a long page.
 *
 * Keep the distance small. Parallax that outruns the text next to it
 * reads as a rendering bug.
 */
export function Parallax({ children, className, distance = -60 }: ParallaxProps) {
  const ref = useRef<HTMLDivElement>(null);

  useGSAP(
    () => {
      const mm = motionOk();
      mm.add(NO_PREFERENCE, () => {
        gsap.to(ref.current, {
          y: distance,
          ease: "none",
          scrollTrigger: {
            trigger: ref.current,
            start: "top bottom",
            end: "bottom top",
            scrub: true,
          },
        });
      });
      return () => mm.revert();
    },
    { scope: ref },
  );

  return (
    <div ref={ref} className={className}>
      {children}
    </div>
  );
}
