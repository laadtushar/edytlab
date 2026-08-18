"use client";

import Link from "next/link";
import { useRef } from "react";
import { Apple, Download } from "lucide-react";

import { Button } from "@/components/ui/button";
import { siteConfig } from "@/lib/site";
import { Magnetic } from "@/components/motion";
import { gsap, useGSAP, motionOk, NO_PREFERENCE } from "@/lib/gsap";

export function CTA() {
  const ref = useRef<HTMLElement>(null);

  useGSAP(
    () => {
      const mm = motionOk();
      mm.add(NO_PREFERENCE, () => {
        // The orb breathes on its own loop, independent of the copy.
        gsap.to("[data-orb]", {
          scale: 1.15,
          opacity: 0.85,
          duration: 4,
          ease: "sine.inOut",
          repeat: -1,
          yoyo: true,
        });

        gsap
          .timeline({
            scrollTrigger: { trigger: ref.current, start: "top 75%", once: true },
          })
          .from("[data-cta-h]", { opacity: 0, y: 26, duration: 0.7 })
          .from("[data-cta-p]", { opacity: 0, y: 18, duration: 0.6 }, "-=0.45")
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
      <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
        <div data-orb className="size-96 rounded-full bg-primary/10 opacity-50 blur-3xl" />
      </div>

      <div className="container relative">
        <div className="mx-auto max-w-2xl text-center">
          <h2
            data-cta-h
            className="text-balance text-3xl font-semibold tracking-tight sm:text-4xl md:text-5xl"
          >
            Stop fighting the DAW. <span className="gradient-text">Start describing.</span>
          </h2>
          <p data-cta-p className="mt-4 text-pretty text-lg text-muted-foreground">
            Free, open source, and local-first. Bring your own LLM key — your
            audio never leaves the machine.
          </p>
          <div
            data-cta-btns
            className="mt-10 flex flex-col items-center justify-center gap-3 sm:flex-row"
          >
            <Magnetic className="w-full sm:w-auto">
              <Button asChild size="lg" className="glow w-full">
                <Link href={siteConfig.releases}>
                  <Apple className="size-4" />
                  Download for Mac
                </Link>
              </Button>
            </Magnetic>
            <Magnetic className="w-full sm:w-auto">
              <Button asChild size="lg" variant="outline" className="w-full">
                <Link href={siteConfig.releases}>
                  <Download className="size-4" />
                  Download for Windows
                </Link>
              </Button>
            </Magnetic>
          </div>
          <p data-cta-note className="mt-4 text-xs text-muted-foreground">
            Unsigned dev builds · Mac (universal) · Windows 10/11 · Linux
          </p>
        </div>
      </div>
    </section>
  );
}
