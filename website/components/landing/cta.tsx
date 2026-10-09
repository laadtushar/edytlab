"use client";

import Link from "next/link";
import { useRef } from "react";

import { BUILD_NOTE, DownloadButtons } from "@/components/landing/download-buttons";
import { Words } from "@/components/motion/words";
import { gsap, useGSAP, motionOk, NO_PREFERENCE, whileVisible } from "@/lib/gsap";
import type { ReleaseAssets } from "@/lib/releases";

/** Deterministic pseudo-random in [0, 1) — server and client agree. */
function seeded(i: number) {
  return ((Math.sin(i * 91.7 + 17.3) * 43758.5453) % 1 + 1) / 2;
}

const BARS = 48;
/** Resting heights, 0–1 of the meter. The markup's static picture. */
const LEVELS = Array.from({ length: BARS }, (_, i) => 0.25 + seeded(i) * 0.6);

/**
 * The close of the page: one line and the download.
 *
 * It used to restate the pitch — free, open source, local-first, bring
 * your own key — which by this point the reader has seen in the stats,
 * the comparison, the provider cards and the FAQ. What this spot has
 * that no other does is the moment of deciding, so it carries only the
 * buttons and the one thing worth knowing before pressing one: the
 * builds are unsigned, and the first launch takes an extra step.
 *
 * Motion: the headline rises a word at a time, the buttons follow, and
 * behind it all a wide, faint level meter — the logo's bars, as a
 * signal — dances while the section is on screen and stops when it is
 * not. The glow behind the copy breathes and drifts against the scroll.
 */
export function CTA({ release }: { release: ReleaseAssets }) {
  const ref = useRef<HTMLElement>(null);

  useGSAP(
    () => {
      const mm = motionOk();
      mm.add(NO_PREFERENCE, () => {
        const section = ref.current;

        // The orb breathes on its own loop, and drifts with the scroll.
        const breathe = gsap.to("[data-orb]", {
          scale: 1.15,
          opacity: 0.85,
          duration: 4,
          ease: "sine.inOut",
          repeat: -1,
          yoyo: true,
        });
        whileVisible(breathe, section);
        gsap.fromTo(
          "[data-orb-drift]",
          { y: 60 },
          {
            y: -60,
            ease: "none",
            scrollTrigger: { trigger: section, start: "top bottom", end: "bottom top", scrub: true },
          },
        );

        // The meter: every bar to a new level, staggered from the
        // centre outwards, forever — but only while it can be seen.
        const meter = gsap.to("[data-meter-bar]", {
          scaleY: () => 0.15 + Math.random() * 0.85,
          transformOrigin: "50% 100%",
          duration: 0.45,
          ease: "sine.inOut",
          stagger: { each: 0.02, from: "center", repeat: -1, yoyo: true, repeatRefresh: true },
        });
        whileVisible(meter, section);

        gsap
          .timeline({
            scrollTrigger: { trigger: section, start: "top 75%", once: true },
          })
          .from("[data-cta-h] .word", { yPercent: 110, opacity: 0, duration: 0.7, stagger: 0.05 })
          .from("[data-cta-btns] > *", { opacity: 0, y: 18, stagger: 0.1, duration: 0.55 }, "-=0.35")
          .from("[data-cta-note]", { opacity: 0, duration: 0.5 }, "-=0.3");
      });
      return () => mm.revert();
    },
    { scope: ref },
  );

  return (
    <section
      ref={ref}
      className="relative overflow-hidden border-t border-border/50 bg-secondary/20 py-24 md:py-32"
    >
      <div
        data-orb-drift
        className="pointer-events-none absolute inset-0 flex items-center justify-center"
      >
        <div data-orb className="size-96 rounded-full bg-primary/10 opacity-50 blur-3xl" />
      </div>

      {/* The level meter along the foot of the section. Decoration:
          hidden from assistive tech, faded at both ends. */}
      <svg
        aria-hidden
        viewBox={`0 0 ${BARS * 10} 80`}
        preserveAspectRatio="none"
        className="pointer-events-none absolute inset-x-0 bottom-0 h-20 w-full opacity-25 [mask-image:linear-gradient(to_right,transparent,black_20%,black_80%,transparent)]"
      >
        {LEVELS.map((level, i) => (
          <rect
            key={i}
            data-meter-bar
            x={i * 10 + 2}
            y={80 - level * 80}
            width="6"
            height={level * 80}
            rx="2"
            className="fill-primary"
          />
        ))}
      </svg>

      <div className="container relative">
        <div className="mx-auto max-w-2xl text-center">
          <h2
            data-cta-h
            className="text-balance text-3xl font-semibold tracking-tight sm:text-4xl md:text-5xl"
          >
            <Words text="Stop fighting the DAW." />{" "}
            <Words text="Start describing." className="gradient-text-split" />
          </h2>
          <DownloadButtons data-cta-btns release={release} className="mt-10" />
          <p data-cta-note className="mt-4 text-xs text-muted-foreground">
            {BUILD_NOTE} ·{" "}
            <Link
              href="/docs/getting-started"
              data-sweep
              className="text-primary"
            >
              First-launch steps
            </Link>
          </p>
        </div>
      </div>
    </section>
  );
}
