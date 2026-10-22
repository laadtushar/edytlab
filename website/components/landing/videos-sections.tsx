import Link from "next/link";
import { BookOpen, Download, Github } from "lucide-react";
import type { ReactNode } from "react";

import { Button } from "@/components/ui/button";
import { siteConfig } from "@/lib/site";

/**
 * A section of the videos hub: a heading, at most one plain sentence,
 * then what it holds. Every section reads the same way, so a visitor
 * sees at a glance what each one is.
 */
export function VideoSection({
  id,
  title,
  lead,
  children,
}: {
  id: string;
  title: string;
  lead?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section aria-labelledby={id} className="mt-20">
      <h2 id={id} className="text-balance text-2xl font-semibold tracking-tight sm:text-3xl">
        {title}
      </h2>
      {lead ? (
        <p className="mt-3 max-w-2xl text-pretty leading-relaxed text-muted-foreground">{lead}</p>
      ) : null}
      <div className="mt-8">{children}</div>
    </section>
  );
}

/** The two buttons a visitor who has seen enough is looking for. */
export function VideoActions({ className }: { className?: string }) {
  return (
    <div className={`flex flex-col gap-3 sm:flex-row ${className ?? ""}`}>
      <Button asChild size="lg">
        <Link href={siteConfig.releases}>
          <Download />
          Download edytlab
        </Link>
      </Button>
      <Button asChild size="lg" variant="outline">
        <Link href="/docs">
          <BookOpen />
          Read the docs
        </Link>
      </Button>
    </div>
  );
}

/**
 * The close of a videos page: where to go after watching. Says what the
 * app costs and where it runs, and the two things a visitor meets right
 * after downloading (the installers are unsigned, and two features are
 * not built yet), in small type under the buttons.
 */
export function VideosCta({ heading = "Try it on your own audio" }: { heading?: string }) {
  return (
    <section
      aria-labelledby="videos-cta"
      className="mt-24 rounded-2xl border border-border/60 bg-secondary/20 px-6 py-10 text-center sm:px-12 sm:py-14"
    >
      <h2 id="videos-cta" className="text-balance text-2xl font-semibold tracking-tight sm:text-3xl">
        {heading}
      </h2>
      <p className="mx-auto mt-3 max-w-xl text-pretty leading-relaxed text-muted-foreground">
        edytlab is free and open source (MIT) for macOS, Windows and Linux. You bring your own AI provider key, or run Ollama on
        your own machine.
      </p>
      <VideoActions className="mt-8 justify-center" />
      <p className="mx-auto mt-6 max-w-xl text-pretty text-xs leading-relaxed text-muted-foreground">
        The installers are unsigned, so macOS and Windows warn the first time you open edytlab; the{" "}
        <Link href="/docs/getting-started" className="text-primary underline underline-offset-4">
          setup guide
        </Link>{" "}
        has the steps. Stem separation and transcription are not built yet. The source is on{" "}
        <a href={siteConfig.github} className="text-primary underline underline-offset-4" target="_blank" rel="noopener noreferrer">
          <Github className="mr-0.5 inline size-3.5 align-[-2px]" aria-hidden />
          GitHub
        </a>
        .
      </p>
    </section>
  );
}
