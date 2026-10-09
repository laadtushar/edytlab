/**
 * The blog's RSS 2.0 feed, served at `/blog/feed.xml`.
 *
 * Built as a string from `posts` so the route handler stays three lines
 * and a test can check that every post is in it. Each item carries the
 * post's full text in `content:encoded`, so a reader app shows the post
 * and not just a teaser.
 */

import { modifiedOf, postsNewestFirst, type Block, type BlogPost } from "./blog";
import { isExternal, parseInline } from "./inline";
import { BLOG_TITLE, FEED_PATH, absoluteUrl, postPath } from "./seo";
import { siteConfig } from "./site";

export function escapeXml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&apos;");
}

/** A feed date: noon UTC on the post's day, so no time zone moves it to a neighbour. */
export function rfc822(isoDate: string): string {
  return new Date(`${isoDate}T12:00:00Z`).toUTCString();
}

function inlineHtml(text: string): string {
  return parseInline(text)
    .map((t) => {
      switch (t.type) {
        case "text":
          return escapeXml(t.text);
        case "code":
          return `<code>${escapeXml(t.text)}</code>`;
        case "bold":
          return `<strong>${escapeXml(t.text)}</strong>`;
        case "link": {
          // A feed is read off-site, so every link must be absolute.
          const href = isExternal(t.href) ? t.href : absoluteUrl(t.href);
          return `<a href="${escapeXml(href)}">${escapeXml(t.text)}</a>`;
        }
      }
    })
    .join("");
}

function blockHtml(block: Block): string {
  switch (block.type) {
    case "h2":
      return `<h2>${escapeXml(block.text)}</h2>`;
    case "h3":
      return `<h3>${escapeXml(block.text)}</h3>`;
    case "p":
      return `<p>${inlineHtml(block.text)}</p>`;
    case "ul":
      return `<ul>${block.items.map((i) => `<li>${inlineHtml(i)}</li>`).join("")}</ul>`;
    case "ol":
      return `<ol>${block.items.map((i) => `<li>${inlineHtml(i)}</li>`).join("")}</ol>`;
    case "prompt":
      return `<pre><code>${escapeXml(block.text)}</code></pre>`;
    case "callout":
      return `<blockquote>${inlineHtml(block.text)}</blockquote>`;
  }
}

export function postHtml(post: BlogPost): string {
  return post.body.map(blockHtml).join("\n");
}

function itemXml(post: BlogPost): string {
  const url = absoluteUrl(postPath(post.slug));
  const html = postHtml(post).replaceAll("]]>", "]]]]><![CDATA[>");
  return [
    "    <item>",
    `      <title>${escapeXml(post.title)}</title>`,
    `      <link>${escapeXml(url)}</link>`,
    `      <guid isPermaLink="true">${escapeXml(url)}</guid>`,
    `      <pubDate>${rfc822(post.date)}</pubDate>`,
    `      <description>${escapeXml(post.description)}</description>`,
    ...post.tags.map((t) => `      <category>${escapeXml(t)}</category>`),
    `      <content:encoded><![CDATA[${html}]]></content:encoded>`,
    "    </item>",
  ].join("\n");
}

export function buildFeed(list: readonly BlogPost[] = postsNewestFirst()): string {
  const feedUrl = absoluteUrl(FEED_PATH);
  const newest = list.reduce((acc, p) => (modifiedOf(p) > acc ? modifiedOf(p) : acc), "1970-01-01");
  return [
    '<?xml version="1.0" encoding="UTF-8"?>',
    '<rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom" xmlns:content="http://purl.org/rss/1.0/modules/content/">',
    "  <channel>",
    `    <title>${escapeXml(BLOG_TITLE)}</title>`,
    `    <link>${escapeXml(absoluteUrl("/blog"))}</link>`,
    `    <description>${escapeXml(
      "Guides for editing audio by describing what you want: workflows you can copy, how the session graph works, and how to choose a language model.",
    )}</description>`,
    "    <language>en-us</language>",
    `    <lastBuildDate>${rfc822(newest)}</lastBuildDate>`,
    `    <atom:link href="${escapeXml(feedUrl)}" rel="self" type="application/rss+xml" />`,
    `    <generator>${escapeXml(siteConfig.name)}</generator>`,
    ...list.map(itemXml),
    "  </channel>",
    "</rss>",
    "",
  ].join("\n");
}
