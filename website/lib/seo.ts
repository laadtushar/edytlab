/**
 * What the site tells search engines and feed readers, built as plain
 * data so a test can read it without rendering a page.
 *
 * The parts that do not depend on the blog live in `seo-core.ts` and
 * are re-exported here; this file adds what is specific to posts.
 */

import { BLOG_AUTHOR, modifiedOf, wordCount, type BlogPost } from "./blog";
import { type Demo, demoPosterSrc, demoVideoSrc } from "./demos";
import {
  BLOG_TITLE,
  absoluteUrl,
  type Crumb,
} from "./seo-core";
import { siteConfig } from "./site";

export * from "./seo-core";

export const postPath = (slug: string) => `/blog/${slug}`;

/**
 * Where a post's social image is served. Next builds it from
 * `app/blog/[slug]/opengraph-image.tsx`, which is why this path is not
 * a file in `public/`.
 */
export const postImagePath = (slug: string) => `/blog/${slug}/opengraph-image`;

export function postBreadcrumbs(post: BlogPost): Crumb[] {
  return [
    { name: "Home", path: "/" },
    { name: "Blog", path: "/blog" },
    { name: post.title, path: postPath(post.slug) },
  ];
}

export function blogPostingJsonLd(post: BlogPost) {
  const url = absoluteUrl(postPath(post.slug));
  return {
    "@context": "https://schema.org",
    "@type": "BlogPosting",
    headline: post.title,
    description: post.description,
    datePublished: post.date,
    dateModified: modifiedOf(post),
    author: { "@type": "Organization", name: BLOG_AUTHOR.name, url: BLOG_AUTHOR.url },
    publisher: {
      "@type": "Organization",
      name: siteConfig.name,
      url: siteConfig.url,
      logo: { "@type": "ImageObject", url: absoluteUrl("/logo.png") },
    },
    image: [absoluteUrl(postImagePath(post.slug))],
    mainEntityOfPage: { "@type": "WebPage", "@id": url },
    url,
    keywords: post.tags.join(", "),
    wordCount: wordCount(post),
    inLanguage: "en",
    isPartOf: { "@type": "Blog", name: BLOG_TITLE, url: absoluteUrl("/blog") },
  };
}

/**
 * A length in seconds as an ISO 8601 duration, which is what a
 * `VideoObject` wants: 165 is `PT2M45S`.
 */
export function isoDuration(totalSeconds: number): string {
  const h = Math.floor(totalSeconds / 3600);
  const m = Math.floor((totalSeconds % 3600) / 60);
  const s = Math.round(totalSeconds % 60);
  return `PT${h ? `${h}H` : ""}${h || m ? `${m}M` : ""}${s}S`;
}

/**
 * A `VideoObject` for one of the demo recordings.
 *
 * Emitted next to the player, on the pages where the video can be
 * watched (the home page's demos section and the DJ use-case page), as
 * Google asks. The thumbnail is the poster the player shows, the
 * content URL is the file it plays, and both are absolute. Every field
 * comes from `lib/demos.ts`, which a test holds to the files in
 * `public/demos/`. No rating, review or view count: there are none.
 */
export function videoObjectJsonLd(demo: Demo) {
  return {
    "@context": "https://schema.org",
    "@type": "VideoObject",
    name: demo.title,
    description: demo.description,
    thumbnailUrl: [absoluteUrl(demoPosterSrc(demo.slug))],
    uploadDate: demo.uploadDate,
    contentUrl: absoluteUrl(demoVideoSrc(demo.slug)),
    duration: isoDuration(demo.seconds),
    inLanguage: "en",
  };
}
