"use client";

import { useRef, type ReactNode } from "react";

import { gsap, useGSAP, motionOk, NO_PREFERENCE } from "@/lib/gsap";

interface MagneticProps {
  children: ReactNode;
  className?: string;
  /** How far it leans toward the cursor, as a fraction of the offset. */
  strength?: number;
}

/**
 * A control that leans very slightly toward the pointer.
 *
 * `quickTo` rather than a fresh tween per `mousemove`: pointer events
 * fire faster than frames, so creating a tween each time queues dozens
 * of overlapping animations for one gesture. `quickTo` reuses a single
 * tween and just retargets it, which is both smoother and cheaper.
 *
 * Bound to a fine pointer. On a touchscreen there is no hover to lead
 * with — the first "move" arrives with the tap, and the button would
 * slide out from under the finger pressing it.
 */
export function Magnetic({ children, className, strength = 0.28 }: MagneticProps) {
  const ref = useRef<HTMLDivElement>(null);

  useGSAP(
    () => {
      const el = ref.current;
      if (!el) return;

      const mm = motionOk();
      mm.add(`${NO_PREFERENCE} and (hover: hover) and (pointer: fine)`, () => {
        const xTo = gsap.quickTo(el, "x", { duration: 0.5, ease: "power3.out" });
        const yTo = gsap.quickTo(el, "y", { duration: 0.5, ease: "power3.out" });

        const move = (e: MouseEvent) => {
          const r = el.getBoundingClientRect();
          xTo((e.clientX - (r.left + r.width / 2)) * strength);
          yTo((e.clientY - (r.top + r.height / 2)) * strength);
        };
        const leave = () => {
          xTo(0);
          yTo(0);
        };

        el.addEventListener("mousemove", move);
        el.addEventListener("mouseleave", leave);
        return () => {
          el.removeEventListener("mousemove", move);
          el.removeEventListener("mouseleave", leave);
        };
      });
      return () => mm.revert();
    },
    { scope: ref },
  );

  return (
    <div ref={ref} className={`inline-block ${className ?? ""}`}>
      {children}
    </div>
  );
}
