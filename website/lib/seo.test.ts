/**
 * What search engines and feed readers are told, against the pages and
 * posts that exist: the sitemap lists every one, the feed carries every
 * post, a post's structured data parses and has the fields a rich
 * result needs, and each page names its own canonical URL.
 */

import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import robots from "../app/robots";
import sitemap from "../app/sitemap";
import { buildFeed, escapeXml, rfc822 } from "./feed";
import { posts, postsNewestFirst, type BlogPost } from "./blog";
import {
  absoluteUrl,
  blogPostingJsonLd,
  breadcrumbJsonLd,
  pageAlternates,
  postBreadcrumbs,
  postImagePath,
  postPath,
  serializeJsonLd,
  softwareJsonLd,
} from "./seo";
import { siteConfig } from "./site";

const ROOT = fileURLToPath(new URL("../", import.meta.url));

/** Every `page.tsx` under `app/`, as its route. Dynamic segments are skipped: the posts are listed from data. */
function staticRoutes(): string[] {
  const routes: string[] = [];
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) walk(path);
      else if (name === "page.tsx") {
        const route = "/" + relative(join(ROOT, "app"), dir).split(sep).join("/");
        if (!route.includes("[")) routes.push(route === "/" ? "/" : route.replace(/\/$/, ""));
      }
    }
  };
  walk(join(ROOT, "app"));
  return routes.sort();
}

describe("sitemap", () => {
  const entries = sitemap();
  const urls = entries.map((e) => e.url);

  it("lists every post, at its canonical URL", () => {
    for (const p of posts) expect(urls).toContain(absoluteUrl(postPath(p.slug)));
  });

  it("lists every page under app/", () => {
    for (const route of staticRoutes()) {
      expect(urls, `${route} is a page but not in the sitemap`).toContain(absoluteUrl(route));
    }
  });

  it("lists nothing twice, and nothing that is not a page or a post", () => {
    expect(new Set(urls).size).toBe(urls.length);
    const known = new Set([...staticRoutes().map(absoluteUrl), ...posts.map((p) => absoluteUrl(postPath(p.slug)))]);
    expect(urls.filter((u) => !known.has(u))).toEqual([]);
  });

  it("gives every entry a real lastModified date", () => {
    for (const e of entries) {
      expect(e.lastModified).toBeInstanceOf(Date);
      expect(Number.isNaN((e.lastModified as Date).getTime())).toBe(false);
    }
  });

  it("dates a post by when it last changed", () => {
    const post = posts[0];
    const entry = entries.find((e) => e.url === absoluteUrl(postPath(post.slug)));
    expect((entry?.lastModified as Date).toISOString().slice(0, 10)).toBe(post.dateModified ?? post.date);
  });

  it("starts every URL from the site's origin", () => {
    for (const u of urls) expect(u.startsWith(siteConfig.url)).toBe(true);
  });
});

describe("robots", () => {
  it("allows crawling and points at the sitemap", () => {
    const r = robots();
    expect(r.sitemap).toBe(absoluteUrl("/sitemap.xml"));
    expect(JSON.stringify(r.rules)).toContain('"allow":"/"');
  });
});

describe("RSS feed", () => {
  const feed = buildFeed();
  const items = feed.split("<item>").slice(1);

  it("is RSS 2.0 with a self link and one item per post", () => {
    expect(feed.startsWith('<?xml version="1.0" encoding="UTF-8"?>')).toBe(true);
    expect(feed).toContain('<rss version="2.0"');
    expect(feed).toContain(`<atom:link href="${absoluteUrl("/blog/feed.xml")}" rel="self"`);
    expect(items).toHaveLength(posts.length);
  });

  it.each(posts.map((p) => [p.slug, p] as const))("lists %s", (_slug, post) => {
    const item = items.find((i) => i.includes(`<guid isPermaLink="true">${absoluteUrl(postPath(post.slug))}</guid>`));
    expect(item, `no item for ${post.slug}`).toBeDefined();
    expect(item).toContain(`<title>${escapeXml(post.title)}</title>`);
    expect(item).toContain(`<description>${escapeXml(post.description)}</description>`);
    expect(item).toContain(`<pubDate>${rfc822(post.date)}</pubDate>`);
    expect(item).toContain("<content:encoded><![CDATA[");
  });

  it("puts the newest post first", () => {
    const newest = postsNewestFirst()[0];
    expect(items[0]).toContain(absoluteUrl(postPath(newest.slug)));
  });

  it("escapes what XML cannot hold outside CDATA", () => {
    const outside = feed.replace(/<!\[CDATA\[[\s\S]*?\]\]>/g, "");
    expect(outside).not.toMatch(/&(?!amp;|lt;|gt;|quot;|apos;)/);
    expect(outside.match(/<item>/g)?.length).toBe(outside.match(/<\/item>/g)?.length);
  });

  it("makes every link in a post absolute, because a feed is read off-site", () => {
    const html = items.join("\n");
    expect(html).not.toMatch(/href="\//);
    expect(html).toContain(`href="${absoluteUrl("/docs/tools")}"`);
  });

  it("escapes a title that would break the markup", () => {
    const hostile: BlogPost = { ...posts[0], title: "A & B <script>", slug: "hostile" };
    const out = buildFeed([hostile]);
    expect(out).toContain("<title>A &amp; B &lt;script&gt;</title>");
    expect(out).not.toContain("<script>");
  });
});

describe("structured data on a post", () => {
  it.each(posts.map((p) => [p.slug, p] as const))("%s has a BlogPosting that parses and is complete", (_slug, post) => {
    const ld = JSON.parse(serializeJsonLd(blogPostingJsonLd(post)));
    const url = absoluteUrl(postPath(post.slug));

    expect(ld["@context"]).toBe("https://schema.org");
    expect(ld["@type"]).toBe("BlogPosting");
    expect(ld.headline).toBe(post.title);
    expect(ld.description).toBe(post.description);
    expect(ld.datePublished).toBe(post.date);
    expect(ld.dateModified >= ld.datePublished).toBe(true);
    expect(ld.author).toMatchObject({ "@type": "Organization", name: "edytlab" });
    expect(ld.publisher.logo.url).toBe(absoluteUrl("/logo.png"));
    expect(ld.image).toEqual([absoluteUrl(postImagePath(post.slug))]);
    expect(ld.mainEntityOfPage).toEqual({ "@type": "WebPage", "@id": url });
    expect(ld.url).toBe(url);
    expect(ld.wordCount).toBeGreaterThan(100);
  });

  it.each(posts.map((p) => [p.slug, p] as const))("%s has a BreadcrumbList ending at the post", (_slug, post) => {
    const ld = JSON.parse(serializeJsonLd(breadcrumbJsonLd(postBreadcrumbs(post))));
    expect(ld["@type"]).toBe("BreadcrumbList");
    expect(ld.itemListElement.map((i: { position: number }) => i.position)).toEqual([1, 2, 3]);
    expect(ld.itemListElement.map((i: { item: string }) => i.item)).toEqual([
      siteConfig.url,
      absoluteUrl("/blog"),
      absoluteUrl(postPath(post.slug)),
    ]);
  });

  it("cannot be ended early by a title containing a closing script tag", () => {
    const out = serializeJsonLd(blogPostingJsonLd({ ...posts[0], title: "</script><b>" }));
    expect(out).not.toContain("</script");
    expect(JSON.parse(out).headline).toBe("</script><b>");
  });
});

describe("structured data on the home page", () => {
  it("says the app runs on macOS, Windows and Linux", () => {
    expect(softwareJsonLd().operatingSystem).toBe("macOS, Windows, Linux");
  });

  it("carries the version the page shows, without the v", () => {
    expect(softwareJsonLd("v0.3.0").softwareVersion).toBe("0.3.0");
  });

  it("leaves the version out when there is none to give", () => {
    expect("softwareVersion" in softwareJsonLd()).toBe(false);
  });

  it("claims no rating", () => {
    expect("aggregateRating" in softwareJsonLd()).toBe(false);
  });
});

describe("keywords", () => {
  it("name no feature that has not shipped", () => {
    const joined = siteConfig.keywords.join(" ").toLowerCase();
    for (const unshipped of ["stem separation", "vocal isolation", "demucs", "whisper", "transcription"]) {
      expect(joined).not.toContain(unshipped);
    }
  });
});

describe("canonical URLs", () => {
  it("makes pageAlternates name the path and the feed", () => {
    expect(pageAlternates("/docs")).toEqual({
      canonical: "/docs",
      types: { "application/rss+xml": [{ url: "/blog/feed.xml", title: "edytlab Blog" }] },
    });
  });

  it("gives every page its own canonical, not the home page's", () => {
    // A root-layout canonical is inherited by any page that sets none.
    expect(readFileSync(join(ROOT, "app/layout.tsx"), "utf8")).not.toMatch(/canonical:/);
    for (const route of staticRoutes()) {
      const dir = route === "/" ? "" : route.slice(1);
      const src = readFileSync(join(ROOT, "app", dir, "page.tsx"), "utf8");
      const m = src.match(/canonical:\s*"([^"]+)"/) ?? src.match(/pageAlternates\("([^"]+)"\)/);
      expect(m?.[1], `${route} sets no canonical`).toBe(route);
    }
  });
});

describe("social images", () => {
  // A page's `openGraph` replaces the layout's, images and all. A page
  // that sets one without an image has no og:image, so it has to name
  // the default itself (or, for a post, have a card of its own).
  it("gives every page that sets openGraph an image", () => {
    const missing: string[] = [];
    for (const route of staticRoutes()) {
      const dir = route === "/" ? "" : route.slice(1);
      const src = readFileSync(join(ROOT, "app", dir, "page.tsx"), "utf8");
      if (/openGraph:\s*\{/.test(src) && !/images:\s*\[DEFAULT_OG_IMAGE\]/.test(src)) missing.push(route);
    }
    expect(missing).toEqual([]);
  });
});

describe("one h1 per page", () => {
  // Each page's h1 lives in one place: its shell, or the page itself.
  // Naming those places is the check: a second h1 added anywhere else
  // fails here, and so does a shell that loses its own.
  const H1 = /<h1\b|as="h1"/g;
  const expected: Record<string, number> = {
    "components/docs/doc-shell.tsx": 1,
    "components/landing/legal-shell.tsx": 1,
    "components/story/scroll-story.tsx": 1,
    "app/blog/page.tsx": 1,
    "app/blog/[slug]/page.tsx": 1,
  };

  const found: Record<string, number> = {};
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      if (name === "node_modules" || name === ".next") continue;
      const path = join(dir, name);
      if (statSync(path).isDirectory()) walk(path);
      else if (/\.tsx$/.test(name)) {
        const n = readFileSync(path, "utf8").match(H1)?.length ?? 0;
        if (n > 0) found[relative(ROOT, path).split(sep).join("/")] = n;
      }
    }
  };
  walk(join(ROOT, "app"));
  walk(join(ROOT, "components"));

  it("has the h1s where they are expected and nowhere else", () => {
    expect(found).toEqual(expected);
  });
});
