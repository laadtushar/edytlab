/**
 * The four audio how-to posts of 2026-10-22, held to the rules their
 * file states (`blog-posts-audio/index.ts`): every outside fact is
 * linked and dated in a "Sources" list, a figure with no primary page is
 * not given, nothing is ranked or called tested, and the question the
 * title asks is answered in the first paragraph.
 *
 * The checks all sourced how-to posts share are in `howto-checks.ts`,
 * which `blog-posts-audio-2.test.ts` runs on the next five. The properties
 * every post has (a cover, a social image, a description a search result
 * can show, internal links that exist) are held by `blog.test.ts` and
 * `seo.test.ts` for the posts as a set; this file is what is particular to
 * these four.
 */

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import { audioHowtoPosts } from "./blog-posts-audio";
import { COMMIT, READ } from "./blog-posts-audio/shared";
import { allText, describeHowtoPosts } from "./howto-checks";

const ROOT = fileURLToPath(new URL("../", import.meta.url));

/** The slugs, and the question each post has to answer in its first paragraph. */
const EXPECTED: [string, RegExp][] = [
  ["lufs-for-spotify-youtube-apple-music-podcasts", /-14 LUFS[\s\S]*-16[\s\S]*-23 LUFS/],
  ["batch-normalize-audio-files-to-a-lufs-target", /batch normalize audio files[\s\S]*-16 LUFS/],
  ["remove-silence-from-audio-without-chopping-breaths", /remove silence from audio[\s\S]*threshold[\s\S]*minimum duration/],
  ["change-tempo-without-changing-pitch", /change tempo without changing pitch[\s\S]*time-stretch/],
];

describeHowtoPosts({
  name: "the four audio how-to posts",
  batch: audioHowtoPosts,
  expected: EXPECTED,
  read: READ,
  commit: COMMIT,
});

describe("what the posts do not say about other platforms", () => {
  const lufs = audioHowtoPosts.find((p) => p.slug.startsWith("lufs-for"))!;
  const text = allText(lufs).join("\n");

  it("gives no LUFS figure for YouTube, Apple Music or Instagram, and says none was found", () => {
    // A figure within a sentence that names one of the three would be a claim
    // with no primary page behind it.
    for (const sentence of text.split(/(?<=[.!?])\s+/)) {
      if (/YouTube|Apple Music|Instagram/.test(sentence) && /-?\d+\s*(?:LUFS|LKFS|dB)/.test(sentence)) {
        // The only allowed shapes: the Apple Digital Masters headroom, the
        // `youtube` preset (a number edytlab ships) and the 1 dB headroom.
        expect(sentence).toMatch(/headroom|preset|`youtube`|Digital Masters/);
      }
    }
    expect(text).toMatch(/no official page that could be found states a figure/);
    expect(text).toMatch(/no help page that states a figure/);
  });

  it("says the limiter and the ceiling are sample-peak, and shows it", () => {
    expect(text).toMatch(/sample-peak ceiling, not an inter-sample true-peak one/);
    expect(text).toMatch(/cannot tell you how large the gap is/);
  });
});

describe("the posts do not turn the blog into a source of FAQ markup", () => {
  it("emits no FAQPage structured data", () => {
    // FAQ rich results stopped on 2026-05-07, and these are visible questions
    // and answers, not schema.
    for (const file of ["lib/seo.ts", "lib/seo-core.ts", "app/blog/[slug]/page.tsx", "app/blog/page.tsx"]) {
      expect(readFileSync(join(ROOT, file), "utf8"), file).not.toContain("FAQPage");
    }
  });
});
