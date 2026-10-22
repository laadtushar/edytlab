import { type ReactNode } from "react";

import { JsonLd } from "@/components/json-ld";
import { Cascade, Reveal, SplitHeading } from "@/components/motion";
import { breadcrumbJsonLd, pageBreadcrumbs } from "@/lib/seo-core";

import { Footer } from "./footer";
import { SiteHeader } from "./site-header";

/**
 * The text pages — privacy, terms, the changelog.
 *
 * `path` is the page's own path. The shell writes the page's
 * BreadcrumbList from it and `title` (Home, then the page), so the three
 * pages that use it cannot forget the trail, and it sits outside the
 * `Cascade`, which would otherwise treat the script as content to reveal.
 */
export function LegalShell({
  title,
  path,
  updated,
  children,
}: {
  title: string;
  path: string;
  updated?: string;
  children: ReactNode;
}) {
  return (
    <>
      <JsonLd data={breadcrumbJsonLd(pageBreadcrumbs(title, path))} />
      <SiteHeader />
      <main className="pb-24 pt-32 md:pt-40">
        <article className="container mx-auto max-w-3xl">
          {/* The title rises a word at a time on arrival; the date and
              the text follow it down. */}
          <SplitHeading
            as="h1"
            text={title}
            className="text-balance text-4xl font-bold tracking-tight sm:text-5xl"
          />
          {updated ? (
            <Reveal distance={10} delay={0.25}>
              <p className="mt-3 text-sm text-muted-foreground">
                Last updated: {updated}
              </p>
            </Reveal>
          ) : null}
          <Cascade
            prose
            className="prose prose-invert mt-10 max-w-none text-foreground/90 [&_a]:text-primary [&_h2]:mt-10 [&_h2]:text-2xl [&_h2]:font-semibold [&_p]:mt-4 [&_p]:leading-relaxed [&_ul]:mt-4 [&_ul]:list-disc [&_ul]:pl-6 [&_li]:mt-1"
          >
            {children}
          </Cascade>
        </article>
      </main>
      <Footer />
    </>
  );
}
