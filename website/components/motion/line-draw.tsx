"use client";

import { useRef, type ElementType, type ReactNode } from "react";

import {
  gsap,
  useGSAP,
  motionOk,
  NO_PREFERENCE,
  REVEAL_START,
  ScrollTrigger,
  DRAWABLE,
  undash,
} from "@/lib/gsap";

interface LineDrawProps {
  children: ReactNode;
  className?: string;
  /** Which SVGs inside to draw. Defaults to every lucide icon. */
  selector?: string;
  as?: ElementType;
}

/** `REVEAL_START` as a fraction of the viewport, for the on-mount split. */
const REVEAL_LINE = 0.85;

/**
 * Icons that draw themselves, stroke by stroke, as they reach the screen.
 *
 * The site's icons are lucide's — outlines, two-pixel strokes, round
 * caps — which is exactly what DrawSVG animates: the pen traces each
 * stroke from its start to its end. Circles cannot be traced, so they
 * fade in behind the lines instead.
 *
 * Every icon is fully drawn in the server's HTML. Icons below the reveal
 * line at mount are hidden by the script and drawn by a batched
 * ScrollTrigger as they arrive; icons already on screen draw straight
 * away. The dash styles DrawSVG writes are cleared once a stroke lands,
 * so a finished icon is exactly the stylesheet's icon.
 */
export function LineDraw({
  children,
  className,
  selector = "svg.lucide",
  as: Tag = "div",
}: LineDrawProps) {
  const ref = useRef<HTMLElement>(null);

  useGSAP(
    () => {
      const root = ref.current;
      if (!root) return;

      const mm = motionOk();
      mm.add(NO_PREFERENCE, () => {
        const icons = gsap.utils.toArray<SVGSVGElement>(root.querySelectorAll(selector));
        if (!icons.length) return;

        const draw = (batch: Element[]) => {
          const strokes = batch.flatMap((svg) => [...svg.querySelectorAll(DRAWABLE)]);
          const dots = batch.flatMap((svg) => [...svg.querySelectorAll("circle")]);
          if (strokes.length) gsap.fromTo(
            strokes,
            { drawSVG: "0%" },
            {
              drawSVG: "100%",
              duration: 0.8,
              ease: "power2.inOut",
              stagger: 0.06,
              onComplete: undash(strokes),
            },
          );
          if (dots.length) {
            gsap.fromTo(dots, { opacity: 0 }, { opacity: 1, duration: 0.4, delay: 0.35, clearProps: "opacity" });
          }
        };

        const line = window.innerHeight * REVEAL_LINE;
        const now = icons.filter((el) => el.getBoundingClientRect().top <= line);
        const later = icons.filter((el) => el.getBoundingClientRect().top > line);

        if (now.length) draw(now);
        if (later.length) {
          const strokes = later.flatMap((svg) => [...svg.querySelectorAll(DRAWABLE)]);
          const dots = later.flatMap((svg) => [...svg.querySelectorAll("circle")]);
          if (strokes.length) gsap.set(strokes, { drawSVG: "0%" });
          if (dots.length) gsap.set(dots, { opacity: 0 });
          ScrollTrigger.batch(later, { start: REVEAL_START, once: true, onEnter: draw });
        }
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
