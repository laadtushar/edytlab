import Link from "next/link";
import { Github } from "lucide-react";

import { LineDraw, Reveal, Stagger } from "@/components/motion";
import { Logo } from "@/components/motion/logo";
import { FOUNDER, LABYNATOR, otherApps } from "@/lib/family";
import { siteConfig } from "@/lib/site";

const links = [
  { href: siteConfig.github, label: "GitHub", external: true, icon: true },
  { href: siteConfig.designSpec, label: "Design spec", external: true },
  { href: "/blog", label: "Blog" },
  { href: "/docs", label: "Docs" },
  { href: "/changelog", label: "Changelog" },
  { href: "/press", label: "Press" },
  { href: "/privacy", label: "Privacy" },
  { href: "/terms", label: "Terms" },
  // Social handles intentionally omitted until accounts exist.
];

/**
 * The foot of every page. The mark builds itself as it scrolls into
 * view, the links arrive left to right, the rule draws out from the
 * middle, and each link underlines with a sweep on hover.
 *
 * The LabyNator family block (`lib/family.ts`) is plain markup with no
 * entrance of its own: it is in the page whether or not scripts run,
 * and with reduced motion the sweep falls back to a plain underline
 * (see `[data-sweep]` in `globals.css`). Its links are ordinary
 * same-tab links to the sites' canonical URLs.
 */
export function Footer() {
  return (
    <footer className="border-t border-border/50 bg-secondary/20">
      <div className="container py-12">
        <div className="flex flex-col items-start justify-between gap-6 md:flex-row md:items-center">
          <Reveal distance={16}>
            <div className="flex items-center gap-2 text-lg font-semibold">
              <Logo className="size-6" play="scroll" />
              edytlab
            </div>
            <p className="mt-1 text-sm text-muted-foreground">
              Describe it. Get pro-grade audio edits.
            </p>
          </Reveal>
          <LineDraw>
            <Stagger className="flex flex-wrap items-center gap-6 text-sm" each={0.05} distance={10}>
              {links.map((l) => (
                <Link
                  key={l.href}
                  href={l.href}
                  data-sweep
                  {...(l.external ? { target: "_blank", rel: "noopener noreferrer" } : {})}
                  className="inline-flex items-center gap-1.5 text-muted-foreground transition-colors hover:text-foreground"
                >
                  {l.icon ? <Github className="size-4" /> : null}
                  {l.label}
                </Link>
              ))}
            </Stagger>
          </LineDraw>
        </div>
        <nav
          aria-label="Use cases"
          className="mt-8 flex flex-wrap items-center gap-x-6 gap-y-2 text-sm"
        >
          <span className="font-medium">Use cases</span>
          <Link
            href="/use-cases/dj"
            data-sweep
            className="text-muted-foreground transition-colors hover:text-foreground"
          >
            AI audio editor for DJs
          </Link>
          <Link
            href="/use-cases/local-ai-audio-editor"
            data-sweep
            className="text-muted-foreground transition-colors hover:text-foreground"
          >
            Local AI audio editor
          </Link>
        </nav>
        <nav
          aria-label="The LabyNator family"
          className="mt-4 flex flex-wrap items-center gap-x-6 gap-y-2 text-sm"
        >
          <span className="text-muted-foreground">
            Part of{" "}
            <a
              href={LABYNATOR.url}
              data-sweep
              className="font-medium text-foreground transition-colors hover:text-primary"
            >
              {LABYNATOR.name}
            </a>
          </span>
          {otherApps.map((app) => (
            <a
              key={app.url}
              href={app.url}
              data-sweep
              className="text-muted-foreground transition-colors hover:text-foreground"
            >
              {app.name} &mdash; {app.blurb}
            </a>
          ))}
          <span className="text-muted-foreground">
            Built by{" "}
            <a
              href={FOUNDER.url}
              data-sweep
              className="font-medium text-foreground transition-colors hover:text-primary"
            >
              {FOUNDER.name}
            </a>
          </span>
        </nav>
        {/* The rule draws out from the middle: `scaleX` from 0, which is
            a transform — the footer's height never changes. */}
        <Reveal
          from={{ scaleX: 0 }}
          duration={0.9}
          className="my-8 h-px w-full bg-border"
        />

        <p className="text-xs text-muted-foreground" suppressHydrationWarning>
          © {new Date().getFullYear()} edytlab. Audio stays on your machine.
        </p>
      </div>
    </footer>
  );
}
