"use client";

import { useRef } from "react";

import { gsap, useGSAP, motionOk, NO_PREFERENCE } from "@/lib/gsap";

const BAR_COUNT = 64;

/**
 * The bars behind the hero.
 *
 * Not real audio and not pretending to be — it is a rhythm, sized from
 * a fixed arithmetic sequence rather than `Math.random` so the server
 * and the client agree on every height and React never has to reconcile
 * a hydration mismatch.
 *
 * One GSAP timeline drives all sixty-four bars with a stagger, instead
 * of sixty-four independent animations. That is the difference between
 * one tick per frame and sixty-four, and on a page where this runs
 * forever behind the fold it is worth the difference.
 */
export function WaveformBackground() {
  const ref = useRef<HTMLDivElement>(null);

  const bars = Array.from({ length: BAR_COUNT }, (_, i) => {
    const seed = (i * 9301 + 49297) % 233280;
    return 8 + (seed / 233280) * 70;
  });

  useGSAP(
    () => {
      const mm = motionOk();
      mm.add(NO_PREFERENCE, () => {
        const els = ref.current?.querySelectorAll<HTMLElement>("[data-bar]");
        if (!els?.length) return;

        gsap.to(els, {
          scaleY: () => 0.35 + Math.abs(Math.sin(gsap.utils.random(0, Math.PI))) * 0.75,
          duration: 1.6,
          ease: "sine.inOut",
          repeat: -1,
          yoyo: true,
          // `from: "center"` makes the ripple travel outward from the
          // middle, which lines up with where the headline sits.
          stagger: { each: 0.045, from: "center", grid: "auto" },
        });
      });
      return () => mm.revert();
    },
    { scope: ref },
  );

  return (
    <div aria-hidden className="pointer-events-none absolute inset-0 -z-10 overflow-hidden">
      <div
        ref={ref}
        className="absolute inset-x-0 bottom-0 flex h-72 items-end justify-center gap-[3px] px-4 opacity-30"
      >
        {bars.map((height, i) => (
          <div
            key={i}
            data-bar
            className="w-[3px] origin-bottom rounded-full bg-gradient-to-t from-primary/0 via-primary/60 to-primary"
            style={{ height }}
          />
        ))}
      </div>
      <div className="absolute inset-0 bg-gradient-to-b from-background via-background/60 to-background" />
      <div className="absolute -top-40 left-1/2 h-[500px] w-[500px] -translate-x-1/2 rounded-full bg-primary/15 blur-3xl" />
    </div>
  );
}
