import { type ReactNode } from "react";

import { Footer } from "./footer";
import { SiteHeader } from "./site-header";

export function LegalShell({
  title,
  updated,
  children,
}: {
  title: string;
  updated?: string;
  children: ReactNode;
}) {
  return (
    <>
      <SiteHeader />
      <main className="pb-24 pt-32 md:pt-40">
        <article className="container mx-auto max-w-3xl">
          <h1 className="text-balance text-4xl font-bold tracking-tight sm:text-5xl">
            {title}
          </h1>
          {updated ? (
            <p className="mt-3 text-sm text-muted-foreground">
              Last updated: {updated}
            </p>
          ) : null}
          <div className="prose prose-invert mt-10 max-w-none text-foreground/90 [&_a]:text-primary [&_h2]:mt-10 [&_h2]:text-2xl [&_h2]:font-semibold [&_p]:mt-4 [&_p]:leading-relaxed [&_ul]:mt-4 [&_ul]:list-disc [&_ul]:pl-6 [&_li]:mt-1">
            {children}
          </div>
        </article>
      </main>
      <Footer />
    </>
  );
}
