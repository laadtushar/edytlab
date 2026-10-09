"use client";

import { useRef } from "react";

import {
  gsap,
  useGSAP,
  motionOk,
  NO_PREFERENCE,
  REVEAL_START,
} from "@/lib/gsap";
import "@/lib/gsap-text";
import { cn } from "@/lib/utils";

import { Words } from "./words";

interface SectionHeadingProps {
  /** The small label above the heading. */
  eyebrow?: string;
  title: string;
  /** One or two sentences under the heading. */
  lead?: string;
  className?: string;
}

/** One cycle of a soft wave — the accent drawn under every heading. */
const ACCENT = "M2 6 C 10 0, 18 0, 26 6 S 42 12, 50 6 S 66 0, 74 6 S 90 12, 98 6";

/**
 * The heading every home-page section opens with: eyebrow, `h2`, lead.
 *
 * One component rather than the same three tags hand-written per
 * section, which is how the eyebrows had drifted into two different
 * type styles. They arrive in the order they are read, on one timeline
 * so they can never fall out of step:
 *
 * 1. the eyebrow resolves out of scrambled capitals (ScrambleText),
 * 2. the claim rises a word at a time,
 * 3. a short wave is drawn under it (DrawSVG) — the site's one
 *    recurring accent, a signal rather than a rule,
 * 4. the explanation follows.
 */
export function SectionHeading({ eyebrow, title, lead, className }: SectionHeadingProps) {
  const ref = useRef<HTMLDivElement>(null);

  useGSAP(
    () => {
      const mm = motionOk();
      mm.add(NO_PREFERENCE, () => {
        const tl = gsap.timeline({
          scrollTrigger: { trigger: ref.current, start: REVEAL_START, once: true },
        });
        if (eyebrow) {
          tl.from("[data-eyebrow]", { opacity: 0, y: 8, duration: 0.4 }).to(
            "[data-eyebrow]",
            {
              scrambleText: { text: "{original}", chars: "upperCase", speed: 0.6 },
              duration: 0.7,
              ease: "none",
            },
            "<",
          );
        }
        tl.from(".word", { yPercent: 110, opacity: 0, duration: 0.65, stagger: 0.04 }, eyebrow ? "-=0.45" : 0)
          .fromTo(
            "[data-accent]",
            { drawSVG: "50% 50%" },
            { drawSVG: "0% 100%", duration: 0.7, ease: "power2.inOut" },
            "-=0.35",
          );
        if (lead) tl.from("[data-lead]", { opacity: 0, y: 12, duration: 0.55 }, "-=0.55");
      });
      return () => mm.revert();
    },
    { scope: ref },
  );

  return (
    <div ref={ref} className={cn("mx-auto mb-12 max-w-2xl text-center", className)}>
      {eyebrow ? (
        // An invisible copy holds the line; the visible label sits over
        // it, so the scramble cannot change the line's width.
        <p className="relative mb-3 whitespace-nowrap font-mono text-xs uppercase tracking-widest text-primary">
          <span aria-hidden className="invisible">
            {eyebrow}
          </span>
          <span data-eyebrow className="absolute inset-0">
            {eyebrow}
          </span>
        </p>
      ) : null}
      <h2 className="text-balance text-3xl font-semibold tracking-tight sm:text-4xl">
        <Words text={title} />
      </h2>
      <svg aria-hidden viewBox="0 0 100 12" className="mx-auto mt-4 h-3 w-24 overflow-visible">
        <path
          data-accent
          d={ACCENT}
          fill="none"
          className="stroke-primary/70"
          strokeWidth="2"
          strokeLinecap="round"
        />
      </svg>
      {lead ? (
        <p data-lead className="mt-4 text-pretty text-lg text-muted-foreground">
          {lead}
        </p>
      ) : null}
    </div>
  );
}
