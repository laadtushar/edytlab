"use client";

import Image from "next/image";
import Link from "next/link";
import { useRef } from "react";
import { Github } from "lucide-react";

import { Button } from "@/components/ui/button";
import { gsap, useGSAP, motionOk, NO_PREFERENCE, ScrollTrigger } from "@/lib/gsap";
import { siteConfig } from "@/lib/site";

const links = [
  { href: "/#features", label: "Features" },
  { href: "/#interface", label: "Interface" },
  { href: "/#tools", label: "Tools" },
  { href: "/#how-it-works", label: "How it works" },
  { href: "/blog", label: "Blog" },
  { href: "/docs", label: "Docs" },
  { href: "/#faq", label: "FAQ" },
];

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

        ScrollTrigger.create({
          start: "top -80",
          end: "max",
          onToggle: (self) => {
            gsap.to("[data-header-bg]", {
              opacity: self.isActive ? 1 : 0,
              duration: 0.35,
            });
          },
        });

        gsap.from(ref.current, { y: -60, opacity: 0, duration: 0.6, ease: "power3.out" });
      });
      return () => mm.revert();
    },
    { scope: ref },
  );

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
      <div className="container relative flex h-14 items-center justify-between">
        <Link href="/" className="group flex items-center gap-2 font-semibold">
          <Image
            src="/logo.svg"
            alt="edytlab logo"
            width={24}
            height={24}
            priority
            className="transition-transform duration-300 group-hover:rotate-12"
          />
          edytlab
        </Link>
        <nav className="hidden items-center gap-6 text-sm text-muted-foreground sm:flex">
          {links.map((l) => (
            <Link
              key={l.href}
              href={l.href}
              // An underline that grows from the left rather than
              // appearing all at once — the same gesture as the scroll
              // progress bar at the top of the window.
              className="relative transition-colors after:absolute after:-bottom-1 after:left-0 after:h-px after:w-full after:origin-left after:scale-x-0 after:bg-primary after:transition-transform after:duration-300 hover:text-foreground hover:after:scale-x-100"
            >
              {l.label}
            </Link>
          ))}
        </nav>
        <Button asChild size="sm" variant="outline">
          <Link href={siteConfig.github} target="_blank" rel="noopener noreferrer">
            <Github className="size-4" />
            GitHub
          </Link>
        </Button>
      </div>
    </header>
  );
}
