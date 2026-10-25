/**
 * What the site tells search engines and feed readers, built as plain
 * data so a test can read it without rendering a page.
 *
 * The parts that do not depend on the blog live in `seo-core.ts` and
 * are re-exported here; this file adds what is specific to posts.
 */

import { BLOG_AUTHOR, modifiedOf, wordCount, type BlogPost } from "./blog";
import {
  BLOG_TITLE,
  WEBSITE_ID,
  absoluteUrl,
  type Crumb,
} from "./seo-core";
import { type WatchVideo, videoPath, watchVideos } from "./videos";
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

/** What a `VideoObject` needs to know about a video, which `Demo` and `SiteVideo` both state. */
export interface VideoFacts {
  slug: string;
  title: string;
  description: string;
  uploadDate: string;
  seconds: number;
}

/**
 * A `VideoObject` for one of the site's videos, on that video's own
 * page (`/videos/<slug>`) and on no other: Google indexes a video only
 * where it is the main content of its page. The three full demos play
 * on the home page and the DJ page too, with no structured data there.
 * The thumbnail is the poster the player shows, the content URL is the
 * file it plays, and both are absolute; `where` says which folder they
 * are in (`videoSrc` and `videoPosterSrc` in `lib/videos.ts`). Every
 * field comes from `lib/videos.ts`, which tests hold to the files in
 * `public/videos/` and `public/demos/`. No rating, review or view
 * count: there are none.
 */
/**
 * A day as a date and time in UTC. Google's video report warns about a
 * bare `uploadDate` ("Datetime property is missing a timezone"); the day
 * stays the one written in `lib/videos.ts` and `lib/demos.ts`.
 */
export function uploadDateTime(day: string): string {
  return /^\d{4}-\d{2}-\d{2}$/.test(day) ? `${day}T00:00:00Z` : day;
}

export function videoObjectJsonLd(video: VideoFacts, where: { video: string; poster: string }) {
  return {
    "@context": "https://schema.org",
    "@type": "VideoObject",
    name: video.title,
    description: video.description,
    thumbnailUrl: [absoluteUrl(where.poster)],
    uploadDate: uploadDateTime(video.uploadDate),
    contentUrl: absoluteUrl(where.video),
    duration: isoDuration(video.seconds),
    inLanguage: "en",
  };
}

/** Home, Videos, then the video's own page. */
export function videoBreadcrumbs(video: WatchVideo): Crumb[] {
  return [
    { name: "Home", path: "/" },
    { name: "Videos", path: "/videos" },
    { name: video.title, path: videoPath(video.slug) },
  ];
}

/**
 * The videos hub as a `CollectionPage` whose `ItemList` points at each
 * video's own page. The hub holds no player, so it carries no
 * `VideoObject`: each video's belongs on its page, where the video is
 * the main content.
 */
export function videosHubJsonLd() {
  return {
    "@context": "https://schema.org",
    "@type": "CollectionPage",
    name: "edytlab videos",
    url: absoluteUrl("/videos"),
    isPartOf: { "@id": WEBSITE_ID },
    inLanguage: "en",
    mainEntity: {
      "@type": "ItemList",
      numberOfItems: watchVideos.length,
      itemListElement: watchVideos.map((v, i) => ({
        "@type": "ListItem",
        position: i + 1,
        url: absoluteUrl(videoPath(v.slug)),
        name: v.title,
      })),
    },
  };
}
