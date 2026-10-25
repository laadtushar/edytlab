/**
 * The top bar and the phone menu.
 *
 * The owner's words: "menu is too long now ... also phone has no menu".
 * The bar keeps to five links, the phone gets a menu with the same links,
 * and the footer stays the full index. These tests pin the list, that the
 * phone menu is in the HTML the server sends (so its links are reachable
 * with scripts off), and the footer's links to what the bar left out.
 */

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { Footer, homeSections } from "../components/landing/footer";
import { SiteHeader } from "../components/landing/site-header";
import { currentFor, navLinks, sectionId } from "../components/landing/site-nav";
import { siteConfig } from "./site";

const ROOT = join(fileURLToPath(new URL(".", import.meta.url)), "..");
const read = (f: string) => readFileSync(join(ROOT, f), "utf8");

/** The `<a href>` values inside the first element of `html` that matches `open`…`close`. */
function hrefsIn(html: string, open: RegExp, close: string): string[] {
  const start = html.search(open);
  expect(start, `${open} is not in the HTML`).toBeGreaterThanOrEqual(0);
  const end = html.indexOf(close, start);
  return [...html.slice(start, end).matchAll(/<a [^>]*?href="([^"]+)"/g)].map((m) => m[1]);
}

const AGREED = ["/#features", "/videos", "/docs", "/blog", "/compare"];

describe("the desktop bar", () => {
  it("keeps to the agreed five links, in order", () => {
    expect(navLinks.map((l) => l.href)).toEqual(AGREED);
    expect(navLinks.map((l) => l.label)).toEqual(["Features", "Videos", "Docs", "Blog", "Compare"]);
  });

  it("renders those five in its nav, and no link is hidden at one width and shown at another", () => {
    const html = renderToStaticMarkup(createElement(SiteHeader));
    expect(hrefsIn(html, /<nav aria-label="Main"/, "</nav>")).toEqual(AGREED);
    // `fromLg` is gone: a link that the bar drops at some width is a link
    // the phone menu and the footer have to carry, and now the list is short enough not to.
    expect(read("components/landing/site-header.tsx")).not.toContain("fromLg");
    expect(read("components/landing/site-header.tsx")).not.toMatch(/hidden lg:inline/);
  });

  it("shows its links and buttons from md, where the phone menu stops", () => {
    const html = renderToStaticMarkup(createElement(SiteHeader));
    expect(html).toMatch(/<nav aria-label="Main"[^>]*class="hidden [^"]*md:flex/);
    expect(html).toMatch(/<details[^>]*class="[^"]*md:hidden/);
  });
});

describe("the phone menu", () => {
  const html = renderToStaticMarkup(createElement(SiteHeader));
  const menu = html.slice(html.indexOf("<details"), html.indexOf("</details>") + "</details>".length);

  it("is a disclosure in the server's HTML, so it opens with scripts off", () => {
    expect(menu).toMatch(/^<details/);
    expect(menu).not.toMatch(/<details[^>]* open/);
    expect(menu).toMatch(/^<details[^>]*><summary /);
  });

  it("has a button named Menu, collapsed, that controls the panel", () => {
    const summary = menu.match(/<summary [^>]*>/)![0];
    expect(summary).toContain('aria-label="Menu"');
    expect(summary).toContain('aria-expanded="false"');
    const controls = summary.match(/aria-controls="([^"]+)"/)?.[1];
    expect(controls, "no aria-controls").toBeTruthy();
    expect(menu).toContain(`id="${controls}"`);
  });

  it("has the bar's five links in the panel, then Download and GitHub", () => {
    expect(hrefsIn(menu, /<nav aria-label="Site"/, "</nav>")).toEqual(AGREED);
    const all = [...menu.matchAll(/<a [^>]*?href="([^"]+)"/g)].map((m) => m[1]);
    expect(all).toEqual([...AGREED, siteConfig.releases, siteConfig.github]);
  });

  it("slides only for people who have not asked for less motion", () => {
    const panel = menu.match(/<div [^>]*id="[^"]+"[^>]*>/)![0];
    expect(panel).toContain("motion-safe:animate-in");
    expect(panel).not.toMatch(/(^|\s)animate-in/);
    expect(panel).not.toMatch(/(^|\s)slide-in/);
  });

  it("is not handed to the GSAP drop-in, whose transform would move the panel's anchor", () => {
    expect(read("components/landing/site-header.tsx")).toContain(":not([data-menu])");
    expect(menu).toContain("data-menu");
  });

  it("does what a disclosure lacks: Escape, a tap outside, a link, focus, and the page's scroll", () => {
    const src = read("components/landing/mobile-menu.tsx");
    expect(src).toContain('"Escape"');
    expect(src).toContain('"pointerdown"');
    expect(src).toContain("onClick={() => close(false)}");
    expect(src).toContain("summary.current?.focus()");
    expect(src).toContain('document.body.style.overflow = "hidden"');
  });

  it("does not load GSAP, so the menu costs the bundle nothing", () => {
    expect(read("components/landing/mobile-menu.tsx")).not.toMatch(/gsap/);
  });
});

describe("which link is current", () => {
  it("marks a route link on its page as the page, and under it as true", () => {
    expect(currentFor("/blog", "/blog")).toBe("page");
    expect(currentFor("/blog", "/blog/")).toBe("page");
    expect(currentFor("/blog", "/blog/some-post")).toBe("true");
    expect(currentFor("/docs", "/docs/api-reference")).toBe("true");
  });

  it("does not mark a link whose path only starts the same", () => {
    expect(currentFor("/docs", "/docs-other")).toBeUndefined();
    expect(currentFor("/videos", "/")).toBeUndefined();
    expect(currentFor("/blog", null)).toBeUndefined();
  });

  it("leaves home-page anchors to the scroll highlight", () => {
    expect(sectionId("/#features")).toBe("features");
    expect(sectionId("/videos")).toBeUndefined();
    expect(currentFor("/#features", "/")).toBeUndefined();
  });

  it("puts aria-current on the matching link only", () => {
    // The header reads the route with `usePathname`, which is null here (no router), so
    // nothing is current; the attribute is left off rather than set to "false".
    const html = renderToStaticMarkup(createElement(SiteHeader));
    expect(html).not.toContain("aria-current");
  });
});

describe("the footer, the full index", () => {
  const html = renderToStaticMarkup(createElement(Footer));
  const hrefs = new Set([...html.matchAll(/<a [^>]*?href="([^"]+)"/g)].map((m) => m[1]));

  it("links every link the bar has", () => {
    for (const l of navLinks) {
      if (sectionId(l.href)) expect(homeSections.map((s) => s.href), l.href).toContain(l.href);
      else expect(hrefs, l.href).toContain(l.href);
    }
  });

  it("links the home page's sections the bar left out", () => {
    expect(homeSections.map((s) => s.href)).toEqual(["/#demos", "/#features", "/#tools", "/#interface", "/#faq"]);
    for (const s of homeSections) expect(hrefs, s.href).toContain(s.href);
  });

  it("links every top-level page of the site", () => {
    const pages = [
      "/about",
      "/blog",
      "/changelog",
      "/compare",
      "/docs",
      "/press",
      "/privacy",
      "/terms",
      "/videos",
      "/use-cases/dj",
      "/use-cases/local-ai-audio-editor",
      "/alternatives/audacity",
      "/alternatives/adobe-audition",
    ];
    for (const p of pages) expect(hrefs, p).toContain(p);
  });
});
