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
  DEFAULT_OG_IMAGE,
  ORGANIZATION_ID,
  PAGE_UPDATED,
  SITE_UPDATED,
  SOFTWARE_ID,
  WEBSITE_ID,
  absoluteUrl,
  articleJsonLd,
  blogPostingJsonLd,
  breadcrumbJsonLd,
  compareBreadcrumbs,
  compareHubJsonLd,
  isoDuration,
  organizationJsonLd,
  pageAlternates,
  pageBreadcrumbs,
  pageSocial,
  pageUpdated,
  postBreadcrumbs,
  postImagePath,
  postPath,
  serializeJsonLd,
  softwareJsonLd,
  videoBreadcrumbs,
  videoObjectJsonLd,
  videosHubJsonLd,
  websiteJsonLd,
} from "./seo";
import { siteConfig } from "./site";
import { demoVideos, getVideo, hubVideos, videoPath, videoPosterSrc, videoSrc, watchVideos } from "./videos";

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

  it("lists every video's own page, at its canonical URL", () => {
    for (const v of watchVideos) expect(urls).toContain(absoluteUrl(videoPath(v.slug)));
  });

  it("lists every page under app/", () => {
    for (const route of staticRoutes()) {
      expect(urls, `${route} is a page but not in the sitemap`).toContain(absoluteUrl(route));
    }
  });

  it("lists nothing twice, and nothing that is not a page or a post", () => {
    expect(new Set(urls).size).toBe(urls.length);
    const known = new Set([
      ...staticRoutes().map(absoluteUrl),
      ...posts.map((p) => absoluteUrl(postPath(p.slug))),
      ...watchVideos.map((v) => absoluteUrl(videoPath(v.slug))),
    ]);
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

describe("IndexNow", () => {
  // IndexNow engines (Bing and others) check that the site serves the key
  // it was told about, at the root, and compare it byte for byte. The key
  // is public by design; it proves control of the site, it is not a secret.
  const KEY = "2da9005c7a104cf98929005b2c4c4beb";

  it("serves its key file at the root, with the key and nothing else", () => {
    expect(readFileSync(join(ROOT, "public", `${KEY}.txt`), "utf8")).toBe(KEY);
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
  // What the page renders: the block as `serializeJsonLd` writes it into
  // the `<script>`, read back the way a crawler reads it.
  const ld = JSON.parse(serializeJsonLd(softwareJsonLd("v0.4.0")));

  it("parses as a schema.org SoftwareApplication", () => {
    expect(ld["@context"]).toBe("https://schema.org");
    expect(ld["@type"]).toBe("SoftwareApplication");
    expect(ld.name).toBe("edytlab");
    expect(ld.applicationCategory).toBe("MultimediaApplication");
  });

  it("names the canonical URL, the home page's own", () => {
    expect(ld.url).toBe("https://www.edytlab.com");
    expect(ld.url).toBe(absoluteUrl("/"));
    // The canonical `pageAlternates("/")` resolves to against `metadataBase`.
    expect(new URL(pageAlternates("/").canonical, siteConfig.url).origin).toBe(new URL(ld.url).origin);
  });

  it("is free and MIT-licensed, with the releases page to download from", () => {
    expect(ld.offers).toEqual({ "@type": "Offer", price: "0", priceCurrency: "USD" });
    expect(ld.license).toBe("https://opensource.org/licenses/MIT");
    expect(ld.downloadUrl).toBe("https://github.com/laadtushar/edytlab/releases");
  });

  it("is published by LabyNator and authored by Tushar Laad", () => {
    expect(ld.publisher).toEqual({
      "@type": "Organization",
      name: "LabyNator",
      url: "https://www.labynator.com",
    });
    expect(ld.author).toEqual({
      "@type": "Person",
      name: "Tushar Laad",
      url: "https://www.tusharlaad.com",
    });
  });

  it("takes its version from the release the page shows, not from a copy", () => {
    expect(ld.softwareVersion).toBe("0.4.0");
    const home = readFileSync(join(ROOT, "app/page.tsx"), "utf8");
    // The same `getLatestRelease()` call feeds the badge, the buttons and this block.
    expect(home).toContain("const release = await getLatestRelease()");
    expect(home).toMatch(/softwareJsonLd\(release\.isFallback \? undefined : release\.version\)/);
    expect(readFileSync(join(ROOT, "lib/seo-core.ts"), "utf8")).not.toMatch(/softwareVersion:\s*["'`]/);
  });

  it("says the app runs on macOS, Windows and Linux", () => {
    expect(softwareJsonLd().operatingSystem).toBe("macOS, Windows, Linux");
  });

  it("carries the version the page shows, without the v", () => {
    expect(softwareJsonLd("v0.3.0").softwareVersion).toBe("0.3.0");
  });

  it("leaves the version out when there is none to give", () => {
    expect("softwareVersion" in softwareJsonLd()).toBe(false);
  });

  it("claims no rating, review or user count", () => {
    const keys = Object.keys(softwareJsonLd("v0.4.0"));
    for (const claim of ["aggregateRating", "review", "interactionStatistic"]) {
      expect(keys).not.toContain(claim);
    }
  });
});

describe("the canonical host", () => {
  // The apex redirects to www, so www is the URL that serves the page;
  // a canonical naming the apex points search engines at a redirect.
  const sources = (dir: string): string[] =>
    readdirSync(join(ROOT, dir)).flatMap((name) => {
      const rel = join(dir, name);
      if (statSync(join(ROOT, rel)).isDirectory()) return sources(rel);
      return /\.tsx?$/.test(name) && !/\.test\.tsx?$/.test(name) ? [rel] : [];
    });

  it("is www.edytlab.com, and metadataBase is built from it", () => {
    expect(siteConfig.url).toBe("https://www.edytlab.com");
    expect(readFileSync(join(ROOT, "app/layout.tsx"), "utf8")).toContain("metadataBase: new URL(siteConfig.url)");
  });

  it("starts the sitemap, the robots sitemap line and every page URL from it", () => {
    expect(robots().sitemap).toBe("https://www.edytlab.com/sitemap.xml");
    for (const e of sitemap()) expect(e.url.startsWith("https://www.edytlab.com")).toBe(true);
  });

  it("is never spelled as the apex in the site's own code", () => {
    const apex = /https?:\/\/edytlab\.com/;
    const offenders = [...sources("app"), ...sources("components"), ...sources("lib")].filter((f) =>
      apex.test(readFileSync(join(ROOT, f), "utf8")),
    );
    expect(offenders).toEqual([]);
  });
});

describe("keywords", () => {
  it("name no feature that has not shipped", () => {
    const joined = siteConfig.keywords.join(" ").toLowerCase();
    for (const unshipped of ["stem separation", "vocal isolation", "demucs"]) {
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

describe("og:url", () => {
  // A page that sets no `openGraph` inherits the layout's, whose url is
  // the home page's. So every page but the home page has to name its own.
  it("matches each page's canonical", () => {
    const wrong: string[] = [];
    for (const route of staticRoutes()) {
      if (route === "/") continue;
      const src = readFileSync(join(ROOT, "app", route.slice(1), "page.tsx"), "utf8");
      const named = [
        ...[...src.matchAll(/\$\{siteConfig\.url\}(\/[^`$]*)`/g)].map((m) => m[1]),
        ...[...src.matchAll(/(?:absoluteUrl|pageSocial)\(\s*"([^"]+)"/g)].map((m) => m[1]),
      ];
      if (!named.includes(route)) wrong.push(route);
    }
    expect(wrong, "pages whose og:url is not their own address").toEqual([]);
  });

  it("is the home page's own on the layout, which the home page inherits", () => {
    expect(readFileSync(join(ROOT, "app/layout.tsx"), "utf8")).toMatch(/openGraph:\s*\{[^}]*url: siteConfig\.url/);
  });

  it("is built by pageSocial from the page's path, with the card and image a share needs", () => {
    const { openGraph, twitter } = pageSocial("/changelog", "Changelog · edytlab", "Release notes.");
    expect(openGraph.url).toBe("https://www.edytlab.com/changelog");
    expect(openGraph).toMatchObject({ type: "website", siteName: "edytlab", title: "Changelog · edytlab" });
    expect(openGraph.images).toEqual([DEFAULT_OG_IMAGE]);
    expect(twitter.card).toBe("summary_large_image");
    expect(twitter.images).toEqual([DEFAULT_OG_IMAGE.url]);
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
    "components/landing/page-shell.tsx": 1,
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


/** What the browser reads back out of a block, as `serializeJsonLd` wrote it. */
const parsed = (data: unknown) => JSON.parse(serializeJsonLd(data));

/** Every key, and every `@type`, anywhere in a parsed JSON-LD value. */
function walk(value: unknown, keys: string[] = [], types: string[] = []): { keys: string[]; types: string[] } {
  if (Array.isArray(value)) value.forEach((v) => walk(v, keys, types));
  else if (value && typeof value === "object") {
    for (const [k, v] of Object.entries(value)) {
      keys.push(k);
      if (k === "@type") types.push(...[v].flat().map(String));
      walk(v, keys, types);
    }
  }
  return { keys, types };
}

/** Markup that claims what the site cannot back: ratings, reviews, counts of users. */
const SOCIAL_PROOF_KEYS = [
  "aggregateRating",
  "review",
  "reviews",
  "reviewRating",
  "ratingValue",
  "ratingCount",
  "reviewCount",
  "bestRating",
  "interactionStatistic",
  "userInteractionCount",
];
const SOCIAL_PROOF_TYPES = ["Review", "AggregateRating", "Rating", "Product", "InteractionCounter"];

function expectNoSocialProof(data: unknown, label: string) {
  const { keys, types } = walk(parsed(data));
  for (const k of SOCIAL_PROOF_KEYS) expect(keys, `${label} has a "${k}"`).not.toContain(k);
  for (const t of SOCIAL_PROOF_TYPES) expect(types, `${label} has a ${t}`).not.toContain(t);
}

/** The pixel size of a PNG, read from its header. */
function pngSize(path: string): { width: number; height: number } {
  const head = readFileSync(path);
  expect(head.subarray(1, 4).toString("latin1"), `${path} is not a PNG`).toBe("PNG");
  return { width: head.readUInt32BE(16), height: head.readUInt32BE(20) };
}

describe("sitemap dates", () => {
  const entries = sitemap();
  const dateOf = (route: string) =>
    (entries.find((e) => e.url === absoluteUrl(route))?.lastModified as Date).toISOString().slice(0, 10);
  const routes = [...staticRoutes(), ...watchVideos.map((v) => videoPath(v.slug))];

  it("gives every page, but the blog index, a date of its own", () => {
    // One date on every page cannot be verified for any of them, and
    // Google uses <lastmod> only when it is. A new page has to be dated.
    for (const route of routes.filter((r) => r !== "/blog")) {
      expect(PAGE_UPDATED[route], `${route} has no date in PAGE_UPDATED (lib/seo-core.ts)`).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    }
  });

  it("writes each page's own date into the sitemap", () => {
    for (const route of routes.filter((r) => r !== "/blog")) expect(dateOf(route)).toBe(PAGE_UPDATED[route]);
  });

  it("does not give every page the same date", () => {
    const dates = new Set(routes.filter((r) => r !== "/blog").map(dateOf));
    // On 2026-10-25 every page changed in substance (the site-wide tightening,
    // #744–#748), so one shared date was true that day. The exception ends by
    // itself: the next page that changes moves its date past it.
    if (dates.size === 1 && dates.has("2026-10-25")) return;
    expect(dates.size).toBeGreaterThan(1);
  });

  it("dates the blog index by its newest post", () => {
    const newest = posts.map((p) => p.dateModified ?? p.date).sort().at(-1);
    expect(dateOf("/blog")).toBe(newest);
  });

  it("lists no date for a page that does not exist, or one in the future", () => {
    expect(Object.keys(PAGE_UPDATED).filter((p) => !routes.includes(p))).toEqual([]);
    for (const [route, date] of Object.entries(PAGE_UPDATED)) {
      expect(date <= SITE_UPDATED, `${route}: ${date} is after the site-wide date ${SITE_UPDATED}`).toBe(true);
    }
  });

  it("uses the site-wide date only for a path with no date of its own", () => {
    expect(pageUpdated("/not-a-page")).toBe(SITE_UPDATED);
    expect(pageUpdated("/privacy")).toBe(PAGE_UPDATED["/privacy"]);
  });
});

describe("structured data on the use-case pages", () => {
  const useCases = [
    { path: "/use-cases/dj", name: "AI audio editor for DJs" },
    { path: "/use-cases/local-ai-audio-editor", name: "A local-first AI audio editor" },
  ];

  it("covers every use-case page there is", () => {
    expect(staticRoutes().filter((r) => r.startsWith("/use-cases/"))).toEqual(useCases.map((u) => u.path).sort());
  });

  describe.each(useCases)("$path", ({ path, name }) => {
    const src = readFileSync(join(ROOT, "app", path.slice(1), "page.tsx"), "utf8");
    const title = src.match(/const title = "([^"]+)"/)?.[1] ?? "";
    const description = src.match(/const description =\s*"([^"]+)"/)?.[1] ?? "";
    const published = src.match(/const published = "(\d{4}-\d{2}-\d{2})"/)?.[1] ?? "";

    it("renders a breadcrumb trail and an article, in the page itself", () => {
      expect(src).toContain("<JsonLd");
      expect(src).toContain("breadcrumbJsonLd(pageBreadcrumbs(");
      expect(src).toContain("articleJsonLd({ path");
      expect(title.length).toBeGreaterThan(0);
      expect(description.length).toBeGreaterThan(0);
      expect(published, "the page names no `published` date").toMatch(/^\d{4}-\d{2}-\d{2}$/);
    });

    it("has a BreadcrumbList from the home page to itself", () => {
      const ld = parsed(breadcrumbJsonLd(pageBreadcrumbs(name, path)));
      expect(ld["@type"]).toBe("BreadcrumbList");
      expect(ld.itemListElement.length).toBeGreaterThanOrEqual(2);
      expect(ld.itemListElement.map((i: { position: number }) => i.position)).toEqual([1, 2]);
      expect(ld.itemListElement.map((i: { item: string }) => i.item)).toEqual([siteConfig.url, absoluteUrl(path)]);
      expect(ld.itemListElement[0].name).toBe("Home");
      expect(ld.itemListElement[1].name).toBe(name);
    });

    it("has an Article by the founder, dated like the sitemap", () => {
      const ld = parsed(articleJsonLd({ path, headline: title, description, datePublished: published }));
      expect(ld["@context"]).toBe("https://schema.org");
      expect(ld["@type"]).toBe("Article");
      expect(ld.headline).toBe(title);
      expect(ld.headline.length).toBeLessThanOrEqual(110); // schema.org's limit for a headline
      expect(ld.description).toBe(description);
      expect(ld.author).toEqual({ "@type": "Person", name: "Tushar Laad", url: "https://www.tusharlaad.com" });
      expect(ld.publisher).toMatchObject({ "@type": "Organization", "@id": ORGANIZATION_ID, name: "edytlab" });
      expect(ld.publisher.logo.url).toBe(absoluteUrl("/logo.png"));
      expect(ld.datePublished).toBe(published);
      expect(ld.dateModified).toBe(PAGE_UPDATED[path]);
      expect(ld.dateModified >= ld.datePublished).toBe(true);
      expect(ld.image).toEqual([absoluteUrl(DEFAULT_OG_IMAGE.url)]);
      expect(ld.url).toBe(absoluteUrl(path));
      expect(ld.mainEntityOfPage).toEqual({ "@type": "WebPage", "@id": absoluteUrl(path) });
    });

    it("claims no rating or review", () => {
      expectNoSocialProof(articleJsonLd({ path, headline: title, description, datePublished: published }), "Article");
      expectNoSocialProof(breadcrumbJsonLd(pageBreadcrumbs(name, path)), "BreadcrumbList");
    });
  });
});

describe("structured data on the comparison pages", () => {
  const alternatives = [
    { path: "/alternatives/audacity", name: "Audacity alternative" },
    { path: "/alternatives/adobe-audition", name: "Adobe Audition alternative" },
  ];

  it("covers every comparison page there is", () => {
    expect(staticRoutes().filter((r) => r.startsWith("/alternatives/"))).toEqual(alternatives.map((a) => a.path).sort());
    expect(staticRoutes()).toContain("/compare");
  });

  it.each(alternatives)("$path is dated in the sitemap and in PAGE_UPDATED, and is listed", ({ path }) => {
    expect(PAGE_UPDATED[path]).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    const entry = sitemap().find((e) => e.url === absoluteUrl(path));
    expect((entry?.lastModified as Date).toISOString().slice(0, 10)).toBe(PAGE_UPDATED[path]);
  });

  it.each(alternatives)("$path has a trail Home, Compare, the page", ({ path, name }) => {
    const src = readFileSync(join(ROOT, "app", path.slice(1), "page.tsx"), "utf8");
    expect(src).toContain("breadcrumbJsonLd(compareBreadcrumbs(");
    const ld = parsed(breadcrumbJsonLd(compareBreadcrumbs(name, path)));
    expect(ld.itemListElement.map((i: { position: number }) => i.position)).toEqual([1, 2, 3]);
    expect(ld.itemListElement.map((i: { item: string }) => i.item)).toEqual([siteConfig.url, absoluteUrl("/compare"), absoluteUrl(path)]);
    expect(ld.itemListElement.map((i: { name: string }) => i.name)).toEqual(["Home", "Compare", name]);
  });

  it("describes the hub as a CollectionPage whose ItemList points at its own sections", () => {
    const ld = parsed(
      compareHubJsonLd({
        path: "/compare",
        name: "Hub",
        description: "A hub.",
        items: [
          { name: "One", anchor: "one" },
          { name: "Two", anchor: "two" },
        ],
      }),
    );
    expect(ld["@type"]).toBe("CollectionPage");
    expect(ld.url).toBe(absoluteUrl("/compare"));
    expect(ld.dateModified).toBe(PAGE_UPDATED["/compare"]);
    expect(ld.isPartOf).toEqual({ "@id": WEBSITE_ID });
    expect(ld.mainEntity["@type"]).toBe("ItemList");
    expect(ld.mainEntity.numberOfItems).toBe(2);
    expect(ld.mainEntity.itemListElement.map((i: { position: number; url: string }) => [i.position, i.url])).toEqual([
      [1, `${absoluteUrl("/compare")}#one`],
      [2, `${absoluteUrl("/compare")}#two`],
    ]);
    expectNoSocialProof(ld, "CollectionPage");
  });
});

describe("a BreadcrumbList on every page below the home page", () => {
  // Breadcrumbs let Google show the page's place in the site under its
  // result. A page with no trail shows a bare URL, and which pages have
  // one was a matter of who remembered: the use-case pages, the blog and
  // the docs did, the changelog, the press kit and the legal pages did
  // not. A page is covered when its own source writes the trail, or when
  // it uses a shell that writes it from the page's path.
  const read = (path: string) => readFileSync(join(ROOT, path), "utf8");
  const shells: Record<string, string> = {
    DocShell: read("components/docs/doc-shell.tsx"),
    LegalShell: read("components/landing/legal-shell.tsx"),
  };
  const routes = staticRoutes().filter((r) => r !== "/");

  it("has pages to check", () => {
    expect(routes.length).toBeGreaterThanOrEqual(12);
    expect(routes).toEqual(expect.arrayContaining(["/changelog", "/press", "/privacy", "/terms", "/blog", "/docs"]));
  });

  it.each(routes)("%s writes its trail, in its own source or through a shell that does", (route) => {
    const src = read(join("app", route.slice(1), "page.tsx"));
    const own = /breadcrumbJsonLd\(/.test(src);
    const shell = Object.entries(shells).find(([name, shellSrc]) => src.includes(`<${name}`) && /breadcrumbJsonLd\(/.test(shellSrc));
    expect(own || shell !== undefined, `${route} renders no BreadcrumbList`).toBe(true);
  });

  it("gives the legal pages' shell the page's own path, which is where the trail ends", () => {
    for (const route of ["/changelog", "/privacy", "/terms"]) {
      const src = read(join("app", route.slice(1), "page.tsx"));
      expect(src, `${route} does not pass its path to LegalShell`).toMatch(new RegExp(`<LegalShell[^>]*\\bpath="${route}"`));
    }
  });

  it("names the press kit's trail Home, then Press kit, at /press", () => {
    const src = read("app/press/page.tsx");
    expect(src).toContain('breadcrumbJsonLd(pageBreadcrumbs("Press kit", "/press"))');
    const ld = parsed(breadcrumbJsonLd(pageBreadcrumbs("Press kit", "/press")));
    expect(ld.itemListElement.map((i: { item: string }) => i.item)).toEqual([siteConfig.url, absoluteUrl("/press")]);
  });

  it("ends each trail at the page's canonical URL, with Home first", () => {
    for (const [name, route] of [
      ["Changelog", "/changelog"],
      ["Privacy", "/privacy"],
      ["Terms of use", "/terms"],
      ["Press kit", "/press"],
    ] as const) {
      const ld = parsed(breadcrumbJsonLd(pageBreadcrumbs(name, route)));
      expect(ld["@type"]).toBe("BreadcrumbList");
      expect(ld.itemListElement.map((i: { position: number }) => i.position)).toEqual([1, 2]);
      expect(ld.itemListElement[0]).toMatchObject({ name: "Home", item: siteConfig.url });
      expect(ld.itemListElement[1]).toMatchObject({ name, item: absoluteUrl(route) });
    }
  });
});

describe("the three full demos", () => {
  it.each(demoVideos.map((d) => [d.slug, d] as const))("%s has a complete VideoObject, on its own page", (_slug, demo) => {
    const ld = parsed(videoObjectJsonLd(demo, { video: videoSrc(demo.slug), poster: videoPosterSrc(demo.slug) }));
    expect(ld["@context"]).toBe("https://schema.org");
    expect(ld["@type"]).toBe("VideoObject");
    expect(ld.name).toBe(demo.title);
    expect(ld.description).toBe(demo.description);
    expect(ld.description.length).toBeGreaterThan(40);
    // The poster the player shows, and the file it plays, both absolute, in the demos' own folder.
    expect(ld.thumbnailUrl).toEqual([`https://www.edytlab.com/demos/${demo.slug}.jpg`]);
    expect(ld.contentUrl).toBe(`https://www.edytlab.com/demos/${demo.slug}.mp4`);
    // A date and time with a timezone, which Google's video report asks for.
    expect(ld.uploadDate).toMatch(/^\d{4}-\d{2}-\d{2}T00:00:00Z$/);
    expect(ld.uploadDate).toBe(`${demo.uploadDate}T00:00:00Z`);
    expect(ld.duration).toMatch(/^PT(?:\d+H)?(?:\d+M)?\d+S$/);
    expect(ld.duration).toBe(isoDuration(demo.seconds));
  });

  it("writes a length as an ISO 8601 duration", () => {
    expect(isoDuration(165)).toBe("PT2M45S");
    expect(isoDuration(115)).toBe("PT1M55S");
    expect(isoDuration(45)).toBe("PT45S");
    expect(isoDuration(3725)).toBe("PT1H2M5S");
  });

  it("still play on the home page and the DJ page, which carry no structured data for them", () => {
    // Google indexes a video only on a page where it is the main content, so a demo's
    // VideoObject is on its own page (`/videos/<slug>`) and on no other.
    const card = readFileSync(join(ROOT, "components/landing/real-demos.tsx"), "utf8");
    const code = card.replace(/\/\*[\s\S]*?\*\//g, "").replace(/^\s*\/\/.*$/gm, "");
    expect(card).toContain("export function DemoCard");
    expect(code).not.toMatch(/JsonLd|videoObjectJsonLd|VideoObject/);
    expect(readFileSync(join(ROOT, "app/page.tsx"), "utf8")).toContain("<RealDemos />");
    expect(readFileSync(join(ROOT, "app/use-cases/dj/page.tsx"), "utf8")).toContain("<DemoCard");
  });

  it("each link to their own page from the card", () => {
    const card = readFileSync(join(ROOT, "components/landing/real-demos.tsx"), "utf8");
    expect(card).toContain("href={videoPath(demo.slug)}");
    expect(card).toContain("Watch on its own page");
  });
});

describe("structured data for the organization and the site", () => {
  const org = parsed(organizationJsonLd());
  const site = parsed(websiteJsonLd());
  const software = parsed(softwareJsonLd("v0.7.0"));

  it("describes the organization, with a mark big enough to use", () => {
    expect(org["@context"]).toBe("https://schema.org");
    expect(org["@type"]).toBe("Organization");
    expect(org.name).toBe("edytlab");
    expect(org.url).toBe(siteConfig.url);
    expect(org.sameAs).toContain(siteConfig.github);
    expect(org.logo.url).toBe(absoluteUrl("/logo.png"));
    // Google asks for at least 112 by 112 pixels.
    const { width, height } = pngSize(join(ROOT, "public/logo.png"));
    expect(width).toBeGreaterThanOrEqual(112);
    expect(height).toBeGreaterThanOrEqual(112);
  });

  it("names the site, so the site name is the brand", () => {
    expect(site["@type"]).toBe("WebSite");
    expect(site.name).toBe("edytlab");
    expect(site.url).toBe(siteConfig.url);
    expect(site.publisher).toEqual({ "@id": ORGANIZATION_ID });
  });

  it("gives the three things stable ids of their own", () => {
    expect(org["@id"]).toBe(ORGANIZATION_ID);
    expect(site["@id"]).toBe(WEBSITE_ID);
    expect(software["@id"]).toBe(SOFTWARE_ID);
    expect(new Set([ORGANIZATION_ID, WEBSITE_ID, SOFTWARE_ID]).size).toBe(3);
    for (const id of [ORGANIZATION_ID, WEBSITE_ID, SOFTWARE_ID]) expect(id.startsWith(`${siteConfig.url}/#`)).toBe(true);
  });

  it("keeps the app's id the same from one release to the next", () => {
    expect(parsed(softwareJsonLd("v0.8.0"))["@id"]).toBe(parsed(softwareJsonLd("v0.7.0"))["@id"]);
    expect(parsed(softwareJsonLd())["@id"]).toBe(SOFTWARE_ID);
  });

  it("is rendered on the home page", () => {
    const home = readFileSync(join(ROOT, "app/page.tsx"), "utf8");
    for (const call of ["organizationJsonLd()", "websiteJsonLd()", "jsonLd"]) {
      expect(home).toContain(`<JsonLd data={${call}} />`);
    }
  });

  it("claims no rating, review or count", () => {
    expectNoSocialProof(organizationJsonLd(), "Organization");
    expectNoSocialProof(websiteJsonLd(), "WebSite");
    expectNoSocialProof(softwareJsonLd("v0.7.0"), "SoftwareApplication");
  });
});

describe("no rating or review markup anywhere", () => {
  // The site has no ratings, reviews or user counts to report, and
  // markup that claims them is spam in Google's eyes. A builder is
  // checked where it is tested; this reads the source, so one added
  // later (a comparison page, say) fails here without anyone adding a test.
  const files: string[] = [];
  const collect = (dir: string) => {
    for (const name of readdirSync(join(ROOT, dir))) {
      const rel = join(dir, name);
      if (statSync(join(ROOT, rel)).isDirectory()) collect(rel);
      else if (/\.tsx?$/.test(name) && !/\.test\.tsx?$/.test(name)) files.push(rel);
    }
  };
  ["app", "components", "lib"].forEach(collect);

  it("reads some source", () => {
    expect(files.length).toBeGreaterThan(20);
  });

  it("names no rating or review property, and no Review, Rating or Product type", () => {
    const keys = new RegExp(`\\b(?:${SOCIAL_PROOF_KEYS.filter((k) => k !== "review" && k !== "reviews").join("|")})\\b`);
    const types = new RegExp(`["']?@type["']?\\s*:\\s*["'](?:${SOCIAL_PROOF_TYPES.join("|")})["']`);
    const offenders = files.filter((f) => {
      // Comments may say what is left out; code may not say it.
      const code = readFileSync(join(ROOT, f), "utf8")
        .replace(/\/\*[\s\S]*?\*\//g, "")
        .replace(/^\s*\/\/.*$/gm, "");
      return keys.test(code) || types.test(code);
    });
    expect(offenders).toEqual([]);
  });
});

describe("the home page's title and headline", () => {
  const layout = readFileSync(join(ROOT, "app/layout.tsx"), "utf8");
  const home = readFileSync(join(ROOT, "app/page.tsx"), "utf8");

  it("says what the product is, in the words people search with", () => {
    expect(siteConfig.homeTitle).toBe("edytlab: free, open-source AI audio editor");
    expect(siteConfig.homeTitle.length).toBeLessThanOrEqual(60);
  });

  it("is the layout's default, which the template does not wrap", () => {
    // `title.default` reaches the browser as written; the template is
    // for the pages that set a title, so the brand is not said twice.
    expect(layout).toMatch(/title:\s*\{\s*default: siteConfig\.homeTitle,\s*template: "%s · edytlab",?\s*\}/);
    expect(siteConfig.homeTitle).not.toContain("·");
    expect(home).not.toMatch(/\btitle\s*:/);
  });

  it("leaves the tagline as the visible headline and the social card's title", () => {
    expect(layout).toContain("title: siteConfig.title,");
    const story = readFileSync(join(ROOT, "components/story/scroll-story.tsx"), "utf8");
    expect(story).toMatch(/<h1[\s\S]*Describe it\.[\s\S]*Get pro-grade audio edits\./);
  });

  it("says free and open source in the hero's eyebrow, and keeps local-first", () => {
    const story = readFileSync(join(ROOT, "components/story/scroll-story.tsx"), "utf8");
    const badge = story.match(/<Badge[\s\S]*?<\/Badge>/)?.[0] ?? "";
    expect(badge).toContain("Free, open-source, local-first AI audio editor");
  });

  it("separates the version from the eyebrow with a space that survives", () => {
    // The badge is a flex container, so a plain space at the start of its
    // second item is dropped ("editor· v0.7.0"). A non-breaking space is not.
    const story = readFileSync(join(ROOT, "components/story/scroll-story.tsx"), "utf8");
    const badge = story.match(/<Badge[\s\S]*?<\/Badge>/)?.[0] ?? "";
    expect(badge).toContain("{`\\u00a0· ${release.version}`}");
    expect(badge).not.toContain("{` · ${release.version}`}");
  });
});

describe("internal links to the use-case pages", () => {
  const useCases = staticRoutes().filter((r) => r.startsWith("/use-cases/"));
  const sections = (dir: string, except: string[]) =>
    readdirSync(join(ROOT, dir))
      .filter((f) => f.endsWith(".tsx") && !except.includes(f))
      .map((f) => readFileSync(join(ROOT, dir, f), "utf8"));
  const linked = (src: string, route: string) => new RegExp(`["']${route}["']`).test(src);

  it("has use-case pages to link to", () => {
    expect(useCases.length).toBeGreaterThanOrEqual(2);
  });

  it.each(useCases)("links %s from the footer, which every page has", (route) => {
    expect(linked(readFileSync(join(ROOT, "components/landing/footer.tsx"), "utf8"), route)).toBe(true);
  });

  it.each(useCases)("links %s from the body of the home page, not only its footer", (route) => {
    const body = sections("components/landing", ["footer.tsx", "site-header.tsx", "page-shell.tsx", "legal-shell.tsx"]);
    expect(body.some((src) => linked(src, route)), `no home section links ${route}`).toBe(true);
  });
});


describe("the videos hub", () => {
  const page = readFileSync(join(ROOT, "app/videos/page.tsx"), "utf8");
  const cards = readFileSync(join(ROOT, "components/landing/site-video.tsx"), "utf8");
  const entry = sitemap().find((e) => e.url === absoluteUrl("/videos"));

  it("is a page, in the sitemap, with a date of its own", () => {
    expect(staticRoutes()).toContain("/videos");
    expect(entry, "/videos is not in the sitemap").toBeDefined();
    expect((entry?.lastModified as Date).toISOString().slice(0, 10)).toBe(PAGE_UPDATED["/videos"]);
  });

  it("has a BreadcrumbList from the home page to itself", () => {
    expect(page).toContain('breadcrumbJsonLd(pageBreadcrumbs("Videos", path))');
    expect(page).toContain('const path = "/videos"');
    const ld = parsed(breadcrumbJsonLd(pageBreadcrumbs("Videos", "/videos")));
    expect(ld.itemListElement.map((i: { item: string }) => i.item)).toEqual([siteConfig.url, absoluteUrl("/videos")]);
    expect(ld.itemListElement[1].name).toBe("Videos");
  });

  it("sets its canonical, social card and title with the site's helpers", () => {
    expect(page).toContain('alternates: pageAlternates("/videos")');
    expect(page).toContain('...pageSocial("/videos"');
    expect(page).toMatch(/const description =\s*"[^"]{60,160}"/);
  });

  it("features the launch video, then lists the three demos and the clips, and no release's video", () => {
    const at = (needle: string) => page.indexOf(needle);
    expect(page).toContain("const featured = launchVideo");
    expect(at("<FeaturedVideo")).toBeGreaterThan(-1);
    expect(at("demoVideos.map(")).toBeGreaterThan(at("<FeaturedVideo"));
    expect(at("hubReels.map(")).toBeGreaterThan(at("demoVideos.map("));
    // A section of what an old release changed reads as stale the day the next one ships.
    expect(page).not.toMatch(/releaseVideo|releaseReels|Earlier releases|v\d+\.\d+\.\d+/);
  });

  it("holds no player and no VideoObject: Google indexes a video that is the main content of its page", () => {
    // A page with several videos is told "Video is not the main content
    // of the page" for most of them. The hub links to each video's page
    // instead, and says so in its own data.
    const code = (src: string) => src.replace(/\/\*[\s\S]*?\*\//g, "").replace(/^\s*\/\/.*$/gm, "");
    expect(code(page)).not.toMatch(/<video\b|<WatchPlayer|<DemoCard|videoObjectJsonLd|VideoObject/);
    const hub = parsed(videosHubJsonLd());
    expect(walk(hub).types).not.toContain("VideoObject");
  });

  it("describes itself as a CollectionPage whose ItemList is each video's own page", () => {
    const hub = parsed(videosHubJsonLd());
    expect(hub["@type"]).toBe("CollectionPage");
    expect(hub.url).toBe(absoluteUrl("/videos"));
    expect(hub.mainEntity["@type"]).toBe("ItemList");
    // The hub's list is the videos it shows as cards; the teaser has a page (and a sitemap entry) but no card.
    expect(hub.mainEntity.numberOfItems).toBe(hubVideos.length);
    expect(hub.mainEntity.itemListElement.map((i: { position: number }) => i.position)).toEqual(hubVideos.map((_, i) => i + 1));
    expect(hub.mainEntity.itemListElement.map((i: { url: string }) => i.url)).toEqual(
      hubVideos.map((v) => absoluteUrl(videoPath(v.slug))),
    );
    expect(page).toContain("<JsonLd data={videosHubJsonLd()} />");
    expectNoSocialProof(videosHubJsonLd(), "CollectionPage");
  });

  it("links each demo's card to the demo's own watch page", () => {
    // The card is the same component as for every other video: it links `videoPath(slug)`.
    expect(page).toContain("demoVideos.map((video) => (");
    expect(page).toContain("<VideoLinkCard key={video.slug} video={video} disclosure={false} />");
    expect(cards).toContain("href={videoPath(video.slug)}");
    for (const d of demoVideos) expect(videoPath(d.slug)).toBe(`/videos/${d.slug}`);
  });

  it("is linked from the footer, the header and the home page's demos section", () => {
    const read = (f: string) => readFileSync(join(ROOT, f), "utf8");
    const demoCards = read("components/landing/real-demos.tsx");
    expect(read("components/landing/footer.tsx")).toMatch(/href: "\/videos"/);
    expect(read("components/landing/site-nav.ts")).toMatch(/href: "\/videos"/);
    expect(demoCards).toContain('href="/videos"');
    expect(demoCards).toContain("More videos");
  });
});

describe("a video's own page", () => {
  const page = readFileSync(join(ROOT, "app/videos/[slug]/page.tsx"), "utf8");
  const player = readFileSync(join(ROOT, "components/landing/site-video.tsx"), "utf8");
  const code = (src: string) => src.replace(/\/\*[\s\S]*?\*\//g, "").replace(/^\s*\/\/.*$/gm, "");

  it("exists for every video, generated from the list", () => {
    expect(page).toContain("watchVideos.map((v) => ({ slug: v.slug }))");
    expect(page).toContain("notFound()");
    expect(getVideo("no-such-video")).toBeUndefined();
  });

  it.each(watchVideos.map((v) => [v.slug, v] as const))("%s is in the sitemap and dated", (_slug, video) => {
    const path = videoPath(video.slug);
    const entry = sitemap().find((e) => e.url === absoluteUrl(path));
    expect(entry, `${path} is not in the sitemap`).toBeDefined();
    expect(PAGE_UPDATED[path], `${path} has no date in PAGE_UPDATED`).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect((entry?.lastModified as Date).toISOString().slice(0, 10)).toBe(PAGE_UPDATED[path]);
  });

  it.each(watchVideos.map((v) => [v.slug, v] as const))("%s has a BreadcrumbList Home > Videos > the video", (_slug, video) => {
    const ld = parsed(breadcrumbJsonLd(videoBreadcrumbs(video)));
    expect(ld["@type"]).toBe("BreadcrumbList");
    expect(ld.itemListElement.map((i: { position: number }) => i.position)).toEqual([1, 2, 3]);
    expect(ld.itemListElement.map((i: { item: string }) => i.item)).toEqual([
      siteConfig.url,
      absoluteUrl("/videos"),
      absoluteUrl(videoPath(video.slug)),
    ]);
    expect(ld.itemListElement.map((i: { name: string }) => i.name)).toEqual(["Home", "Videos", video.title]);
  });

  it.each(watchVideos.map((v) => [v.slug, v] as const))("%s has a complete VideoObject, the mp4 as its content", (_slug, video) => {
    const files = { video: videoSrc(video.slug), poster: videoPosterSrc(video.slug) };
    const ld = parsed(videoObjectJsonLd(video, files));
    expect(ld["@type"]).toBe("VideoObject");
    expect(ld.name).toBe(video.title);
    expect(ld.description).toBe(video.description);
    expect(ld.thumbnailUrl).toEqual([absoluteUrl(videoPosterSrc(video.slug))]);
    expect(ld.contentUrl).toBe(absoluteUrl(videoSrc(video.slug)));
    expect("embedUrl" in ld).toBe(false);
    expect(ld.uploadDate).toBe(`${video.uploadDate}T00:00:00Z`);
    expect(ld.duration).toBe(isoDuration(video.seconds));
    expectNoSocialProof(videoObjectJsonLd(video, files), "VideoObject");
  });

  it("puts the video first, and alone, as the main content", () => {
    const body = code(page);
    // The player comes straight after the heading, ahead of every other section.
    expect(body.indexOf("<WatchPlayer")).toBeGreaterThan(-1);
    expect(body.indexOf("<WatchPlayer")).toBeLessThan(body.indexOf("<section"));
    // One player, and nothing else on the page that plays or marks up a video. The related
    // videos are cards, which link to their own pages and hold no `<video>`.
    expect(body.match(/<WatchPlayer/g)?.length).toBe(1);
    expect(body).not.toMatch(/<video\b|<DemoCard|<DemoLinkCard|<FeaturedVideo|videoObjectJsonLd|VideoObject/);
    expect(code(player).match(/<video\b/g)?.length).toBe(1);
    expect(code(player).match(/videoObjectJsonLd\(/g)?.length).toBe(1);
  });

  it("says what it shows and what happens, in order, from the list", () => {
    expect(page).toContain("video.caption");
    expect(page).toContain("video.moments.map(");
    expect(page).toContain("formatMoment(m.at)");
  });

  it("sets its canonical, social card and title with the site's helpers", () => {
    expect(page).toContain("alternates: pageAlternates(videoPath(video.slug))");
    expect(page).toContain("...pageSocial(videoPath(video.slug)");
    expect(page).toContain("title: video.title");
  });

  it("never plays by itself, loads nothing until asked, and plays inline", () => {
    const p = code(player);
    expect(p).toContain('preload="none"');
    expect(p).toContain("playsInline");
    expect(p).toContain("controls");
    expect(p).toContain("poster=");
    expect(p).not.toMatch(/autoPlay|autoplay|\bloop\b/);
  });

  it("offers the launch video's captions as a captions track, not switched on", () => {
    expect(player).toContain('kind="captions"');
    expect(player).toContain('srcLang="en"');
    expect(player).not.toMatch(/<track[^>]*\bdefault\b/);
  });

  it("keeps the three demos' players on the home page and the DJ page, without structured data", () => {
    expect(readFileSync(join(ROOT, "app/page.tsx"), "utf8")).toContain("<RealDemos />");
    expect(readFileSync(join(ROOT, "app/use-cases/dj/page.tsx"), "utf8")).toContain("<DemoCard");
  });
});
