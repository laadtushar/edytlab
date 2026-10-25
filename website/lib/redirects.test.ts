/**
 * The videos the site took down, and where their addresses go now.
 *
 * The v0.7.0 release video and its three reels had pages of their own
 * and files under `public/videos/`, and those addresses are still linked
 * from outside: the sitemap Google read, Search Console's indexing
 * requests, launch posts. They were removed once v0.9.1 shipped, so each
 * page and each file now moves permanently (308) to the hub. This holds
 * the list, checks it with the matcher Next compiles `redirects()`
 * sources with, and checks that no redirect hides a page or a file the
 * site still serves.
 */

import { existsSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

// The matcher Next compiles `redirects()` sources with. Next ships no types for it.
// @ts-expect-error TS7016: no declaration file for a module inside `next`
import { pathToRegexp } from "next/dist/compiled/path-to-regexp";
import { describe, expect, it } from "vitest";

import sitemap from "../app/sitemap";
import nextConfig from "../next.config.mjs";
import { PAGE_UPDATED } from "./seo-core";
import { videoCaptionsSrc, videoPath, videoPosterSrc, videoSrc, watchVideos } from "./videos";

const SITE = fileURLToPath(new URL("../", import.meta.url));

const REMOVED: Record<string, readonly string[]> = {
  "whats-new-v070": ["mp4", "jpg", "vtt"],
  "reel-v070-whats-new": ["mp4", "jpg"],
  "reel-v070-describe-the-edit": ["mp4", "jpg"],
  "reel-v070-200-tracks": ["mp4", "jpg"],
};

/** Every address a removed video had: its page and each of its files. */
const OLD_ADDRESSES = Object.entries(REMOVED).flatMap(([slug, exts]) => [
  `/videos/${slug}`,
  ...exts.map((ext) => `/videos/${slug}.${ext}`),
]);

async function redirectFor(path: string) {
  const rules = (await nextConfig.redirects?.()) ?? [];
  return rules.filter((r) =>
    (pathToRegexp(r.source, [], { sensitive: false, delimiter: "/", strict: false }) as RegExp).test(path),
  );
}

describe("the removed videos", () => {
  it.each(OLD_ADDRESSES)("%s moves permanently to /videos", async (path) => {
    const rules = await redirectFor(path);
    expect(rules, `${path} has no redirect`).toHaveLength(1);
    expect(rules[0].destination).toBe("/videos");
    // `permanent: true` is a 308, which keeps the method and tells a search engine the move is for good.
    expect(rules[0].permanent).toBe(true);
    expect("statusCode" in rules[0]).toBe(false);
  });

  it("redirects nothing else", async () => {
    const rules = (await nextConfig.redirects?.()) ?? [];
    expect(rules.map((r) => r.source).sort()).toEqual([...OLD_ADDRESSES].sort());
  });

  it("leaves every page and file the site still serves alone", async () => {
    const live = [
      "/videos",
      "/videos/",
      ...watchVideos.flatMap((v) => [videoPath(v.slug), videoSrc(v.slug), videoPosterSrc(v.slug), videoCaptionsSrc(v.slug)]),
    ];
    for (const path of live) expect(await redirectFor(path), path).toEqual([]);
  });

  it("are gone from the list, the sitemap, the page dates and public/videos", () => {
    const slugs = watchVideos.map((v) => v.slug);
    const urls = sitemap().map((e) => e.url);
    for (const [slug, exts] of Object.entries(REMOVED)) {
      expect(slugs).not.toContain(slug);
      expect(urls.some((u) => u.includes(slug)), `${slug} is still in the sitemap`).toBe(false);
      expect(PAGE_UPDATED[`/videos/${slug}`]).toBeUndefined();
      for (const ext of exts) {
        expect(existsSync(join(SITE, "public/videos", `${slug}.${ext}`)), `public/videos/${slug}.${ext}`).toBe(false);
      }
    }
  });
});
