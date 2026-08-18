"use client";

import { useRef } from "react";

import { gsap, useGSAP, motionOk, NO_PREFERENCE } from "@/lib/gsap";

/**
 * A hairline at the top of the window showing how far down the page the
 * reader is.
 *
 * On a page this long the scrollbar is the only progress indicator, and
 * on macOS it is hidden until you scroll. This is the same information,
 * always visible, one pixel tall.
 *
 * `transformOrigin: left` with a scaled `div` rather than an animated
 * `width`: width animates layout, transform animates on the compositor.
 * At scroll rate that difference is the whole frame budget.
 */
export function ScrollProgress() {
  const ref = useRef<HTMLDivElement>(null);

  useGSAP(() => {
    const mm = motionOk();
    // A progress bar *is* motion, so it goes away entirely rather than
    // jumping between positions when motion is reduced.
    mm.add(NO_PREFERENCE, () => {
      gsap.fromTo(
        ref.current,
        { scaleX: 0 },
        {
          scaleX: 1,
          ease: "none",
          scrollTrigger: { start: 0, end: "max", scrub: 0.2 },
        },
      );
    });
    return () => mm.revert();
  });

  return (
    <div
      aria-hidden
      className="pointer-events-none fixed inset-x-0 top-0 z-[60] h-[2px] origin-left scale-x-0 bg-gradient-to-r from-primary via-fuchsia-400 to-primary"
      ref={ref}
    />
  );
}
