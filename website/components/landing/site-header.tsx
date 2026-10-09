"use client";

import Link from "next/link";
import { useEffect, useRef } from "react";
import { Download, Github } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Logo } from "@/components/motion/logo";
import { gsap, useGSAP, motionOk, NO_PREFERENCE, ScrollTrigger } from "@/lib/gsap";
import { siteConfig } from "@/lib/site";

/** In page order, so the highlight walks left to right as you scroll. */
const links = [
  { href: "/#demos", label: "Demos" },
  { href: "/#features", label: "Features" },
  { href: "/#tools", label: "Tools" },
  { href: "/#interface", label: "Interface" },
  { href: "/#faq", label: "FAQ" },
  { href: "/blog", label: "Blog" },
  { href: "/docs", label: "Docs" },
];

/** The section id a home-page link points at, if it is one. */
function sectionId(href: string) {
  return href.startsWith("/#") ? href.slice(2) : undefined;
}

export function SiteHeader() {
  const ref = useRef<HTMLElement>(null);

  useGSAP(
    () => {
      const mm = motionOk();
      mm.add(NO_PREFERENCE, () => {
        // Over the hero the bar has no background; past it, it earns
        // one. The background and border live on their own layer whose
        // *opacity* is animated, rather than tweening the colours
        // themselves: the colours are `hsl(var(--token) / a)`, and
        // GSAP's colour parser cannot read a `var()` inside `hsl()` —
        // it fails at tween construction, which is a page-level
        // exception rather than a silent no-op.
        //
        // The layer is also the cheaper thing to animate. Opacity is a
        // compositor property; background-color is not.
        gsap.set("[data-header-bg]", { opacity: 0 });
        // The accent: a hairline of the brand colour that draws out
        // from the middle as the bar turns solid, and retracts at the
        // top. A scale, not a width, so it never touches layout.
        gsap.set("[data-header-line]", { scaleX: 0 });

        ScrollTrigger.create({
          start: "top -80",
          end: "max",
          onToggle: (self) => {
            gsap.to("[data-header-bg]", {
              opacity: self.isActive ? 1 : 0,
              duration: 0.35,
              overwrite: true,
            });
            gsap.to("[data-header-line]", {
              scaleX: self.isActive ? 1 : 0,
              duration: self.isActive ? 0.7 : 0.35,
              overwrite: true,
            });
          },
        });

        // The bar drops in, then its links follow one by one — the eye
        // is led across the nav once, on arrival, and never again.
        gsap
          .timeline()
          .from(ref.current, { y: -60, opacity: 0, duration: 0.6 })
          .from("[data-nav] > a", { y: -8, opacity: 0, duration: 0.4, stagger: 0.04 }, "-=0.3")
          .from("[data-header-actions] > *", { y: -8, opacity: 0, duration: 0.4, stagger: 0.06 }, "<");
      });
      return () => mm.revert();
    },
    { scope: ref },
  );

  // Which section the reader is in, marked on its link. This is state,
  // not motion, so it runs for everyone; the underline it raises is a
  // CSS transition, which the reduced-motion rule already flattens.
  //
  // An IntersectionObserver rather than a ScrollTrigger per section:
  // this header mounts before the scroll story pins, so triggers made
  // here would measure the page without the pin's spacer and light up
  // the wrong link. The observer reads the layout as it actually is.
  useEffect(() => {
    const anchors = new Map<string, HTMLElement>();
    ref.current?.querySelectorAll<HTMLElement>("[data-spy]").forEach((a) => {
      const id = a.dataset.spy;
      if (id && document.getElementById(id)) anchors.set(id, a);
    });
    if (anchors.size === 0) return;

    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          anchors.get(entry.target.id)?.toggleAttribute("data-active", entry.isIntersecting);
        }
      },
      // A thin band just above the middle of the window: a section is
      // "current" while it crosses the line the eye is reading at.
      { rootMargin: "-45% 0px -54% 0px" },
    );
    anchors.forEach((_, id) => observer.observe(document.getElementById(id)!));
    return () => observer.disconnect();
  }, []);

  return (
    <header ref={ref} className="fixed inset-x-0 top-0 z-40 backdrop-blur-md">
      {/* Rendered opaque so the bar is legible before the script runs
          and when motion is reduced; GSAP fades it out at the top of the
          page and back in past the hero. */}
      <div
        data-header-bg
        aria-hidden
        className="absolute inset-0 border-b border-border/40 bg-background/70"
      />
      <div
        data-header-line
        aria-hidden
        className="absolute inset-x-0 bottom-0 h-px bg-gradient-to-r from-transparent via-primary/60 to-transparent"
      />
      <div className="container relative flex h-14 items-center justify-between">
        <Link href="/" className="flex items-center gap-2 font-semibold">
          <Logo className="size-6" />
          edytlab
        </Link>
        <nav data-nav className="hidden items-center gap-6 text-sm text-muted-foreground sm:flex">
          {links.map((l) => (
            <Link
              key={l.href}
              href={l.href}
              data-spy={sectionId(l.href)}
              // An underline that grows from the left rather than
              // appearing all at once — the same gesture as the scroll
              // progress bar at the top of the window. Hover raises it
              // briefly; being in the section holds it up.
              className="relative transition-colors after:absolute after:-bottom-1 after:left-0 after:h-px after:w-full after:origin-left after:scale-x-0 after:bg-primary after:transition-transform after:duration-300 hover:text-foreground hover:after:scale-x-100 data-[active]:text-foreground data-[active]:after:scale-x-100"
            >
              {l.label}
            </Link>
          ))}
        </nav>
        <div data-header-actions className="flex items-center gap-2">
          <Button asChild size="sm" variant="outline">
            <Link href={siteConfig.github} target="_blank" rel="noopener noreferrer">
              <Github className="size-4" />
              GitHub
            </Link>
          </Button>
          {/* The hero is a scroll-driven story now, so its download
              buttons live in the final scene. This keeps the primary
              action reachable from anywhere on the page rather than
              only at the end of the sequence. */}
          <Button asChild size="sm" className="hidden sm:inline-flex">
            <Link href={siteConfig.releases}>
              <Download className="size-4" />
              Download
            </Link>
          </Button>
        </div>
      </div>
    </header>
  );
}
