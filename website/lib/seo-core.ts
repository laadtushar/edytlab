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

/**
 * The fallback date for a page with no entry in `PAGE_UPDATED`: the day
 * the site was last checked as a whole. A page should have its own
 * (`seo.test.ts` fails when one does not), because Google reads
 * `<lastmod>` only "if it's consistently and verifiably" accurate, and
 * one date on every page is not verifiable for any of them.
 */
export const SITE_UPDATED = "2026-10-24";

/**
 * The day each page's content last changed in substance, by path, for
 * the sitemap and for the `dateModified` of a page's `Article` data.
 *
 * Each date is the committer date of the last change to the files the
 * page is made of, read when this was written:
 *
 *     git log -1 --format=%cs -- <the page's source files>
 *
 * The source files are the page's own `page.tsx` plus the components
 * and data that only it renders (the home page's sections, the demo
 * list for the pages that play the demos). The header, the footer and
 * the shells are on every page and not part of what a page says, so a
 * change to one does not move the others. `/blog` is not here: its date
 * is its newest post's.
 *
 * Move a date when a page's content changes, not when the page is
 * rebuilt: changing dates to look fresh is on Google's list of warning
 * signs for helpful-content review.
 */
export const PAGE_UPDATED: Readonly<Record<string, string>> = {
  "/": "2026-10-24",
  "/docs": "2026-10-23",
  "/docs/getting-started": "2026-10-22",
  "/docs/user-guide": "2026-10-23",
  "/docs/tools": "2026-10-23",
  "/docs/developer-guide": "2026-10-23",
  "/docs/api-reference": "2026-10-24",
  "/use-cases/dj": "2026-10-22",
  "/use-cases/local-ai-audio-editor": "2026-10-24",
  "/about": "2026-10-24",
  "/alternatives/audacity": "2026-10-24",
  "/alternatives/adobe-audition": "2026-10-24",
  "/compare": "2026-10-24",
  "/videos": "2026-10-24",
  "/videos/whats-new-v070": "2026-10-23",
  "/videos/launch-video": "2026-10-24",
  "/videos/dj-beatmatched-transition": "2026-10-23",
  "/videos/dj-extended-club-intro": "2026-10-23",
  "/videos/dj-mini-mix": "2026-10-23",
  "/videos/reel-v070-whats-new": "2026-10-23",
  "/videos/reel-v070-describe-the-edit": "2026-10-23",
  "/videos/reel-v070-200-tracks": "2026-10-23",
  "/videos/reel-highlights": "2026-10-23",
  "/videos/reel-teaser": "2026-10-23",
  "/videos/reel-beatmatched-transition": "2026-10-23",
  "/videos/reel-extended-club-intro": "2026-10-23",
  "/videos/reel-mini-mix": "2026-10-23",
  "/changelog": "2026-10-23",
  "/press": "2026-10-24",
  "/privacy": "2026-10-24",
  "/terms": "2026-10-23",
};

/** When a page last changed in substance: its own date, else the site's. */
export function pageUpdated(path: string): string {
  return PAGE_UPDATED[path] ?? SITE_UPDATED;
}

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

/**
 * Stable identifiers for the three things the home page describes, so
 * other blocks (and other pages) can point at them instead of repeating
 * them, and a crawler reads each as one entity across visits. They are
 * URLs under the home page and never change with a release.
 */
export const ORGANIZATION_ID = `${siteConfig.url}/#organization`;
export const WEBSITE_ID = `${siteConfig.url}/#website`;
export const SOFTWARE_ID = `${siteConfig.url}/#software`;

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
    "@id": SOFTWARE_ID,
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

/**
 * The home page's `Organization`: edytlab itself, as the project that
 * publishes the app, with its mark (`public/logo.png`, 512 by 512, well
 * over the 112 by 112 minimum), where it lives on the web, and the two
 * facts the footer states: it is part of LabyNator and was built by
 * Tushar Laad.
 *
 * `sameAs` is the repository and nothing else: social handles are
 * omitted until the accounts exist (see the footer). No rating, review
 * or count of any kind.
 */
export function organizationJsonLd() {
  return {
    "@context": "https://schema.org",
    "@type": "Organization",
    "@id": ORGANIZATION_ID,
    name: siteConfig.name,
    url: siteConfig.url,
    logo: { "@type": "ImageObject", url: absoluteUrl("/logo.png"), width: 512, height: 512 },
    description: siteConfig.description,
    sameAs: [siteConfig.github],
    founder: { "@type": "Person", name: FOUNDER.name, url: FOUNDER.url },
    parentOrganization: { "@type": "Organization", name: LABYNATOR.name, url: LABYNATOR.url },
  };
}

/**
 * The home page's `WebSite`. Its `name` is what Google may show as the
 * site name in results, so it is the brand and nothing else.
 */
export function websiteJsonLd() {
  return {
    "@context": "https://schema.org",
    "@type": "WebSite",
    "@id": WEBSITE_ID,
    name: siteConfig.name,
    url: siteConfig.url,
    description: siteConfig.description,
    inLanguage: "en",
    publisher: { "@id": ORGANIZATION_ID },
  };
}

/**
 * An `Article` for a page that reads as one: the use-case pages.
 *
 * Authored by the founder, as the rest of the site names him, and
 * published by the project. `dateModified` comes from `PAGE_UPDATED`,
 * the date the sitemap gives the same page, so the two cannot disagree.
 * `datePublished` is when the page first went up, read from git when it
 * was written and passed in by the page.
 */
export function articleJsonLd(page: {
  path: string;
  headline: string;
  description: string;
  datePublished: string;
}) {
  const url = absoluteUrl(page.path);
  return {
    "@context": "https://schema.org",
    "@type": "Article",
    headline: page.headline,
    description: page.description,
    datePublished: page.datePublished,
    dateModified: pageUpdated(page.path),
    author: { "@type": "Person", name: FOUNDER.name, url: FOUNDER.url },
    publisher: {
      "@type": "Organization",
      "@id": ORGANIZATION_ID,
      name: siteConfig.name,
      url: siteConfig.url,
      logo: { "@type": "ImageObject", url: absoluteUrl("/logo.png") },
    },
    image: [absoluteUrl(DEFAULT_OG_IMAGE.url)],
    mainEntityOfPage: { "@type": "WebPage", "@id": url },
    url,
    inLanguage: "en",
  };
}

/**
 * The trail for a page one level below the home page: Home, then the
 * page. (There is no `/use-cases` index to put between them.)
 */
export function pageBreadcrumbs(name: string, path: string): Crumb[] {
  return [
    { name: "Home", path: "/" },
    { name, path },
  ];
}

/**
 * The trail for a page that sits under the comparison hub: Home, then
 * Compare (`/compare`), then the page. The Audacity and Audition pages
 * use it; the hub itself is one level below the home page and uses
 * `pageBreadcrumbs`.
 */
export function compareBreadcrumbs(name: string, path: string): Crumb[] {
  return [
    { name: "Home", path: "/" },
    { name: "Compare", path: "/compare" },
    { name, path },
  ];
}

/**
 * The `CollectionPage` the comparison hub is: a page whose main content
 * is a list, here the products compared, each pointing at the hub's own
 * section for it. No `Review`, `Rating` or `Product` anywhere: the hub
 * reports what each product's pages say and scores nothing.
 */
export function compareHubJsonLd(page: {
  path: string;
  name: string;
  description: string;
  items: readonly { name: string; anchor: string }[];
}) {
  const url = absoluteUrl(page.path);
  return {
    "@context": "https://schema.org",
    "@type": "CollectionPage",
    name: page.name,
    description: page.description,
    url,
    inLanguage: "en",
    dateModified: pageUpdated(page.path),
    isPartOf: { "@id": WEBSITE_ID },
    author: { "@type": "Person", name: FOUNDER.name, url: FOUNDER.url },
    mainEntity: {
      "@type": "ItemList",
      numberOfItems: page.items.length,
      itemListOrder: "https://schema.org/ItemListUnordered",
      itemListElement: page.items.map((item, i) => ({
        "@type": "ListItem",
        position: i + 1,
        name: item.name,
        url: `${url}#${item.anchor}`,
      })),
    },
  };
}
