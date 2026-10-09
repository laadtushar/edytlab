import type { MetadataRoute } from "next";

import { modifiedOf, posts } from "@/lib/blog";
import { SITE_UPDATED, absoluteUrl, postPath } from "@/lib/seo";

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
  { path: "/changelog", priority: 0.6, changeFrequency: "weekly" },
  { path: "/privacy", priority: 0.3, changeFrequency: "yearly" },
  { path: "/terms", priority: 0.3, changeFrequency: "yearly" },
];

export default function sitemap(): MetadataRoute.Sitemap {
  // The newest post moves the blog index; the site-wide date moves the rest.
  const blogUpdated = posts.map(modifiedOf).reduce((a, b) => (a > b ? a : b));

  const fixed: MetadataRoute.Sitemap = pages.map((p) => ({
    url: absoluteUrl(p.path),
    lastModified: new Date(p.path === "/blog" ? blogUpdated : SITE_UPDATED),
    changeFrequency: p.changeFrequency,
    priority: p.priority,
  }));

  const blogPosts: MetadataRoute.Sitemap = posts.map((post) => ({
    url: absoluteUrl(postPath(post.slug)),
    lastModified: new Date(modifiedOf(post)),
    changeFrequency: "monthly",
    priority: 0.8,
  }));

  return [...fixed, ...blogPosts];
}
