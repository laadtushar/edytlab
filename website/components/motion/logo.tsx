"use client";

import { useRef } from "react";

import {
  gsap,
  useGSAP,
  motionOk,
  NO_PREFERENCE,
  HOVER_MOTION,
  REVEAL_START,
} from "@/lib/gsap";

interface LogoProps {
  className?: string;
  /** When the mark builds itself: as the page loads, or when scrolled to. */
  play?: "load" | "scroll";
}

/**
 * The mark — three level bars and two rising strokes — drawn inline so
 * it can move.
 *
 * `public/logo.svg` is the same artwork for everything that needs a
 * file (the manifest, link previews); this is that file with its group
 * transform folded into the `viewBox`, so the shapes sit in their own
 * coordinates and can be animated one by one. The bars rise like a
 * meter catching a signal, the strokes are drawn by DrawSVG, and on
 * hover the bars bounce once — the meter reading the pointer.
 *
 * Rendered finished; the build-up is applied after mount, and only
 * when motion is welcome.
 */
export function Logo({ className, play = "load" }: LogoProps) {
  const ref = useRef<SVGSVGElement>(null);

  useGSAP(
    () => {
      const svg = ref.current;
      if (!svg) return;
      const mm = motionOk();

      mm.add(NO_PREFERENCE, () => {
        gsap
          .timeline({
            delay: play === "load" ? 0.15 : 0,
            scrollTrigger:
              play === "scroll" ? { trigger: svg, start: REVEAL_START, once: true } : undefined,
          })
          .from("[data-logo-bar]", {
            scaleY: 0.15,
            transformOrigin: "50% 50%",
            duration: 0.5,
            stagger: 0.08,
            ease: "back.out(2.2)",
          })
          .fromTo(
            "[data-logo-line]",
            { drawSVG: "0%" },
            {
              drawSVG: "100%",
              duration: 0.6,
              stagger: 0.1,
              ease: "power2.inOut",
              clearProps: "strokeDasharray,strokeDashoffset",
            },
            "-=0.25",
          );
      });

      mm.add(HOVER_MOTION, () => {
        const host = svg.closest("a") ?? svg;
        const bounce = () =>
          gsap.to(svg.querySelectorAll("[data-logo-bar]"), {
            keyframes: { scaleY: [1, 0.45, 1.18, 1] },
            transformOrigin: "50% 50%",
            duration: 0.6,
            stagger: 0.07,
            ease: "sine.inOut",
            overwrite: "auto",
          });
        host.addEventListener("pointerenter", bounce);
        return () => host.removeEventListener("pointerenter", bounce);
      });

      return () => mm.revert();
    },
    { scope: ref },
  );

  return (
    <svg
      ref={ref}
      viewBox="3.854 3.354 57.29 57.29"
      className={className}
      aria-hidden
      focusable="false"
    >
      <g fill="#9D4EF5">
        <rect data-logo-bar x="5" y="26" width="6" height="12" rx="3" />
        <rect data-logo-bar x="14" y="20" width="6" height="24" rx="3" />
        <rect data-logo-bar x="23" y="25" width="6" height="14" rx="3" />
      </g>
      <path
        data-logo-line
        d="M34 26 L41 18 L48 23 L57 12"
        fill="none"
        stroke="#9D4EF5"
        strokeWidth="6"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <path
        data-logo-line
        d="M34 38 L41 46 L48 41 L57 52"
        fill="none"
        stroke="#6248E8"
        strokeWidth="6"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}
