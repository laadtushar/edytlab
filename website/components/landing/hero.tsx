import Link from "next/link";
import { Apple, Download } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { siteConfig } from "@/lib/site";

import { WaveformBackground } from "./waveform-bg";
import { FadeIn } from "./fade-in";

export function Hero() {
  return (
    <section className="relative overflow-hidden pb-24 pt-32 md:pb-32 md:pt-40">
      <WaveformBackground />
      <div className="container relative">
        <FadeIn className="mx-auto max-w-4xl text-center">
          <Badge
            variant="outline"
            className="mb-6 border-primary/30 bg-primary/10 text-primary"
          >
            Local-first AI audio editor · {siteConfig.version}
          </Badge>
          <h1 className="text-balance text-5xl font-bold tracking-tight sm:text-6xl md:text-7xl">
            Talk to Claude.
            <br />
            <span className="gradient-text">Get pro-grade audio edits.</span>
          </h1>
          <p className="mx-auto mt-6 max-w-2xl text-pretty text-lg text-muted-foreground md:text-xl">
            Desktop audio editor where you chat with an AI to load, cut, mix,
            transcribe, and render. Pure-Rust DSP, local-first, BYO LLM key.
          </p>
          <div className="mt-10 flex flex-col items-center justify-center gap-3 sm:flex-row">
            <Button asChild size="lg" className="glow w-full sm:w-auto">
              <Link href={siteConfig.releases}>
                <Apple className="size-4" />
                Download for Mac
              </Link>
            </Button>
            <Button
              asChild
              size="lg"
              variant="outline"
              className="w-full sm:w-auto"
            >
              <Link href={siteConfig.releases}>
                <Download className="size-4" />
                Download for Windows
              </Link>
            </Button>
          </div>
          <p className="mt-4 text-xs text-muted-foreground">
            Signed installers · Universal Mac (Apple Silicon + Intel) · Windows 10/11
          </p>
        </FadeIn>
      </div>
    </section>
  );
}
