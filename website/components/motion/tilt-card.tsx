"use client";

import { useRef, type ReactNode } from "react";

import { gsap, useGSAP, motionOk, NO_PREFERENCE } from "@/lib/gsap";

interface TiltCardProps {
  children: ReactNode;
  className?: string;
}

/**
 * A card that tips toward the cursor and lights up where it is.
 *
 * The tilt is deliberately shallow — eight degrees. Steeper looks like a
 * toy and, on a card containing body text, makes the text genuinely
 * harder to read at the far edge.
 *
 * Both the rotation and the highlight are driven by `quickTo`, so a
 * gesture retargets one tween per property instead of stacking a new
 * one per pointer event. The highlight rides a CSS custom property, so
 * the gradient itself is declared in the stylesheet and only two numbers
 * cross the JS boundary.
 */
export function TiltCard({ children, className = "" }: TiltCardProps) {
  const ref = useRef<HTMLDivElement>(null);

  useGSAP(
    () => {
      const el = ref.current;
      if (!el) return;

      const mm = motionOk();
      mm.add(`${NO_PREFERENCE} and (hover: hover) and (pointer: fine)`, () => {
        const rx = gsap.quickTo(el, "rotateX", { duration: 0.5, ease: "power3.out" });
        const ry = gsap.quickTo(el, "rotateY", { duration: 0.5, ease: "power3.out" });
        const gx = gsap.quickSetter(el, "--gx", "%");
        const gy = gsap.quickSetter(el, "--gy", "%");

        const move = (e: MouseEvent) => {
          const r = el.getBoundingClientRect();
          const px = (e.clientX - r.left) / r.width;
          const py = (e.clientY - r.top) / r.height;
          rx((0.5 - py) * 8);
          ry((px - 0.5) * 8);
          gx(px * 100);
          gy(py * 100);
        };
        const enter = () => gsap.to(el, { scale: 1.02, duration: 0.4 });
        const leave = () => {
          rx(0);
          ry(0);
          gsap.to(el, { scale: 1, duration: 0.5 });
        };

        el.addEventListener("mousemove", move);
        el.addEventListener("mouseenter", enter);
        el.addEventListener("mouseleave", leave);
        return () => {
          el.removeEventListener("mousemove", move);
          el.removeEventListener("mouseenter", enter);
          el.removeEventListener("mouseleave", leave);
        };
      });
      return () => mm.revert();
    },
    { scope: ref },
  );

  return (
    <div
      ref={ref}
      className={`tilt-card relative cursor-default ${className}`}
      style={{ transformStyle: "preserve-3d", perspective: 900 }}
    >
      {children}
    </div>
  );
}
