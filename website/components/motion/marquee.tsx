"use client";

import { useRef, type ReactNode } from "react";

import { gsap, useGSAP, motionOk, NO_PREFERENCE } from "@/lib/gsap";

interface MarqueeProps {
  children: ReactNode;
  /** Seconds for one full pass. Longer is calmer. */
  duration?: number;
  reverse?: boolean;
  className?: string;
}

/**
 * An endless horizontal scroll.
 *
 * The track is rendered twice and translated by exactly -50%, which is
 * what makes the loop seamless: at the moment it resets, the second copy
 * sits precisely where the first one started. Any other distance shows a
 * jump once per cycle.
 *
 * The duplicate is `aria-hidden` — it is the same words again, and a
 * screen reader should not read the list twice.
 */
export function Marquee({ children, duration = 40, reverse = false, className }: MarqueeProps) {
  const ref = useRef<HTMLDivElement>(null);

  useGSAP(
    () => {
      const mm = motionOk();
      mm.add(NO_PREFERENCE, () => {
        const tween = gsap.to(ref.current, {
          xPercent: reverse ? 50 : -50,
          duration,
          ease: "none",
          repeat: -1,
        });
        if (reverse) gsap.set(ref.current, { xPercent: -50 });

        // Slow to a crawl on hover so a name can actually be read.
        const el = ref.current?.parentElement;
        const slow = () => gsap.to(tween, { timeScale: 0.15, duration: 0.4 });
        const go = () => gsap.to(tween, { timeScale: 1, duration: 0.4 });
        el?.addEventListener("mouseenter", slow);
        el?.addEventListener("mouseleave", go);
        return () => {
          el?.removeEventListener("mouseenter", slow);
          el?.removeEventListener("mouseleave", go);
        };
      });
      return () => mm.revert();
    },
    { scope: ref },
  );

  return (
    <div
      className={`group relative flex overflow-hidden [mask-image:linear-gradient(to_right,transparent,black_12%,black_88%,transparent)] ${className ?? ""}`}
    >
      <div ref={ref} className="flex w-max shrink-0">
        <div className="flex shrink-0">{children}</div>
        <div className="flex shrink-0" aria-hidden>
          {children}
        </div>
      </div>
    </div>
  );
}
