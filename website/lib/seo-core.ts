/**
 * The parts of the site's SEO data that do not depend on the blog, kept
 * apart so a client component (the docs shell) can import them without
 * pulling every post into its bundle. `seo.ts` re-exports all of it and
 * adds what is specific to posts.
 *
 * Every absolute URL starts from `siteConfig.url`, so moving the site
 * to another domain is a one-line change there.
 */

import type { Metadata } from "next";

import { FOUNDER, LABYNATOR } from "./family";
import { siteConfig } from "./site";

/** The day the site's pages last changed in substance, for the sitemap. */
export const SITE_UPDATED = "2026-10-10";

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

/** `/blog/x` becomes `https://www.edytlab.com/blog/x`; `/` is the bare origin. */
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

/**
 * The `openGraph` and `twitter` blocks for a page that has none of its
 * own. Without them the page inherits the layout's, whose `og:url` is
 * the home page: a share of `/changelog` would name `/` as its address
 * and disagree with the page's canonical.
 */
export function pageSocial(path: string, title: string, description: string) {
  return {
    openGraph: {
      type: "website",
      url: absoluteUrl(path),
      title,
      description,
      siteName: siteConfig.name,
      images: [DEFAULT_OG_IMAGE],
    },
    twitter: {
      card: "summary_large_image",
      title,
      description,
      images: [DEFAULT_OG_IMAGE.url],
    },
  } satisfies Pick<Metadata, "openGraph" | "twitter">;
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

/** The licence the app is released under, as schema.org wants it: a URL. */
export const LICENSE_URL = "https://opensource.org/licenses/MIT";

/**
 * The home page's `SoftwareApplication`.
 *
 * `version` is the release the page offers for download, the same one
 * its badge and buttons show (`getLatestRelease()`), so this block
 * cannot name a version the page does not. Leave it out when that is
 * only the placeholder used while GitHub is unreachable.
 *
 * The app is free and open source, so `offers` says price 0 and
 * `license` the MIT URL. It carries no rating, review or user count:
 * there are none to report.
 */
export function softwareJsonLd(version?: string) {
  return {
    "@context": "https://schema.org",
    "@type": "SoftwareApplication",
    name: siteConfig.name,
    url: siteConfig.url,
    description: siteConfig.description,
    applicationCategory: "MultimediaApplication",
    operatingSystem: "macOS, Windows, Linux",
    downloadUrl: siteConfig.releases,
    ...(version ? { softwareVersion: version.replace(/^v/, "") } : {}),
    license: LICENSE_URL,
    offers: { "@type": "Offer", price: "0", priceCurrency: "USD" },
    sameAs: [siteConfig.github],
    publisher: { "@type": "Organization", name: LABYNATOR.name, url: LABYNATOR.url },
    author: { "@type": "Person", name: FOUNDER.name, url: FOUNDER.url },
  };
}
