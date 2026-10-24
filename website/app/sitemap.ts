import type { MetadataRoute } from "next";

import { modifiedOf, posts } from "@/lib/blog";
import { escapeXml } from "@/lib/feed";
import { absoluteUrl, pageUpdated, postPath } from "@/lib/seo";
import { type WatchVideo, videoPath, videoPosterSrc, videoSrc, watchVideos } from "@/lib/videos";

type Entry = MetadataRoute.Sitemap[number];

// Every page under `app/` that is a single URL. `seo.test.ts` walks
// `app/` and fails when a page is missing from this list, so a new
// page cannot go unindexed by being forgotten here. The posts are
// added below from the data, not listed.
const pages: { path: string; priority: number; changeFrequency: Entry["changeFrequency"] }[] = [
  { path: "/", priority: 1, changeFrequency: "weekly" },
  { path: "/blog", priority: 0.9, changeFrequency: "weekly" },
  { path: "/docs", priority: 0.9, changeFrequency: "weekly" },
  { path: "/docs/getting-started", priority: 0.8, changeFrequency: "monthly" },
  { path: "/docs/user-guide", priority: 0.8, changeFrequency: "monthly" },
  { path: "/docs/tools", priority: 0.8, changeFrequency: "monthly" },
  { path: "/docs/developer-guide", priority: 0.7, changeFrequency: "monthly" },
  { path: "/docs/api-reference", priority: 0.7, changeFrequency: "monthly" },
  { path: "/use-cases/dj", priority: 0.8, changeFrequency: "monthly" },
  { path: "/use-cases/local-ai-audio-editor", priority: 0.8, changeFrequency: "monthly" },
  { path: "/about", priority: 0.6, changeFrequency: "monthly" },
  { path: "/compare", priority: 0.8, changeFrequency: "monthly" },
  { path: "/alternatives/audacity", priority: 0.8, changeFrequency: "monthly" },
  { path: "/alternatives/adobe-audition", priority: 0.8, changeFrequency: "monthly" },
  { path: "/videos", priority: 0.7, changeFrequency: "monthly" },
  { path: "/changelog", priority: 0.6, changeFrequency: "weekly" },
  { path: "/press", priority: 0.5, changeFrequency: "monthly" },
  { path: "/privacy", priority: 0.3, changeFrequency: "yearly" },
  { path: "/terms", priority: 0.3, changeFrequency: "yearly" },
];

/**
 * The `<video:video>` a watch page declares, for Google's video
 * sitemap: its one video, from the same `lib/videos.ts` entry the page's
 * `VideoObject` is made from, so the two cannot disagree. The fields
 * are the ones Google requires (title, thumbnail, description) and the
 * two it asks for (the file, and its length in seconds).
 *
 * Next writes these strings into the XML as they are, so the text is
 * escaped here. Only the watch pages get one: the hub and the pages
 * that also play the demos (the home page, the DJ page) are not where
 * those videos are the main content.
 */
function videoSitemapEntry(video: WatchVideo) {
  return {
    title: escapeXml(video.title),
    thumbnail_loc: absoluteUrl(videoPosterSrc(video.slug)),
    description: escapeXml(video.description),
    content_loc: absoluteUrl(videoSrc(video.slug)),
    duration: video.seconds,
  };
}

export default function sitemap(): MetadataRoute.Sitemap {
  // The newest post moves the blog index; every other page has a date of
  // its own in `PAGE_UPDATED` (`lib/seo-core.ts`), the day its content
  // last changed, so a date moves only when its page does.
  const blogUpdated = posts.map(modifiedOf).reduce((a, b) => (a > b ? a : b));

  const fixed: MetadataRoute.Sitemap = pages.map((p) => ({
    url: absoluteUrl(p.path),
    lastModified: new Date(p.path === "/blog" ? blogUpdated : pageUpdated(p.path)),
    changeFrequency: p.changeFrequency,
    priority: p.priority,
  }));

  const blogPosts: MetadataRoute.Sitemap = posts.map((post) => ({
    url: absoluteUrl(postPath(post.slug)),
    lastModified: new Date(modifiedOf(post)),
    changeFrequency: "monthly",
    priority: 0.8,
  }));

  // Each video has a page of its own, listed from the data like a post.
  const videoPages: MetadataRoute.Sitemap = watchVideos.map((v) => ({
    url: absoluteUrl(videoPath(v.slug)),
    lastModified: new Date(pageUpdated(videoPath(v.slug))),
    changeFrequency: "monthly",
    priority: 0.6,
    videos: [videoSitemapEntry(v)],
  }));

  return [...fixed, ...blogPosts, ...videoPages];
}
