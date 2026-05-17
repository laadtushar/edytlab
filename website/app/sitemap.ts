import type { MetadataRoute } from "next";

import { siteConfig } from "@/lib/site";
import { posts } from "@/lib/blog";

export default function sitemap(): MetadataRoute.Sitemap {
  const now = new Date();
  const blogPosts = posts.map((post) => ({
    url: `${siteConfig.url}/blog/${post.slug}`,
    lastModified: new Date(post.date),
    priority: 0.8,
    changeFrequency: "monthly" as const,
  }));

  return [
    { url: siteConfig.url, lastModified: now, priority: 1, changeFrequency: "weekly" },
    { url: `${siteConfig.url}/blog`, lastModified: now, priority: 0.9, changeFrequency: "weekly" },
    ...blogPosts,
    { url: `${siteConfig.url}/changelog`, lastModified: now, priority: 0.6, changeFrequency: "weekly" },
    { url: `${siteConfig.url}/privacy`, lastModified: now, priority: 0.3, changeFrequency: "yearly" },
    { url: `${siteConfig.url}/terms`, lastModified: now, priority: 0.3, changeFrequency: "yearly" },
  ];
}
