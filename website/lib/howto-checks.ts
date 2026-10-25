/**
 * The checks every sourced audio how-to post is held to, shared by
 * `blog-posts-audio.test.ts` (the first four) and
 * `blog-posts-audio-2.test.ts` (the next five), so a rule is stated once:
 * every outside fact is linked and dated in a "Sources" list, a figure with
 * no primary page is not given, nothing is ranked or called tested, and the
 * question the title asks is answered in the first paragraph.
 *
 * Not a test file itself (no `.test.` in the name); it only registers
 * `describe` blocks when a test file calls it.
 */

import { describe, expect, it } from "vitest";

import sitemap from "../app/sitemap";
import { MAX_DESCRIPTION, posts, wordCount, type Block, type BlogPost } from "./blog";
import { buildFeed } from "./feed";
import { isExternal, linksIn, stripInline } from "./inline";
import { absoluteUrl, postPath } from "./seo";
import { siteConfig } from "./site";
import { getVideo } from "./videos";

/** Every string in a post that can carry inline markup. */
export function markup(post: BlogPost): string[] {
  return post.body.flatMap((b: Block) => {
    switch (b.type) {
      case "ul":
      case "ol":
        return b.items;
      case "prompt":
      case "code":
      case "h2":
      case "h3":
        return [];
      default:
        return [b.text];
    }
  });
}

/** Every string a reader reads, headings and code included. */
export function allText(post: BlogPost): string[] {
  return post.body.flatMap((b) => (b.type === "ul" || b.type === "ol" ? b.items : [b.text]));
}

/** The list items under the "Sources" heading, and what comes before it. */
function split(post: BlogPost): { before: Block[]; sources: string[]; after: Block[] } {
  const at = post.body.findIndex((b) => b.type === "h2" && b.text === "Sources");
  expect(at, `${post.slug} has no Sources heading`).toBeGreaterThan(0);
  const rest = post.body.slice(at + 1);
  const list = rest.find((b) => b.type === "ul");
  expect(list, `${post.slug}: no list under Sources`).toBeDefined();
  const listAt = rest.indexOf(list!);
  return {
    before: post.body.slice(0, at),
    sources: list!.type === "ul" ? list!.items : [],
    after: rest.slice(listAt + 1),
  };
}

export interface HowtoBatch {
  /** The describe title. */
  name: string;
  /** The posts of the batch. */
  batch: BlogPost[];
  /** Each slug, and the question its first paragraph has to answer. */
  expected: [string, RegExp][];
  /** The day every outside page was read, which is also each post's date. */
  read: string;
  /** The commit the edytlab links point at. */
  commit: string;
  /**
   * A later day on which a post's edytlab sources were read again, for an
   * update (a source item may say `read <this day>` in place of `read <read>`).
   */
  reread?: string;
}

export function describeHowtoPosts({ name, batch, expected, read, commit, reread }: HowtoBatch): void {
  describe(name, () => {
    it("are these, on the blog, dated the day their sources were read", () => {
      expect(batch.map((p) => p.slug).sort()).toEqual(expected.map(([s]) => s).sort());
      for (const p of batch) {
        expect(posts).toContain(p);
        expect(p.date).toBe(read);
        expect(p.dateModified === undefined || p.dateModified >= p.date).toBe(true);
      }
    });

    it("are in the sitemap and the feed", () => {
      const urls = sitemap().map((e) => e.url);
      const feed = buildFeed();
      for (const p of batch) {
        expect(urls).toContain(absoluteUrl(postPath(p.slug)));
        expect(feed).toContain(`<guid isPermaLink="true">${absoluteUrl(postPath(p.slug))}</guid>`);
      }
    });

    it("names the commit the edytlab source links point at, as a full hash", () => {
      expect(commit).toMatch(/^[0-9a-f]{40}$/);
    });

    describe.each(batch.map((p) => [p.slug, p] as const))("%s", (_slug, post) => {
      it("fits a search result: the title with the site's suffix is at most 60 characters", () => {
        expect(`${post.title} · edytlab`.length).toBeLessThanOrEqual(60);
        expect(post.description.length).toBeLessThanOrEqual(MAX_DESCRIPTION);
        expect(post.title).not.toMatch(/\b(?:best|only|ultimate|perfect|greatest|top)\b/i);
      });

      it("is a post of a real length with a read time that matches it", () => {
        const words = wordCount(post);
        expect(words).toBeGreaterThanOrEqual(1000);
        expect(words).toBeLessThanOrEqual(2400);
        expect(post.readTime).toBe(Math.ceil(words / 220));
      });

      it("answers the question of its title in the first paragraph", () => {
        const first = post.body[0];
        expect(first.type).toBe("p");
        const want = expected.find(([s]) => s === post.slug)![1];
        expect(stripInline(first.type === "p" ? first.text : "")).toMatch(want);
      });

      it("links the docs, the releases, a page of the site's own to read next and a watch page", () => {
        const links = markup(post).flatMap(linksIn);
        expect(links.some((l) => l.startsWith("/docs")), "no link into /docs").toBe(true);
        expect(links.some((l) => l === siteConfig.releases), "no link to the releases").toBe(true);
        expect(links.some((l) => l.startsWith("/use-cases/") || l.startsWith("/blog/")), "no related page").toBe(true);
        const videos = links.filter((l) => l.startsWith("/videos/"));
        expect(videos.length, "no watch page").toBeGreaterThan(0);
        for (const v of videos) expect(getVideo(v.slice("/videos/".length)), `no video ${v}`).toBeDefined();
        expect(post.body.some((b) => b.type === "prompt"), "no example prompt").toBe(true);
      });

      it("says where the unsigned installers warn wherever it links the releases", () => {
        const withRelease = markup(post).filter((t) => linksIn(t).includes(siteConfig.releases));
        expect(withRelease.length).toBeGreaterThan(0);
        for (const t of withRelease) expect(t, post.slug).toMatch(/unsigned/);
      });

      it("has a visible questions section of at least three questions, each answered", () => {
        const at = post.body.findIndex((b) => b.type === "h2" && /^Questions about /.test(b.text));
        expect(at, "no Questions about… section").toBeGreaterThan(0);
        const end = post.body.findIndex((b, i) => i > at && b.type === "h2");
        const section = post.body.slice(at + 1, end);
        const questions = section.filter((b) => b.type === "h3");
        expect(questions.length).toBeGreaterThanOrEqual(3);
        for (const q of questions) expect(q.type === "h3" && q.text.endsWith("?"), q.type === "h3" ? q.text : "").toBe(true);
        section.forEach((b, i) => {
          if (b.type === "h3") expect(section[i + 1]?.type, `no answer under "${b.text}"`).toBe("p");
        });
      });

      it("ends with a Sources list in which every link is https and says the day it was read", () => {
        const { sources, after } = split(post);
        expect(sources.length).toBeGreaterThanOrEqual(5);
        for (const item of sources) {
          const links = linksIn(item);
          expect(links.length, item).toBeGreaterThan(0);
          for (const l of links) expect(l, item).toMatch(/^https:\/\//);
          const days = reread ? [read, reread] : [read];
          expect(
            days.some((d) => item.includes(`read ${d}`)),
            `no read date in ${item}`,
          ).toBe(true);
        }
        // Only a closing note may follow the list.
        for (const b of after) expect(b.type).toBe("p");
      });

      it("cites every outside page it links, in its Sources", () => {
        const { before, sources } = split(post);
        const cited = new Set(sources.flatMap(linksIn));
        const linked = before
          .flatMap((b) => (b.type === "ul" || b.type === "ol" ? b.items : b.type === "prompt" || b.type === "code" ? [] : [b.text]))
          .flatMap(linksIn)
          .filter(isExternal)
          // The download and the repository are links to the project, not sources.
          .filter((l) => l !== siteConfig.releases && l !== siteConfig.github);
        for (const l of linked) expect(cited.has(l), `${l} is linked but not in Sources`).toBe(true);
      });

      it("does not rank, count, rate or call itself tested", () => {
        for (const t of allText(post)) {
          expect(t, post.slug).not.toMatch(/\bbest\b|\bbetter than\b|\bworst\b|\bfastest\b|\bcheapest\b/i);
          expect(t, post.slug).not.toMatch(/\btested\b/i);
          expect(t, post.slug).not.toMatch(/\b\d[\d,]*\s*(?:users|downloads|stars|ratings|reviews)\b/i);
        }
      });

      it("never describes what edytlab does not do as working", () => {
        // Stem separation and the things that need it have not shipped
        // (transcription did, in v0.8.0, but none of these posts needs it); the limiter is a sample-peak ceiling and not a true-peak one;
        // nothing here is ML enhancement, MIDI or plugin hosting.
        const unshipped =
          /stem separation|vocal isolation|speaker detection|ML enhance|MIDI|plugin hosting|VST|true-peak (?:limiter|compliance|compliant)/i;
        for (const t of allText(post)) expect(t, `${post.slug}: ${t}`).not.toMatch(unshipped);
      });

      it("says what a reported run was made on, and states no benchmark", () => {
        // A run is described as one: synthetic audio, the tools called directly.
        expect(allText(post).join(" ")).toMatch(/synthetic/);
        const benchmark = /\b\d+(?:\.\d+)?\s?(?:x|×)\s+(?:faster|slower|cheaper)|\b\d+%\s+(?:faster|cheaper|more|less)|per minute of audio|\bRTX\b|\bM[1-4]\b/i;
        for (const t of allText(post)) expect(t, `${post.slug}: ${t}`).not.toMatch(benchmark);
      });
    });
  });
}
