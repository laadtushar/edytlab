"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useId, useRef, useState } from "react";
import { Download, Github, Menu, X } from "lucide-react";

import { currentFor, navLinks } from "@/components/landing/site-nav";
import { siteConfig } from "@/lib/site";

/**
 * The menu below `md`, where the bar has no room for its links.
 *
 * A `<details>` disclosure, not a button that toggles a hidden panel. The
 * links are in the server's HTML either way, but with a `<button>` the
 * panel can only open through script, and a phone that has not run it (or
 * never will) would have a menu button that does nothing. `<summary>` opens
 * and closes the panel natively, so the links are reachable with
 * JavaScript off, and the script only adds what a disclosure lacks:
 * Escape, a tap outside or on a link closing it, focus moving into the
 * panel and back, and the page not scrolling behind it.
 *
 * The state is the element's own `open` attribute, read on `toggle`,
 * rather than React state that drives it: the native behaviour and ours
 * cannot disagree, because there is only one of it.
 */
export function MobileMenu() {
  const pathname = usePathname();
  const panelId = useId();
  const details = useRef<HTMLDetailsElement>(null);
  const summary = useRef<HTMLElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const backdrop = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);

  const close = (returnFocus: boolean) => {
    const el = details.current;
    if (!el?.open) return;
    el.open = false;
    if (returnFocus) summary.current?.focus();
  };

  // A page change closes it, so a link followed to another route does not
  // leave the panel over the new page.
  useEffect(() => close(false), [pathname]);

  useEffect(() => {
    if (!open) return;

    panel.current?.querySelector<HTMLElement>("a")?.focus();

    const { overflow } = document.body.style;
    document.body.style.overflow = "hidden";

    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close(true);
    };
    const onPointer = (e: PointerEvent) => {
      // A tap on the backdrop closes on its click instead (below), so the
      // tap is consumed by the backdrop and cannot reach a link under it.
      if (e.target === backdrop.current) return;
      if (details.current && !details.current.contains(e.target as Node)) close(true);
    };
    document.addEventListener("keydown", onKey);
    document.addEventListener("pointerdown", onPointer);
    return () => {
      document.body.style.overflow = overflow;
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("pointerdown", onPointer);
    };
  }, [open]);

  return (
    <>
    {/* The dimmed page behind the panel. A sibling of the disclosure, not
        inside it, so it stays rendered while it fades out (a closed
        <details> drops its content at once). Absolute against the header
        rather than fixed: the header's backdrop-filter and its slide-in
        transform make it the containing block of fixed children. It hangs
        from the bottom of the bar, so the menu button stays undimmed. The
        visibility change is delayed by the fade, so a tap that closes it
        is still caught by the backdrop and not by a link underneath. */}
    <div
      ref={backdrop}
      aria-hidden
      data-menu=""
      onClick={() => close(true)}
      className={`absolute left-1/2 top-full h-[100dvh] w-screen -translate-x-1/2 bg-black/50 backdrop-blur-sm md:hidden motion-safe:transition-[opacity,visibility] motion-safe:duration-200 ${
        open ? "visible opacity-100" : "invisible opacity-0"
      }`}
    />
    <details
      ref={details}
      data-menu
      className="group md:hidden"
      onToggle={(e) => setOpen(e.currentTarget.open)}
    >
      <summary
        ref={summary}
        aria-label="Menu"
        aria-expanded={open}
        aria-controls={panelId}
        className="inline-flex size-9 cursor-pointer list-none items-center justify-center rounded-md border border-input bg-background ring-offset-background transition-colors hover:bg-accent hover:text-accent-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 [&::-webkit-details-marker]:hidden"
      >
        <Menu aria-hidden className="size-4 group-open:hidden" />
        <X aria-hidden className="hidden size-4 group-open:block" />
      </summary>
      {/* Positioned against the header (a fixed box), so it hangs from
          the bottom of the bar at the full width of the screen. It
          slides in for people who have not asked for less motion; with
          reduced motion it simply appears. */}
      <div
        ref={panel}
        id={panelId}
        className="absolute inset-x-0 top-full max-h-[calc(100dvh-3.5rem)] overflow-y-auto overscroll-contain border-b border-border bg-background shadow-lg motion-safe:animate-in motion-safe:fade-in-0 motion-safe:slide-in-from-top-2 motion-safe:duration-200"
      >
        <nav aria-label="Site" className="container flex flex-col py-2 text-base">
          {navLinks.map((l) => {
            const current = currentFor(l.href, pathname);
            return (
              <Link
                key={l.href}
                href={l.href}
                aria-current={current}
                onClick={() => close(false)}
                className="-mx-2 rounded-md px-2 py-3 text-muted-foreground transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring aria-[current]:font-medium aria-[current]:text-foreground"
              >
                {l.label}
              </Link>
            );
          })}
        </nav>
        <div className="container flex flex-col gap-2 border-t border-border/50 py-4 sm:flex-row">
          <Link
            href={siteConfig.releases}
            onClick={() => close(false)}
            className="btn-motion btn-lift inline-flex h-10 shrink-0 items-center sm:flex-1 justify-center gap-2 rounded-md bg-primary px-4 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
          >
            <Download aria-hidden className="size-4" />
            Download
          </Link>
          <Link
            href={siteConfig.github}
            target="_blank"
            rel="noopener noreferrer"
            onClick={() => close(false)}
            className="btn-motion btn-lift inline-flex h-10 shrink-0 items-center sm:flex-1 justify-center gap-2 rounded-md border border-input bg-background px-4 text-sm font-medium hover:bg-accent hover:text-accent-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
          >
            <Github aria-hidden className="size-4" />
            GitHub
          </Link>
        </div>
      </div>
    </details>
    </>
  );
}
