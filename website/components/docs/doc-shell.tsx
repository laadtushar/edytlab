"use client";

import { type ReactNode } from "react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { ArrowLeft } from "lucide-react";

import { SiteHeader } from "@/components/landing/site-header";
import { Footer } from "@/components/landing/footer";
import { Cascade, LineDraw, Reveal, SplitHeading, Stagger } from "@/components/motion";
import { breadcrumbJsonLd, serializeJsonLd, type Crumb } from "@/lib/seo-core";

const navGroups = [
  {
    label: "Getting Started",
    links: [
      { href: "/docs", label: "Overview" },
      { href: "/docs/getting-started", label: "Installation & Setup" },
    ],
  },
  {
    label: "User Guide",
    links: [
      { href: "/docs/user-guide", label: "Using edytlab" },
      { href: "/docs/tools", label: "Audio Tools Reference" },
    ],
  },
  {
    label: "Developer",
    links: [
      { href: "/docs/developer-guide", label: "Development Guide" },
      { href: "/docs/api-reference", label: "API Reference" },
    ],
  },
];

export function DocShell({
  title,
  description,
  parent,
  aside,
  children,
}: {
  title: string;
  description?: string;
  /**
   * The page this one sits under, for a page one level below a docs page
   * (an area of the tools reference): the breadcrumb gets it between
   * Documentation and the page, and a link back to it shows above the
   * heading.
   */
  parent?: { name: string; path: string };
  /**
   * A column to the right of the content, sticky, from `xl` up (the
   * "On this page" list of a long reference page). Below `xl` it is not
   * shown: the page puts its own, collapsed, in the content.
   */
  aside?: ReactNode;
  children: ReactNode;
}) {
  const pathname = usePathname();

  // Home > Documentation > [parent >] this page. The overview page is its
  // own "Documentation" crumb, so it gets two crumbs, not three.
  const trail: Crumb[] = [
    { name: "Home", path: "/" },
    { name: "Documentation", path: "/docs" },
    ...(parent ? [parent] : []),
    ...(pathname === "/docs" ? [] : [{ name: title, path: pathname }]),
  ];

  return (
    <>
      <script
        type="application/ld+json"
        dangerouslySetInnerHTML={{ __html: serializeJsonLd(breadcrumbJsonLd(trail)) }}
      />
      <SiteHeader />
      <div className="min-h-screen pt-14">
        <div className="container mx-auto flex gap-0 lg:gap-8 xl:gap-12">
          {/* Sidebar */}
          <aside className="hidden w-56 shrink-0 lg:block">
            <Stagger className="sticky top-20 max-h-[calc(100vh-6rem)] overflow-y-auto pb-12 pt-8" each={0.08} distance={14}>
              {navGroups.map((group) => (
                <div key={group.label} className="mb-6">
                  <p className="mb-2 text-xs font-semibold uppercase tracking-widest text-muted-foreground">
                    {group.label}
                  </p>
                  <ul className="space-y-0.5">
                    {group.links.map((link) => {
                      // A page under a link (an area of the tools reference)
                      // keeps its section's link lit; the overview only
                      // for itself, since every docs page is under /docs.
                      const active =
                        pathname === link.href ||
                        (link.href !== "/docs" && pathname.startsWith(`${link.href}/`));
                      return (
                        <li key={link.href} className="relative">
                          {/* The page you are on: a bar that grows in
                              beside its link as the page arrives. */}
                          {active ? (
                            <Reveal
                              from={{ scaleY: 0 }}
                              duration={0.5}
                              delay={0.3}
                              className="absolute inset-y-1 left-0 w-0.5 rounded-full bg-primary"
                            />
                          ) : null}
                          <Link
                            href={link.href}
                            data-nudge={active ? undefined : "3"}
                            className={`block rounded-md px-3 py-1.5 text-sm transition-colors ${
                              active
                                ? "bg-primary/10 font-medium text-primary"
                                : "text-muted-foreground hover:bg-secondary/60 hover:text-foreground"
                            }`}
                          >
                            {link.label}
                          </Link>
                        </li>
                      );
                    })}
                  </ul>
                </div>
              ))}
            </Stagger>
          </aside>

          {/* Content */}
          <main className="min-w-0 flex-1 py-8 pb-24">
            <div className="mx-auto max-w-3xl lg:mx-0">
              {parent ? (
                <Link
                  href={parent.path}
                  className="mb-3 inline-flex items-center gap-1.5 text-sm text-muted-foreground transition-colors hover:text-foreground"
                >
                  <ArrowLeft className="size-3.5" aria-hidden />
                  {parent.name}
                </Link>
              ) : null}
              <SplitHeading
                as="h1"
                text={title}
                className="text-3xl font-bold tracking-tight sm:text-4xl"
              />
              {description && (
                <Reveal distance={10} delay={0.2}>
                  <p className="mt-3 text-lg text-muted-foreground">
                    {description}
                  </p>
                </Reveal>
              )}
              <Reveal from={{ scaleX: 0 }} duration={0.9} delay={0.3} className="rule-fade mt-6" />
              {/* Blocks, list items and table rows cascade in as they
                  scroll up; any icon among them draws itself. On a phone
                  a table too wide for the screen scrolls on its own,
                  instead of pushing the whole page sideways. */}
              <LineDraw>
                <Cascade
                  prose
                  className="mt-8 text-foreground/90 [&>:first-child]:mt-0 [&_a:not(:where(.not-prose,.not-prose_*))]:[overflow-wrap:anywhere] [&_blockquote]:my-4 [&_blockquote]:rounded-lg [&_blockquote]:border [&_blockquote]:border-primary/30 [&_blockquote]:bg-primary/5 [&_blockquote]:p-4 [&_blockquote]:text-sm [&_code:not(:where(.not-prose,.not-prose_*))]:break-words [&_code:not(:where(.not-prose,.not-prose_*))]:rounded [&_code:not(:where(.not-prose,.not-prose_*))]:bg-secondary [&_code:not(:where(.not-prose,.not-prose_*))]:px-1.5 [&_code:not(:where(.not-prose,.not-prose_*))]:py-0.5 [&_code:not(:where(.not-prose,.not-prose_*))]:text-sm [&_code:not(:where(.not-prose,.not-prose_*))]:font-mono [&_h2]:mb-3 [&_h2]:mt-10 [&_h2]:text-xl [&_h2]:font-semibold [&_h3]:mb-2 [&_h3]:mt-6 [&_h3]:text-base [&_h3]:font-semibold [&_li:not(:where(.not-prose,.not-prose_*))]:mt-1 [&_ol:not(:where(.not-prose,.not-prose_*))]:mt-3 [&_ol:not(:where(.not-prose,.not-prose_*))]:list-decimal [&_ol:not(:where(.not-prose,.not-prose_*))]:pl-6 [&_p:not(:where(.not-prose,.not-prose_*))]:mt-4 [&_p:not(:where(.not-prose,.not-prose_*))]:leading-7 [&_pre]:my-4 [&_pre]:overflow-x-auto [&_pre]:rounded-lg [&_pre]:bg-secondary [&_pre]:p-4 [&_pre_code]:bg-transparent [&_pre_code]:p-0 [&_table]:my-4 [&_table]:w-full max-sm:[&_table]:block max-sm:[&_table]:overflow-x-auto [&_table]:border-collapse [&_td]:border [&_td]:border-border/50 [&_td]:px-3 [&_td]:py-2 [&_td]:text-sm [&_th]:border [&_th]:border-border/50 [&_th]:bg-secondary/60 [&_th]:px-3 [&_th]:py-2 [&_th]:text-left [&_th]:text-sm [&_th]:font-semibold [&_ul:not(:where(.not-prose,.not-prose_*))]:mt-3 [&_ul:not(:where(.not-prose,.not-prose_*))]:list-disc [&_ul:not(:where(.not-prose,.not-prose_*))]:pl-6 [&_a:not(:where(.not-prose,.not-prose_*))]:text-primary [&_a:not(:where(.not-prose,.not-prose_*))]:underline [&_a:not(:where(.not-prose,.not-prose_*))]:underline-offset-4 [&_a:not(:where(.not-prose,.not-prose_*)):hover]:no-underline"
                >
                  {children}
                </Cascade>
              </LineDraw>
            </div>
          </main>

          {/* Right column, wide screens only: sticky, and it scrolls
              inside itself when its list is taller than the screen. */}
          {aside ? (
            <aside className="hidden w-52 shrink-0 xl:block">
              <div className="sticky top-20 max-h-[calc(100vh-6rem)] overflow-y-auto pb-12 pt-8">{aside}</div>
            </aside>
          ) : null}
        </div>
      </div>
      <Footer />
    </>
  );
}
