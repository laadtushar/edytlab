import Link from "next/link";
import { type ReactNode } from "react";
import { BookOpen, Download, Newspaper } from "lucide-react";

import { Button } from "@/components/ui/button";
import { siteConfig } from "@/lib/site";

import { Footer } from "./footer";
import { SiteHeader } from "./site-header";

/**
 * The shell for the marketing pages that are longer than a legal page:
 * the press kit and the use-case pages. `LegalShell` is the same idea
 * with a narrower column and a prose wrapper around everything; these
 * pages put a video and a call to action between the prose, so the prose
 * is a component of its own (`Prose`) rather than the whole page.
 */

export function PageShell({
  eyebrow,
  title,
  lead,
  children,
}: {
  eyebrow: string;
  title: string;
  lead: ReactNode;
  children: ReactNode;
}) {
  return (
    <>
      <SiteHeader />
      <main className="pb-24 pt-32 md:pt-40">
        <article className="container mx-auto max-w-3xl">
          <p className="font-mono text-xs uppercase tracking-widest text-primary">
            {eyebrow}
          </p>
          <h1 className="mt-3 text-balance text-4xl font-bold tracking-tight sm:text-5xl">
            {title}
          </h1>
          <p className="mt-6 text-pretty text-lg leading-relaxed text-muted-foreground">
            {lead}
          </p>
          {children}
        </article>
      </main>
      <Footer />
    </>
  );
}

/** Prose styled the way `LegalShell` styles its body. */
export function Prose({ children }: { children: ReactNode }) {
  return (
    <div className="prose prose-invert mt-10 max-w-none text-foreground/90 [&_a]:text-primary [&_h2]:mt-12 [&_h2]:text-2xl [&_h2]:font-semibold [&_p]:mt-4 [&_p]:leading-relaxed [&_ul]:mt-4 [&_ul]:list-disc [&_ul]:pl-6 [&_li]:mt-1">
      {children}
    </div>
  );
}

/** The closing row: download, docs, blog. Every marketing page ends with it. */
export function PageLinks() {
  return (
    <div className="mt-14 flex flex-col gap-3 border-t border-border/50 pt-8 sm:flex-row">
      <Button asChild>
        <Link href={siteConfig.releases}>
          <Download />
          Download edytlab
        </Link>
      </Button>
      <Button asChild variant="outline">
        <Link href="/docs">
          <BookOpen />
          Read the docs
        </Link>
      </Button>
      <Button asChild variant="outline">
        <Link href="/blog">
          <Newspaper />
          Blog
        </Link>
      </Button>
    </div>
  );
}
