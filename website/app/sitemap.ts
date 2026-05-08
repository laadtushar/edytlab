import type { MetadataRoute } from "next";

import { siteConfig } from "@/lib/site";

export default function sitemap(): MetadataRoute.Sitemap {
  const now = new Date();
  return [
    { url: siteConfig.url, lastModified: now, priority: 1 },
    { url: `${siteConfig.url}/privacy`, lastModified: now, priority: 0.5 },
    { url: `${siteConfig.url}/terms`, lastModified: now, priority: 0.5 },
    { url: `${siteConfig.url}/changelog`, lastModified: now, priority: 0.6 },
  ];
}
