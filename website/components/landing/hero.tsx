"use client";

import Link from "next/link";
import { useRef } from "react";
import { Apple, Download } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Magnetic, SplitWords } from "@/components/motion";
import { gsap, useGSAP, motionOk, NO_PREFERENCE } from "@/lib/gsap";
import type { ReleaseAssets } from "@/lib/releases";
import { WaveformBackground } from "./waveform-bg";

/**
 * The hero runs on a timeline rather than on per-element delays.
 *
 * Hand-tuned delays are how an entrance drifts out of sync: change the
 * headline animation and every number underneath it is silently wrong.
 * A timeline states the *order* — headline, then subhead overlapping its
 * tail, then buttons — and the relative offsets hold when any one
 * duration changes.
 */
export function Hero({ release }: { release: ReleaseAssets }) {
  const ref = useRef<HTMLDivElement>(null);

  useGSAP(
    () => {
      const mm = motionOk();
      mm.add(NO_PREFERENCE, () => {
        const tl = gsap.timeline({ defaults: { ease: "power3.out" } });
        tl.from("[data-hero-badge]", { opacity: 0, y: -14, scale: 0.85, duration: 0.5 })
          // `<` and `-=` keep these anchored to the headline's reveal,
          // which `SplitWords` owns and this timeline never sees.
          .from("[data-hero-sub]", { opacity: 0, y: 20, duration: 0.6 }, 0.75)
          .from("[data-hero-cta] > *", { opacity: 0, y: 20, stagger: 0.09, duration: 0.55 }, "-=0.35")
          .from("[data-hero-note]", { opacity: 0, duration: 0.5 }, "-=0.25");
      });
      return () => mm.revert();
    },
    { scope: ref },
  );

  return (
    <section className="relative pb-24 pt-32 md:pb-32 md:pt-40">
      <WaveformBackground />
      <div className="container relative">
        <div ref={ref} className="mx-auto max-w-4xl text-center">
          <div data-hero-badge>
            <Badge
              variant="outline"
              className="mb-6 border-primary/30 bg-primary/10 text-primary"
            >
              Local-first AI audio editor · {release.version}
            </Badge>
          </div>

          <h1 className="text-balance text-5xl font-bold tracking-tight sm:text-6xl md:text-7xl">
            <span className="mb-1 block">
              <SplitWords text="Describe it." />
            </span>
            <span className="block">
              <SplitWords
                text="Get pro-grade audio edits."
                wordClassName="gradient-text-split"
                delay={0.14}
              />
            </span>
          </h1>

          <p
            data-hero-sub
            className="mx-auto mt-6 max-w-2xl text-pretty text-lg text-muted-foreground md:text-xl"
          >
            Desktop audio editor where you chat with an AI to load, cut, mix,
            transcribe, and render. Pure-Rust DSP, local-first, BYO LLM key.
          </p>

          <div
            data-hero-cta
            className="mt-10 flex flex-col items-center justify-center gap-3 sm:flex-row"
          >
            {/* See scroll-story.tsx: a missing installer goes to the
                release page under a label that says so (#241). */}
            <Magnetic>
              <Button asChild size="lg" className="glow w-full sm:w-auto">
                <Link href={release.macUrl ?? release.releaseUrl}>
                  <Apple className="size-4" />
                  {release.macUrl ? "Download for Mac" : "Mac builds on GitHub"}
                </Link>
              </Button>
            </Magnetic>
            <Magnetic>
              <Button asChild size="lg" variant="outline" className="w-full sm:w-auto">
                <Link href={release.winUrl ?? release.releaseUrl}>
                  <Download className="size-4" />
                  {release.winUrl ? "Download for Windows" : "Windows builds on GitHub"}
                </Link>
              </Button>
            </Magnetic>
          </div>

          <p data-hero-note className="mt-4 text-xs text-muted-foreground">
            Unsigned dev builds · Mac (universal) · Windows 10/11 · Linux
          </p>
        </div>
      </div>
    </section>
  );
}
