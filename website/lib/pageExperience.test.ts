/**
 * How a page loads and reads, checked against the files that make it.
 *
 * Lighthouse (mobile) found five things on the live site that a build
 * never shows: the hero's text was hidden by its own entrance until the
 * page hydrated, GSAP-based extras were mounted on every page ahead of
 * first paint, a `<dl>` nested its `<dt>`/`<dd>` two `div`s deep, a table
 * lost its third column to a sideways scroll on a phone, and the media in
 * `public/` was served with no cache lifetime. Each is held here so it
 * cannot come back unnoticed.
 *
 * Most checks read the source, as the rest of `lib/*.test.ts` does; the
 * two that are about markup render the component on the server, which is
 * what a crawler and the first paint get.
 */

import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

// The matcher Next compiles `headers()` sources with, so a source here
// matches a path in the test exactly when it does in the server. Next
// ships no types for it.
// @ts-expect-error TS7016: no declaration file for a module inside `next`
import { pathToRegexp } from "next/dist/compiled/path-to-regexp";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import nextConfig from "../next.config.mjs";
import { Comparison } from "../components/landing/comparison";
import { DemoCard } from "../components/landing/real-demos";
import { StatsStrip } from "../components/landing/stats-strip";
import { ScrollStory } from "../components/story/scroll-story";

const ROOT = fileURLToPath(new URL("../", import.meta.url));
const read = (path: string) => readFileSync(join(ROOT, path), "utf8");

/** Source without its comments, so a comment may say what the code must not do. */
function code(path: string): string {
  return (existsSync(join(ROOT, path)) ? read(path) : "")
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/^\s*\/\/.*$/gm, "");
}

const VOID = new Set(["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"]);

/**
 * For each `<tag>` in a string of static markup, the names of the
 * elements around it, outermost first. React writes well-formed markup,
 * so a stack is all the parsing this needs.
 */
function ancestorsOf(html: string, tag: string): string[][] {
  const stack: string[] = [];
  const found: string[][] = [];
  for (const m of html.matchAll(/<(\/?)([a-zA-Z][a-zA-Z0-9-]*)\b[^>]*?(\/?)>/g)) {
    const [, closing, name, selfClosing] = m;
    const el = name.toLowerCase();
    if (closing) {
      stack.pop();
      continue;
    }
    if (el === tag) found.push([...stack]);
    if (!selfClosing && !VOID.has(el)) stack.push(el);
  }
  return found;
}

describe("the hero paints from the server HTML", () => {
  const release = {
    version: "v0.0.0",
    macUrl: null,
    winUrl: null,
    releaseUrl: "https://github.com/laadtushar/edytlab/releases",
    isFallback: true,
  };
  const html = renderToStaticMarkup(createElement(ScrollStory, { release }));
  const story = code("components/story/scroll-story.tsx");

  it("has the headline and the subtitle in the markup, not hidden", () => {
    expect(html).toMatch(/<h1[^>]*>[\s\S]*Describe[\s\S]*Get[\s\S]*audio[\s\S]*edits\.[\s\S]*<\/h1>/);
    const sub = html.match(/<p data-s1-sub[^>]*>/)?.[0] ?? "";
    expect(sub).not.toBe("");
    expect(sub).not.toMatch(/opacity|visibility|hidden/);
  });

  it("does not start the first scene's text at opacity 0, or lift it out of its box, with a tween", () => {
    // The badge, the headline and the subtitle are `data-s1-*`. The page
    // hides whatever a `from()` or a `set()` names the moment it hydrates,
    // and Lighthouse then measures the text as painted when the tween
    // lets it back. No selector in the story may name them.
    const selectors = [...story.matchAll(/["'`]([^"'`\n]*\[data-s1-[^"'`\n]*)["'`]/g)].map((m) => m[1]);
    expect(selectors).toEqual([]);
  });

  it("lays the first scene out as the pinned stage does, so hydration does not move it", () => {
    // GSAP pins the stage at `100vh` and centres each scene in it, with
    // no padding. The server's scene one has to sit where that puts it:
    // text that is visible at first paint and then jumps is a layout shift.
    const first = html.match(/<section data-scene="1" class="([^"]*)"/)?.[1] ?? "";
    expect(first.split(" ")).toEqual(expect.arrayContaining(["md:min-h-screen", "md:pt-0", "md:pb-0"]));
  });
});

describe("the demo posters are not part of the first load", () => {
  const demo = {
    slug: "dj-mini-mix",
    title: "t",
    caption: "c",
    description: "d",
    seconds: 1,
    uploadDate: "2026-01-01",
  };
  const html = renderToStaticMarkup(createElement(DemoCard, { demo, index: 0 }));

  it("renders the video without a poster, so the server's HTML asks for no image", () => {
    expect(html).toContain("<video");
    expect(html).not.toMatch(/<video[^>]*\bposter=/);
    // The structured data names the poster (as a thumbnail URL, in a script); nothing fetches it.
    expect(html.replace(/<script[\s\S]*?<\/script>/g, "")).not.toContain("dj-mini-mix.jpg");
  });

  it("sets the poster once the video is near the viewport, and at once without IntersectionObserver", () => {
    const src = code("components/landing/lazy-poster-video.tsx");
    expect(src).toMatch(/new IntersectionObserver\(/);
    expect(src).toMatch(/rootMargin/);
    expect(src).toMatch(/typeof IntersectionObserver === "undefined"/);
    expect(src).toMatch(/poster=\{near \? poster : undefined\}/);
  });

  it("is what the demo card plays its video with", () => {
    expect(code("components/landing/real-demos.tsx")).toMatch(/<LazyPosterVideo\b/);
    expect(code("components/landing/real-demos.tsx")).not.toMatch(/<video\b/);
  });
});

describe("GSAP extras are not part of the first load", () => {
  const layout = code("app/layout.tsx");
  const deferred = code("components/motion/site-motion.tsx");

  it("keeps the progress bar and the press/hover feedback out of the root layout's imports", () => {
    const names = ["ScrollProgress", "MicroInteractions"].filter((n) => new RegExp(`\\b${n}\\b`).test(layout));
    expect(names, "the root layout still names these").toEqual([]);
    expect(/components\/motion["']/.test(layout), "the root layout imports the motion barrel").toBe(false);
    expect(/<SiteMotion\s*\/>/.test(layout), "the root layout renders <SiteMotion />").toBe(true);
  });

  it("is not re-exported from the motion barrel, which every page that animates imports", () => {
    // A re-export is a static import of the module from each importer of
    // the barrel, which is how it would get back into the first load.
    const barrel = code("components/motion/index.ts");
    expect(barrel.match(/ScrollProgress|MicroInteractions/g) ?? []).toEqual([]);
  });

  it("loads them as their own chunks, in the browser only, once the page is idle", () => {
    const lazy = (file: string) => new RegExp(`dynamic\\(\\s*\\(\\)\\s*=>\\s*import\\(["']\\./${file}["']`).test(deferred);
    expect(lazy("scroll-progress"), "scroll-progress is not a dynamic import").toBe(true);
    expect(lazy("micro-interactions"), "micro-interactions is not a dynamic import").toBe(true);
    expect(deferred.match(/ssr:\s*false/g)?.length ?? 0, "both are loaded with ssr: false").toBe(2);
    expect(/requestIdleCallback/.test(deferred), "waits for the browser to be idle").toBe(true);
    // No static import of either: that would put them back in the entry chunk.
    expect(/^import[^\n]*["']\.\/(scroll-progress|micro-interactions)["']/m.test(deferred)).toBe(false);
  });
});

describe("the home page's stats list", () => {
  const html = renderToStaticMarkup(createElement(StatsStrip));
  const ok = (parents: string[]) =>
    parents.at(-1) === "dl" || (parents.at(-1) === "div" && parents.at(-2) === "dl");

  it("has three terms and three descriptions", () => {
    expect(ancestorsOf(html, "dt")).toHaveLength(3);
    expect(ancestorsOf(html, "dd")).toHaveLength(3);
  });

  it("puts each <dt> and <dd> directly in the <dl>, or in one <div> that is (the HTML rule Lighthouse checks)", () => {
    for (const tag of ["dt", "dd"]) {
      for (const parents of ancestorsOf(html, tag)) {
        expect(ok(parents), `<${tag}> sits in ${parents.slice(-4).join(" > ")}`).toBe(true);
      }
    }
  });

  it("names the unit in the term, beside the figure, so the term reads '94 tools'", () => {
    const terms = [...html.matchAll(/<dt[^>]*>([\s\S]*?)<\/dt>/g)].map((m) => m[1].replace(/<[^>]+>/g, " ").replace(/\s+/g, " ").trim());
    expect(terms.some((t) => /^94\b.*\btools\b/.test(t))).toBe(true);
  });
});

describe("the comparison table on a phone", () => {
  const html = renderToStaticMarkup(createElement(Comparison));
  const table = html.match(/<table[\s\S]*<\/table>/)?.[0] ?? "";
  const rows = [...table.matchAll(/<tr[\s\S]*?<\/tr>/g)].map((m) => m[0]).filter((r) => r.includes("<td"));

  it("has a table with a row for each claim", () => {
    expect(table).not.toBe("");
    expect(rows.length).toBeGreaterThanOrEqual(7);
  });

  it("does not force the table wider than a phone", () => {
    expect(table.match(/\bmin-w-\[[^\]]*\]/g) ?? []).toEqual([]);
  });

  it("hides the third column below sm, in the heading and in every row", () => {
    const heads = [...table.matchAll(/<th class="([^"]*)"/g)].map((m) => m[1]);
    expect(heads).toHaveLength(3);
    expect(heads[2]).toMatch(/\bhidden\b/);
    expect(heads[2]).toMatch(/\bsm:table-cell\b/);
    for (const row of rows) {
      const cells = [...row.matchAll(/<td([^>]*)>/g)].map((m) => m[1]);
      expect(cells).toHaveLength(3);
      expect(cells[2]).toMatch(/\bhidden\b/);
      expect(cells[2]).toMatch(/\bsm:table-cell\b/);
    }
  });

  it("shows each row's detail under its label below sm, and only there", () => {
    for (const row of rows) {
      const cells = [...row.matchAll(/<td[^>]*>([\s\S]*?)<\/td>/g)].map((m) => m[1]);
      const detail = cells[2].replace(/<[^>]+>/g, "").trim();
      expect(detail.length).toBeGreaterThan(20);
      const under = cells[0].match(/<p class="([^"]*)">([^<]*)<\/p>/);
      expect(under, `no detail under the label in: ${cells[0].slice(0, 80)}`).not.toBeNull();
      expect(under?.[2]).toBe(detail);
      expect(under?.[1]).toMatch(/\bsm:hidden\b/);
    }
  });
});

describe("cache lifetime of the files in public/", () => {
  const CACHE = "public, max-age=86400, stale-while-revalidate=604800";

  /** Every file under `public/`, as the URL it is served at. */
  function publicFiles(dir = "public"): string[] {
    return readdirSync(join(ROOT, dir)).flatMap((name) => {
      const rel = `${dir}/${name}`;
      return statSync(join(ROOT, rel)).isDirectory() ? publicFiles(rel) : [rel.slice("public".length)];
    });
  }

  /** What Next does with a rule: match the path against `source`, case-insensitively. */
  async function cacheControl(path: string): Promise<string[]> {
    const rules = (await nextConfig.headers?.()) ?? [];
    return rules
      .filter((r) => (pathToRegexp(r.source, [], { sensitive: false, delimiter: "/", strict: false }) as RegExp).test(path))
      .flatMap((r) => r.headers.filter((h) => h.key.toLowerCase() === "cache-control").map((h) => h.value));
  }

  it("finds the demo videos and posters to check", () => {
    const files = publicFiles();
    expect(files.filter((f) => f.startsWith("/demos/") && f.endsWith(".mp4")).length).toBeGreaterThan(0);
    expect(files.filter((f) => f.startsWith("/demos/") && f.endsWith(".jpg")).length).toBeGreaterThan(0);
  });

  /** The media: what the rule is for. A `.txt` (the IndexNow key) or any other text file is not. */
  const MEDIA = /\.(?:png|jpe?g|gif|webp|avif|svg|ico|mp4|webm|vtt)$/i;

  it("gives every image and video in public/ a day, then a week of stale-while-revalidate", async () => {
    const media = publicFiles().filter((f) => MEDIA.test(f));
    expect(media.length).toBeGreaterThan(5);
    for (const file of media) {
      expect(await cacheControl(file), `${file} has no Cache-Control rule in next.config.mjs`).toEqual([CACHE]);
    }
  });

  it("serves a text file at the root, such as the IndexNow key, like any other file", async () => {
    const texts = publicFiles().filter((f) => f.endsWith(".txt") && f.indexOf("/", 1) === -1);
    expect(texts.length).toBeGreaterThan(0);
    for (const file of texts) expect(await cacheControl(file), file).toEqual([]);
  });

  it("covers the folders for media that will be added: /demos and /videos", async () => {
    for (const path of ["/demos/new-recording.mp4", "/demos/new-recording.vtt", "/videos/launch.mp4", "/videos/launch.jpg"]) {
      expect(await cacheControl(path), path).toEqual([CACHE]);
    }
  });

  it("does not mark them immutable: a re-recorded demo keeps its file name", async () => {
    for (const value of await Promise.all(publicFiles().filter((f) => MEDIA.test(f)).map(cacheControl))) {
      expect(value.join(" ")).not.toMatch(/immutable/);
    }
  });

  it("leaves the videos hub and each video's own page to Next, though they share a folder with the files", async () => {
    // `/videos` is the hub and `/videos/<slug>` a page for each video; only the
    // files under that folder (`.mp4`, `.jpg`, `.vtt`) are media.
    for (const path of ["/videos", "/videos/", "/videos/launch-video", "/videos/whats-new-v070", "/videos/reel-v070-200-tracks"]) {
      expect(await cacheControl(path), path).toEqual([]);
    }
    for (const path of ["/videos/launch-video.mp4", "/videos/launch-video.jpg", "/videos/launch-video.vtt"]) {
      expect(await cacheControl(path), path).toEqual([CACHE]);
    }
  });

  it("leaves pages, the app's own assets and the sitemap to Next", async () => {
    for (const path of ["/", "/docs/getting-started", "/blog/feed.xml", "/sitemap.xml", "/robots.txt", "/_next/static/chunks/a.js", "/_next/static/media/font.woff2", "/_next/static/media/hero.png", "/_next/image"]) {
      expect(await cacheControl(path), path).toEqual([]);
    }
  });
});
