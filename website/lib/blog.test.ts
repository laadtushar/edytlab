/**
 * The blog's data against everything that hangs off it: each post has
 * a cover, a social image route, a description a search result can
 * show, and links that go somewhere. These are properties of the posts
 * as a set, so a new post is held to them without anyone adding a test.
 */

import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { BlogCover, coverSlugs, hasCover } from "../components/blog/cover";
import { MAX_DESCRIPTION, posts, wordCount, modifiedOf, type Block, type BlogPost } from "./blog";
import { octoberPosts } from "./blog-posts-october";
import { linksIn } from "./inline";
import { motifPath, seedOf } from "./og-motif";
import { siteConfig } from "./site";

const ROOT = fileURLToPath(new URL("../", import.meta.url));
const COVER_BUDGET_BYTES = 8 * 1024;

/** Every string in a post that can carry inline markup. */
function markupOf(post: BlogPost): string[] {
  return post.body.flatMap((b: Block) => {
    switch (b.type) {
      case "ul":
      case "ol":
        return b.items;
      case "prompt":
      case "h2":
      case "h3":
        return [];
      default:
        return [b.text];
    }
  });
}

function textOf(post: BlogPost): string[] {
  return post.body.flatMap((b) => (b.type === "ul" || b.type === "ol" ? b.items : [b.text]));
}

/** Every `.tsx` / `.ts` under a folder, as `[path, source]`. */
function sourcesUnder(dir: string): [string, string][] {
  const out: [string, string][] = [];
  for (const name of readdirSync(dir)) {
    if (name === "node_modules" || name === ".next") continue;
    const path = join(dir, name);
    if (statSync(path).isDirectory()) out.push(...sourcesUnder(path));
    else if (/\.tsx?$/.test(name)) out.push([path, readFileSync(path, "utf8")]);
  }
  return out;
}

describe("posts", () => {
  it("has a post to show", () => {
    expect(posts.length).toBeGreaterThan(0);
  });

  it("uses each slug once, in kebab-case", () => {
    const slugs = posts.map((p) => p.slug);
    expect(new Set(slugs).size).toBe(slugs.length);
    for (const s of slugs) expect(s).toMatch(/^[a-z0-9]+(?:-[a-z0-9]+)*$/);
  });

  it.each(posts.map((p) => [p.slug, p] as const))("%s has what search and social need", (_slug, post) => {
    expect(post.title.length).toBeGreaterThan(0);
    expect(post.title.length).toBeLessThanOrEqual(110); // schema.org's limit for a headline
    expect(post.description.length).toBeGreaterThan(40);
    expect(post.description.length).toBeLessThanOrEqual(MAX_DESCRIPTION);
    expect(post.date).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(Number.isNaN(new Date(post.date).getTime())).toBe(false);
    expect(modifiedOf(post) >= post.date).toBe(true);
    expect(post.coverAlt.length).toBeGreaterThan(20);
    expect(post.tags.length).toBeGreaterThan(0);
    expect(post.readTime).toBeGreaterThan(0);
  });

  it("gives every post a description of its own", () => {
    const d = posts.map((p) => p.description);
    expect(new Set(d).size).toBe(d.length);
  });
});

describe("covers", () => {
  it("has a cover drawn for every post, and none for a post that is gone", () => {
    for (const p of posts) expect(hasCover(p.slug), `no cover for ${p.slug}`).toBe(true);
    const slugs = new Set(posts.map((p) => p.slug));
    expect(coverSlugs.filter((s) => !slugs.has(s))).toEqual([]);
  });

  it.each(posts.map((p) => [p.slug, p] as const))("%s renders an accessible, small, themed SVG", (_slug, post) => {
    const svg = renderToStaticMarkup(createElement(BlogCover, { slug: post.slug, alt: post.coverAlt }));
    expect(svg.startsWith("<svg")).toBe(true);
    expect(svg).toContain('role="img"');
    expect(svg).toContain(`<title>${post.coverAlt}</title>`);
    expect(svg).toContain(`aria-label="${post.coverAlt}"`);
    expect(new TextEncoder().encode(svg).length).toBeLessThan(COVER_BUDGET_BYTES);
    // Themed: colour comes from the site's tokens, never a literal.
    expect(svg).toMatch(/class="[^"]*(?:stroke|fill)-primary/);
    expect(svg).not.toMatch(/#[0-9a-f]{3,8}\b/i);
    expect(svg).not.toMatch(/rgb\(|hsl\(/i);
  });

  it("draws a different cover for each post", () => {
    const drawn = posts.map((p) => renderToStaticMarkup(createElement(BlogCover, { slug: p.slug, alt: "x" })));
    expect(new Set(drawn).size).toBe(drawn.length);
  });
});

describe("social images", () => {
  const dir = join(ROOT, "app/blog/[slug]");

  it.each(["opengraph-image.tsx", "twitter-image.tsx"])("has a %s route that renders every post", (file) => {
    const path = join(dir, file);
    expect(existsSync(path), `missing app/blog/[slug]/${file}`).toBe(true);
    const src = readFileSync(path, "utf8");
    expect(src).toContain('contentType = "image/png"'); // crawlers do not draw SVG
    expect(src).toContain("size = OG_SIZE");
    expect(src).toContain("getAllSlugs");
    expect(src).toContain("renderPostOg");
  });

  it("draws the card at the size social networks expect", () => {
    const card = readFileSync(join(ROOT, "components/blog/og-card.tsx"), "utf8");
    expect(card).toContain("width: 1200, height: 630");
  });

  it("gives each post a motif of its own", () => {
    const paths = posts.map((p) => motifPath(seedOf(p.slug)));
    expect(new Set(paths).size).toBe(paths.length);
  });
});

describe("the four posts that came with v0.3.0", () => {
  it.each(octoberPosts.map((p) => [p.slug, p] as const))("%s is the length of a real post", (_slug, post) => {
    const words = wordCount(post);
    expect(words).toBeGreaterThanOrEqual(800);
    expect(words).toBeLessThanOrEqual(1400);
  });

  it.each(octoberPosts.map((p) => [p.slug, p] as const))("%s states a read time that matches its length", (_slug, post) => {
    // 220 words a minute, rounded up; a mismatch means one was edited without the other.
    expect(post.readTime).toBe(Math.ceil(wordCount(post) / 220));
  });

  it.each(octoberPosts.map((p) => [p.slug, p] as const))("%s links to the docs, a release and a prompt to try", (_slug, post) => {
    const links = markupOf(post).flatMap(linksIn);
    expect(links.some((l) => l.startsWith("/docs")), "no link into /docs").toBe(true);
    expect(links.some((l) => l === siteConfig.releases), "no link to the releases").toBe(true);
    expect(post.body.some((b) => b.type === "prompt"), "no example prompt").toBe(true);
  });

  it("points the beatmatch post at the demo videos", () => {
    const post = octoberPosts.find((p) => p.slug === "beatmatch-and-blend-two-tracks");
    expect(post && markupOf(post).flatMap(linksIn)).toContain("/#demos");
  });

  it("never describes stem separation or transcription as working", () => {
    // Neither has shipped. A mention is fine; it has to say so in the same breath.
    const unshipped = /stem separation|transcri(?:be|ption)|demucs|whisper/i;
    const admits = /not shipped|has not shipped|not available|no ability|cannot isolate|can't isolate/i;
    for (const post of octoberPosts) {
      for (const text of textOf(post)) {
        if (unshipped.test(text)) expect(text, `${post.slug}: ${text}`).toMatch(admits);
      }
    }
  });

  it("states no figure as a measurement", () => {
    // Example numbers are fine when they are labelled. These shapes are
    // how an invented benchmark reads.
    const benchmark = /\b\d+(?:\.\d+)?\s?(?:x|×)\s+(?:faster|slower|cheaper)|\b\d+%\s+(?:faster|cheaper|more|less)|per minute of audio|\bRTX\b|\bM[1-4]\b/i;
    for (const post of octoberPosts) {
      for (const text of textOf(post)) expect(text, `${post.slug}: ${text}`).not.toMatch(benchmark);
    }
  });
});

describe("links inside posts", () => {
  const appPages = sourcesUnder(join(ROOT, "app")).map(([p]) => p.replace(ROOT, "").replace(/\\/g, "/"));
  const ids = new Set(
    [...sourcesUnder(join(ROOT, "components")), ...sourcesUnder(join(ROOT, "app"))].flatMap(([, src]) =>
      [...src.matchAll(/\bid="([a-z0-9-]+)"/g)].map((m) => m[1]),
    ),
  );
  const slugs = new Set(posts.map((p) => p.slug));

  const internal = posts.flatMap((p) =>
    markupOf(p)
      .flatMap(linksIn)
      .filter((l) => l.startsWith("/"))
      .map((l) => [p.slug, l] as const),
  );

  it("has internal links to check", () => {
    expect(internal.length).toBeGreaterThan(0);
  });

  it.each(internal)("%s links to %s, which exists", (_slug, href) => {
    const [path, hash] = href.split("#");
    if (hash) expect(ids.has(hash), `no element with id="${hash}"`).toBe(true);
    if (path === "" || path === "/") return;
    if (path.startsWith("/blog/")) {
      expect(slugs.has(path.slice("/blog/".length)), `no post ${path}`).toBe(true);
    } else {
      expect(appPages, `no page for ${path}`).toContain(`app${path}/page.tsx`);
    }
  });
});
