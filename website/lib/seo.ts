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
