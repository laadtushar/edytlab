"use client";

import { useRef } from "react";

import { gsap, useGSAP, motionOk, NO_PREFERENCE } from "@/lib/gsap";

interface CounterProps {
  /** `"86"`, `"100%"`, or anything else — non-numeric values pass through. */
  value: string;
  className?: string;
}

/**
 * A number that counts up when it scrolls into view.
 *
 * The final value is what renders. GSAP overwrites the text content on
 * the way up and the last frame lands exactly on the target, so the
 * markup is correct before, during and after — and correct too when the
 * tween never runs. A counter that starts at "0" in the HTML is a
 * counter that shows "0" forever to anyone with motion turned off.
 */
export function Counter({ value, className }: CounterProps) {
  const ref = useRef<HTMLSpanElement>(null);
  const match = value.match(/^(\d+)(\D*)$/);

  useGSAP(
    () => {
      if (!match || !ref.current) return;
      const target = Number(match[1]);
      const suffix = match[2];
      const el = ref.current;

      const mm = motionOk();
      mm.add(NO_PREFERENCE, () => {
        const box = { n: 0 };
        gsap.to(box, {
          n: target,
          duration: 1.6,
          ease: "power2.out",
          snap: { n: 1 },
          onUpdate: () => {
            el.textContent = `${box.n}${suffix}`;
          },
          // Whatever happens — reverted, killed, interrupted — the
          // number ends where the markup says it should.
          onInterrupt: () => {
            el.textContent = value;
          },
          scrollTrigger: { trigger: el, start: "top 90%", once: true },
        });
      });
      return () => {
        mm.revert();
        el.textContent = value;
      };
    },
    { scope: ref, dependencies: [value] },
  );

  return (
    <span ref={ref} className={className}>
      {value}
    </span>
  );
}
