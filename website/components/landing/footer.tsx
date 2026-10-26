import Link from "next/link";
import { Github } from "lucide-react";

import { CookieSettingsLink } from "@/components/analytics/cookie-settings-link";
import { LineDraw, Reveal, Stagger } from "@/components/motion";
import { Logo } from "@/components/motion/logo";
import { FOUNDER, LABYNATOR, otherApps } from "@/lib/family";
import { siteConfig } from "@/lib/site";

const links = [
  { href: siteConfig.github, label: "GitHub", external: true, icon: true },
  { href: siteConfig.designSpec, label: "Design spec", external: true },
  { href: "/blog", label: "Blog" },
  { href: "/videos", label: "Videos" },
  { href: "/docs", label: "Docs" },
  { href: "/changelog", label: "Changelog" },
  { href: "/press", label: "Press" },
  { href: "/privacy", label: "Privacy" },
  { href: "/terms", label: "Terms" },
  // Social handles intentionally omitted until accounts exist.
];

/** The home page's sections, which the header leaves out. */
export const homeSections = [
  { href: "/#demos", label: "Demos" },
  { href: "/#features", label: "Features" },
  { href: "/#tools", label: "Tools" },
  { href: "/#interface", label: "Interface" },
  { href: "/#faq", label: "FAQ" },
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
                  className="inline-flex items-center gap-1.5 py-2.5 text-muted-foreground sm:py-0 transition-colors hover:text-foreground"
                >
                  {l.icon ? <Github className="size-4" /> : null}
                  {l.label}
                </Link>
              ))}
            </Stagger>
          </LineDraw>
        </div>
        {/* The sections of the home page. The header no longer lists
            them (it keeps to five links), so this is where they stay one
            tap from any page. */}
        <nav
          aria-label="On the home page"
          className="mt-8 flex flex-wrap items-center gap-x-6 gap-y-2 text-sm"
        >
          <span className="font-medium">On the home page</span>
          {homeSections.map((s) => (
            <Link
              key={s.href}
              href={s.href}
              data-sweep
              className="inline-block py-2.5 text-muted-foreground transition-colors hover:text-foreground sm:py-0"
            >
              {s.label}
            </Link>
          ))}
        </nav>
        <nav
          aria-label="Use cases"
          className="mt-8 flex flex-wrap items-center gap-x-6 gap-y-2 text-sm"
        >
          <span className="font-medium">Use cases</span>
          <Link
            href="/use-cases/dj"
            data-sweep
            className="inline-block py-2.5 text-muted-foreground transition-colors hover:text-foreground sm:py-0"
          >
            AI audio editor for DJs
          </Link>
          <Link
            href="/use-cases/local-ai-audio-editor"
            data-sweep
            className="inline-block py-2.5 text-muted-foreground transition-colors hover:text-foreground sm:py-0"
          >
            Local AI audio editor
          </Link>
        </nav>
        <nav
          aria-label="Compare"
          className="mt-4 flex flex-wrap items-center gap-x-6 gap-y-2 text-sm"
        >
          <span className="font-medium">Compare</span>
          <Link
            href="/about"
            data-sweep
            className="inline-block py-2.5 text-muted-foreground transition-colors hover:text-foreground sm:py-0"
          >
            About and how we compare
          </Link>
          <Link
            href="/compare"
            data-sweep
            className="inline-block py-2.5 text-muted-foreground transition-colors hover:text-foreground sm:py-0"
          >
            AI audio editors compared
          </Link>
          <Link
            href="/alternatives/audacity"
            data-sweep
            className="inline-block py-2.5 text-muted-foreground transition-colors hover:text-foreground sm:py-0"
          >
            Audacity alternative
          </Link>
          <Link
            href="/alternatives/adobe-audition"
            data-sweep
            className="inline-block py-2.5 text-muted-foreground transition-colors hover:text-foreground sm:py-0"
          >
            Adobe Audition alternative
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
              className="inline-block py-2.5 font-medium text-foreground transition-colors hover:text-primary sm:py-0"
            >
              {LABYNATOR.name}
            </a>
          </span>
          {otherApps.map((app) => (
            <a
              key={app.url}
              href={app.url}
              data-sweep
              className="inline-block py-2.5 text-muted-foreground transition-colors hover:text-foreground sm:py-0"
            >
              {app.name} &mdash; {app.blurb}
            </a>
          ))}
          <span className="text-muted-foreground">
            Built by{" "}
            <a
              href={FOUNDER.url}
              data-sweep
              className="inline-block py-2.5 font-medium text-foreground transition-colors hover:text-primary sm:py-0"
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

        <div className="flex flex-wrap items-center gap-x-6 gap-y-2">
          <p className="text-xs text-muted-foreground" suppressHydrationWarning>
            © {new Date().getFullYear()} edytlab. Audio stays on your machine.
          </p>
          <CookieSettingsLink />
        </div>
      </div>
    </footer>
  );
}
