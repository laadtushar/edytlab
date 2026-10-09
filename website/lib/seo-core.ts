/**
 * The parts of the site's SEO data that do not depend on the blog, kept
 * apart so a client component (the docs shell) can import them without
 * pulling every post into its bundle. `seo.ts` re-exports all of it and
 * adds what is specific to posts.
 *
 * Every absolute URL starts from `siteConfig.url`, so moving the site
 * to another domain is a one-line change there.
 */

import { siteConfig } from "./site";

/** The day the site's pages last changed in substance, for the sitemap. */
export const SITE_UPDATED = "2026-10-09";

export const BLOG_TITLE = "edytlab Blog";
export const FEED_PATH = "/blog/feed.xml";

/**
 * The site's social image, for a page that sets its own `openGraph`.
 * A page's `openGraph` replaces the layout's wholesale, images
 * included, so without this such a page has no `og:image` at all. A
 * post does not use it: it has a card of its own.
 */
export const DEFAULT_OG_IMAGE = {
  url: "/opengraph-image",
  width: 1200,
  height: 630,
  alt: "edytlab — Describe it. Get pro-grade audio edits.",
} as const;

/** `/blog/x` becomes `https://edytlab.com/blog/x`; `/` is the bare origin. */
export function absoluteUrl(path = "/"): string {
  return path === "/" ? siteConfig.url : `${siteConfig.url}${path}`;
}

/**
 * The `alternates` block every indexable page uses: its own canonical
 * URL, and the RSS feed. A page that sets `alternates` replaces the
 * layout's wholesale, so the feed link has to ride along with each.
 */
export function pageAlternates(path: string) {
  return {
    canonical: path,
    types: {
      "application/rss+xml": [{ url: FEED_PATH, title: BLOG_TITLE }],
    },
  };
}

export interface Crumb {
  name: string;
  path: string;
}

export function breadcrumbJsonLd(trail: readonly Crumb[]) {
  return {
    "@context": "https://schema.org",
    "@type": "BreadcrumbList",
    itemListElement: trail.map((crumb, i) => ({
      "@type": "ListItem",
      position: i + 1,
      name: crumb.name,
      item: absoluteUrl(crumb.path),
    })),
  };
}

/**
 * JSON for a `<script type="application/ld+json">` body.
 *
 * `<` is escaped so a title containing `</script>` cannot end the tag
 * early. The escape is valid JSON, so a parser reads back the same data.
 */
export function serializeJsonLd(data: unknown): string {
  return JSON.stringify(data).replace(/</g, "\\u003c");
}

/** The home page's `SoftwareApplication`. `version` is the release the page shows, if it is a real one. */
export function softwareJsonLd(version?: string) {
  return {
    "@context": "https://schema.org",
    "@type": "SoftwareApplication",
    name: siteConfig.name,
    description: siteConfig.description,
    applicationCategory: "MultimediaApplication",
    operatingSystem: "macOS, Windows, Linux",
    url: siteConfig.url,
    downloadUrl: siteConfig.releases,
    ...(version ? { softwareVersion: version.replace(/^v/, "") } : {}),
    offers: { "@type": "Offer", price: "0", priceCurrency: "USD" },
    publisher: {
      "@type": "Organization",
      name: siteConfig.name,
      url: siteConfig.url,
      sameAs: [siteConfig.github],
    },
  };
}
