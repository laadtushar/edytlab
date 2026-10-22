/**
 * The videos the site serves against the files they are copies of.
 *
 * `/videos` plays the launch video and the vertical reels from
 * `public/videos/`. Each is a copy of a file under `marketing/`, which
 * is where it is cut and kept, so a re-cut there leaves the site playing
 * the old one unless something notices. This does: every site copy has
 * to be byte-identical (SHA-256) to its source, the list and the folder
 * have to match (no file that is not listed, none missing, a poster for
 * every video), and each running time and picture size in `lib/videos.ts`
 * has to be the one in the file's own header.
 *
 * Only the plain files are ever served. The narrated cuts, the clips
 * under `audio/` and `sfx/`, and everything in `marketing/archive/` carry
 * ElevenLabs free-plan audio, which is not licensed for commercial use
 * (see `marketing/reels/README.md`), so a list entry that points at one
 * fails here.
 */

import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

// The function Next writes `sitemap.xml` with, so the test reads what the built file says.
import { resolveSitemap } from "next/dist/build/webpack/loaders/metadata/resolve-route-data";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import sitemap from "../app/sitemap";
import { DemoCard, RealDemos } from "../components/landing/real-demos";
import { demos } from "./demos";
import { escapeXml } from "./feed";
import { mp4Seconds, mp4Size } from "./mp4-header";
import { absoluteUrl } from "./seo";
import { srtToVtt } from "./srt-to-vtt";
import { demoVideos, getVideo, videoCaptionsSrc, videoPath, videoPosterSrc, videoSrc, videos, watchVideos } from "./videos";

const SITE = fileURLToPath(new URL("../", import.meta.url));
const REPO = join(SITE, "..");
const VIDEOS_DIR = join(SITE, "public/videos");

const sha256 = (file: string) => createHash("sha256").update(readFileSync(file)).digest("hex");
const isNonEmptyFile = (path: string) => existsSync(path) && statSync(path).isFile() && statSync(path).size > 0;
const onDisk = (publicPath: string) => join(SITE, "public", publicPath);

/** The file names the list says `public/videos/` holds. */
function listedFiles(): string[] {
  return videos
    .flatMap((v) => [videoSrc(v.slug), videoPosterSrc(v.slug), v.captionsSource ? videoCaptionsSrc(v.slug) : ""])
    .filter(Boolean)
    .map((p) => p.replace(/^\/videos\//, ""));
}

describe("the list", () => {
  it("has the v0.7.0 video first, then the launch video, then the reels, newest first", () => {
    expect(videos.map((v) => v.slug)).toEqual([
      "whats-new-v070",
      "launch-video",
      "reel-v070-whats-new",
      "reel-v070-describe-the-edit",
      "reel-v070-200-tracks",
      "reel-highlights",
      "reel-teaser",
      "reel-beatmatched-transition",
      "reel-extended-club-intro",
      "reel-mini-mix",
    ]);
    expect(videos.map((v) => v.kind)).toEqual(["release", "launch", ...Array(8).fill("reel")]);
  });

  it("uses each slug once, in kebab-case", () => {
    const slugs = videos.map((v) => v.slug);
    expect(new Set(slugs).size).toBe(slugs.length);
    for (const s of slugs) expect(s).toMatch(/^[a-z0-9]+(?:-[a-z0-9]+)*$/);
  });

  it.each(videos.map((v) => [v.slug, v] as const))("%s says what it shows, and says it plainly", (_slug, v) => {
    expect(v.title.length).toBeGreaterThan(0);
    expect(v.caption.length).toBeGreaterThan(60);
    expect(v.description.length).toBeGreaterThan(40);
    expect(v.description.length).toBeLessThanOrEqual(160); // the length a search result shows
    // The watch page's title, then " · edytlab".
    expect(v.title.length).toBeLessThanOrEqual(55);
    // The rules in marketing/README.md ("The accuracy rules"): nothing
    // that has not shipped, no counts or ratings, no superlatives.
    // The launch video says on screen what it cannot do yet, and its
    // caption repeats that; it is the one place the words may appear.
    const words = `${v.title} ${v.caption} ${v.description} ${v.moments.map((m) => m.text).join(" ")}`.replace(
      /no stem separation or transcription yet/gi,
      "",
    );
    expect(words).not.toMatch(/\bthe only\b|\b(?:best|better than|ultimate)\b/i);
    expect(words).not.toMatch(/\bmashups?\b|vocals over|isolat|\bacapella\b|stem separation|\bstems\b/i);
    expect(words).not.toMatch(/\btranscri(?:be|bes|bed|ption|pt)\b|filler|speaker detection/i);
    expect(words).not.toMatch(/\b\d+\s+(?:\w+\s+){0,2}tools\b|\d+\+?\s*(?:users|downloads|stars)\b|\bratings?\b|trusted by/i);
    expect(v.uploadDate).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(v.uploadDate <= new Date().toISOString().slice(0, 10)).toBe(true);
    expect(v.uploadDate >= "2026-05-01").toBe(true);
    expect(Number.isInteger(v.seconds) && v.seconds > 0).toBe(true);
    expect(v.posterAt).toBeGreaterThan(0);
    expect(v.posterAt).toBeLessThan(v.seconds);
  });

  it("gives no video the name of another, a full demo included", () => {
    // They share a hub and a search result: a reel of a demo is "(short reel)", not a second copy of it.
    const names = watchVideos.map((v) => v.title);
    expect(new Set(names).size).toBe(names.length);
  });

  it("names a vertical reel 9:16 and the two long videos 16:9", () => {
    for (const v of videos) {
      const ratio = v.width / v.height;
      expect(ratio, v.slug).toBeCloseTo(v.kind === "reel" ? 9 / 16 : 16 / 9, 3);
    }
  });

  it("says, for every v0.7.0 video, that it is the real app with a scripted assistant", () => {
    // Every shot of app in those videos is labelled "v0.7.0 · real app ·
    // scripted assistant" (marketing/promo-v0.7.0/claims.md). A caption or
    // description that left that out would claim more than the video does.
    const promo = videos.filter((v) => v.source.startsWith("marketing/promo-v0.7.0/"));
    expect(promo.map((v) => v.slug)).toEqual([
      "whats-new-v070",
      "reel-v070-whats-new",
      "reel-v070-describe-the-edit",
      "reel-v070-200-tracks",
    ]);
    // "Five changes you can see" is the vertical what's-new reel's claim (claims.md, R1). The 16:9
    // cut shows the typed edit and six changes, so it carries no count of changes.
    const wide = promo.find((v) => v.slug === "whats-new-v070") as (typeof promo)[number];
    expect(`${wide.title} ${wide.caption} ${wide.description}`).not.toMatch(/\bfive\b/i);
    for (const v of promo) {
      expect(v.description, `${v.slug} description`).toMatch(/scripted assistant/i);
      expect(v.description, `${v.slug} description`).toMatch(/real (?:edytlab )?app/i);
      expect(v.caption, `${v.slug} caption`).toMatch(/real app, scripted assistant/i);
    }
  });
});

describe("public/videos", () => {
  it.each(videos.map((v) => [v.slug, v] as const))("has the video and its poster for %s", (slug, v) => {
    expect(isNonEmptyFile(onDisk(videoSrc(slug))), `missing public${videoSrc(slug)}`).toBe(true);
    expect(isNonEmptyFile(onDisk(videoPosterSrc(slug))), `missing public${videoPosterSrc(slug)}`).toBe(true);
    if (v.captionsSource) expect(isNonEmptyFile(onDisk(videoCaptionsSrc(slug))), `missing captions for ${slug}`).toBe(true);
  });

  it("holds nothing the list does not name", () => {
    const listed = new Set(listedFiles());
    const files = existsSync(VIDEOS_DIR) ? readdirSync(VIDEOS_DIR) : [];
    expect(files.filter((f) => !listed.has(f))).toEqual([]);
    expect([...listed].filter((f) => !files.includes(f))).toEqual([]);
  });

  it("holds no narrated cut and no clip with a voice or a sound effect on it", () => {
    for (const f of existsSync(VIDEOS_DIR) ? readdirSync(VIDEOS_DIR) : []) {
      expect(f, "an ElevenLabs free-plan file is not for commercial use").not.toMatch(/narrat|\.mp3$|\.wav$|sfx|audio/i);
    }
  });

  it("serves a poster that is a JPEG", () => {
    for (const v of videos) {
      const head = readFileSync(onDisk(videoPosterSrc(v.slug))).subarray(0, 3);
      expect([...head], `${v.slug} poster is not a JPEG`).toEqual([0xff, 0xd8, 0xff]);
    }
  });
});

describe("the copies match their sources under marketing/", () => {
  it.each(videos.map((v) => [v.slug, v] as const))("%s names a plain file that exists", (_slug, v) => {
    expect(v.source).toMatch(
      /^marketing\/(?:reels\/reel-[a-z-]+|launch-video\/out\/launch-[a-z]+|promo-v0\.7\.0\/out\/edytlab-v0\.7\.0-[a-z0-9-]+)\.mp4$/,
    );
    expect(v.source).not.toMatch(/narrated|picture-only|\/audio\/|\/sfx\/|archive|narration|1x1/i);
    expect(isNonEmptyFile(join(REPO, v.source)), `${v.source} is gone: was it re-cut and moved?`).toBe(true);
  });

  it.each(videos.map((v) => [v.slug, v] as const))("%s is byte-identical to its source", (slug, v) => {
    // A re-cut in marketing/ has to be copied here too (CLAUDE.md).
    expect(sha256(onDisk(videoSrc(slug))), `public${videoSrc(slug)} differs from ${v.source}: copy the re-cut`).toBe(
      sha256(join(REPO, v.source)),
    );
  });

  it("has captions that are the source's own, converted to WebVTT", () => {
    // The two 16:9 videos; the vertical reels show their words burned in
    // and have a cue file or an .srt for their moments instead.
    const withCaptions = videos.filter((v) => v.captionsSource);
    expect(withCaptions.map((v) => v.slug)).toEqual(["whats-new-v070", "launch-video"]);
    for (const v of withCaptions) {
      const source = v.captionsSource as string;
      expect(source).toMatch(
        /^marketing\/(?:launch-video\/out\/launch-[a-z]+|promo-v0\.7\.0\/out\/edytlab-v0\.7\.0-[a-z0-9]+)\.srt$/,
      );
      expect(source.replace(/\.srt$/, ".mp4"), "the captions are of another cut").toBe(v.source);
      expect(readFileSync(onDisk(videoCaptionsSrc(v.slug)), "utf8")).toBe(srtToVtt(readFileSync(join(REPO, source), "utf8")));
    }
  });
});

/** The start time of each cue of a SubRip file, in seconds, in order. */
function srtStarts(srt: string): number[] {
  return [...srt.matchAll(/^(\d{2}):(\d{2}):(\d{2})[,.](\d{3}) --> /gm)].map(
    (m) => Number(m[1]) * 3600 + Number(m[2]) * 60 + Number(m[3]) + Number(m[4]) / 1000,
  );
}

describe("what happens, in order, is the source's own", () => {
  it.each(videos.map((v) => [v.slug, v] as const))("%s has moments that follow the cues of its source", (_slug, v) => {
    expect(v.moments.length).toBeGreaterThan(2);
    const times = v.moments.map((m) => m.at);
    expect([...times].sort((a, b) => a - b)).toEqual(times);
    expect(new Set(times).size).toBe(times.length);
    for (const m of v.moments) {
      expect(m.text.length).toBeGreaterThan(8);
      expect(m.at).toBeGreaterThanOrEqual(0);
      expect(m.at).toBeLessThan(v.seconds);
    }

    const source = readFileSync(join(REPO, v.momentsSource), "utf8");
    if (v.momentsSource.endsWith("-cues.json")) {
      // A reel's cue file. Its moments are the words it shows, as written, one for each cue.
      expect(v.momentsSource).toBe(v.source.replace(/\.mp4$/, "-cues.json"));
      const cues = JSON.parse(source).cues as { start: number; onscreen: string }[];
      expect(v.moments, "a cue moved or changed: update `moments`").toEqual(
        cues.map((c) => ({ at: c.start, text: c.onscreen })),
      );
    } else {
      // The burned-in words, as an .srt. A moment describes them and may stand for several
      // cues, but each starts where a cue starts, so a re-cut that moves the words is noticed.
      expect(v.momentsSource).toBe(v.source.replace(/\.mp4$/, ".srt"));
      const starts = srtStarts(source).map((t) => Math.round(t * 1000));
      for (const m of v.moments) {
        expect(starts, `no cue starts at ${m.at} s: a caption moved, update \`moments\``).toContain(Math.round(m.at * 1000));
      }
      // The first moment is the first thing on screen.
      expect(Math.round(v.moments[0].at * 1000)).toBe(Math.min(...starts));
    }
  });

  it("gives a 16:9 video captions and a reel none", () => {
    for (const v of videos) expect(Boolean(v.captionsSource), v.slug).toBe(v.kind !== "reel");
  });
});

describe("the list's numbers are the file's own", () => {
  it.each(videos.map((v) => [v.slug, v] as const))("%s has the running time and the size its header states", (slug, v) => {
    const file = readFileSync(onDisk(videoSrc(slug)));
    const actual = mp4Seconds(file);
    expect(Math.abs(v.seconds - actual), `${slug}.mp4 is ${actual.toFixed(2)} s long`).toBeLessThan(1);
    expect(mp4Size(file), `${slug}.mp4 is not ${v.width}x${v.height}`).toEqual({ width: v.width, height: v.height });
  });
});

describe("srtToVtt", () => {
  it("writes the header and turns the comma in a time into a point", () => {
    const out = srtToVtt("1\n00:00:00,550 --> 00:00:04,300\nHello\n\n2\n00:00:04,350 --> 00:00:08,780\nWorld\n");
    expect(out.startsWith("WEBVTT\n\n")).toBe(true);
    expect(out).toContain("00:00:00.550 --> 00:00:04.300\nHello");
    expect(out).toContain("00:00:04.350 --> 00:00:08.780\nWorld");
  });

  it("leaves a comma in the words alone", () => {
    expect(srtToVtt("1\n00:00:01,000 --> 00:00:02,000\nOne, two\n")).toContain("One, two");
  });

  it("takes Windows line endings and a byte-order mark", () => {
    const out = srtToVtt("﻿1\r\n00:00:01,000 --> 00:00:02,000\r\nHi\r\n");
    expect(out).toContain("00:00:01.000 --> 00:00:02.000\nHi");
    expect(out).not.toContain("\r");
    expect(out).not.toContain("﻿");
  });
});

describe("the video sitemap entries", () => {
  const entries = sitemap();
  const watchUrls = new Set(watchVideos.map((v) => absoluteUrl(videoPath(v.slug))));

  it.each(watchVideos.map((v) => [v.slug, v] as const))("%s's page declares exactly its one video", (_slug, v) => {
    const entry = entries.find((e) => e.url === absoluteUrl(videoPath(v.slug)));
    expect(entry, `${videoPath(v.slug)} is not in the sitemap`).toBeDefined();
    // The fields come from the list, as the page's VideoObject does. Next writes them
    // into the XML as they are, so the text is escaped.
    expect(entry?.videos).toEqual([
      {
        title: escapeXml(v.title),
        thumbnail_loc: absoluteUrl(videoPosterSrc(v.slug)),
        description: escapeXml(v.description),
        content_loc: absoluteUrl(videoSrc(v.slug)),
        duration: v.seconds,
      },
    ]);
    // Google's limits: a title of 100 characters, a description of 2048, a length of 1 to 28800 seconds.
    expect(v.title.length).toBeLessThanOrEqual(100);
    expect(v.description.length).toBeLessThanOrEqual(2048);
    expect(v.seconds).toBeGreaterThanOrEqual(1);
    expect(v.seconds).toBeLessThanOrEqual(28800);
  });

  it("gives no other page a video: not the hub, not the pages that play the demos", () => {
    const withVideos = entries.filter((e) => e.videos?.length).map((e) => e.url);
    expect(new Set(withVideos)).toEqual(watchUrls);
    expect(withVideos.length).toBe(watchVideos.length);
  });

  it("writes the video namespace and one <video:video> per watch page into sitemap.xml", () => {
    const xml: string = resolveSitemap(entries);
    expect(xml).toContain('xmlns:video="http://www.google.com/schemas/sitemap-video/1.1"');
    expect(xml.match(/<video:video>/g)?.length).toBe(watchVideos.length);
    for (const v of watchVideos) {
      const block = xml.slice(xml.indexOf(`<loc>${absoluteUrl(videoPath(v.slug))}</loc>`));
      const own = block.slice(0, block.indexOf("</url>"));
      expect(own, v.slug).toContain(`<video:title>${escapeXml(v.title)}</video:title>`);
      expect(own, v.slug).toContain(`<video:thumbnail_loc>${absoluteUrl(videoPosterSrc(v.slug))}</video:thumbnail_loc>`);
      expect(own, v.slug).toContain(`<video:content_loc>${absoluteUrl(videoSrc(v.slug))}</video:content_loc>`);
      expect(own, v.slug).toContain(`<video:duration>${v.seconds}</video:duration>`);
      expect(own.match(/<video:video>/g)?.length, v.slug).toBe(1);
    }
    // Well-formed: nothing the XML cannot hold is left unescaped in the text.
    expect(xml.replace(/<[^>]+>/g, "")).not.toMatch(/&(?!amp;|lt;|gt;|quot;|apos;)/);
  });
});

describe("the three full demos have pages of their own", () => {
  it("are the demos in lib/demos.ts, in the same order, with the same words", () => {
    expect(demoVideos.map((d) => d.slug)).toEqual(demos.map((d) => d.slug));
    demoVideos.forEach((v, i) => {
      expect(v.kind).toBe("demo");
      expect(v.title).toBe(demos[i].title);
      expect(v.caption).toBe(demos[i].caption);
      expect(v.description).toBe(demos[i].description);
      expect(v.seconds).toBe(demos[i].seconds);
      expect(v.uploadDate).toBe(demos[i].uploadDate);
    });
  });

  it("list every video with a page, in the hub's order, and look each one up", () => {
    expect(watchVideos.map((v) => v.slug)).toEqual([
      "whats-new-v070",
      "launch-video",
      "dj-beatmatched-transition",
      "dj-extended-club-intro",
      "dj-mini-mix",
      "reel-v070-whats-new",
      "reel-v070-describe-the-edit",
      "reel-v070-200-tracks",
      "reel-highlights",
      "reel-teaser",
      "reel-beatmatched-transition",
      "reel-extended-club-intro",
      "reel-mini-mix",
    ]);
    for (const v of watchVideos) expect(getVideo(v.slug)).toBe(v);
  });

  it.each(demoVideos.map((d) => [d.slug, d] as const))("%s is served from public/demos/, not copied into public/videos/", (slug) => {
    expect(videoSrc(slug)).toBe(`/demos/${slug}.mp4`);
    expect(videoPosterSrc(slug)).toBe(`/demos/${slug}.jpg`);
    expect(isNonEmptyFile(join(SITE, "public", videoSrc(slug)))).toBe(true);
    expect(isNonEmptyFile(join(SITE, "public", videoPosterSrc(slug)))).toBe(true);
    expect(existsSync(join(VIDEOS_DIR, `${slug}.mp4`)), "a demo copied into public/videos/").toBe(false);
    expect(existsSync(join(VIDEOS_DIR, `${slug}.jpg`)), "a demo poster copied into public/videos/").toBe(false);
  });

  it.each(demoVideos.map((d) => [d.slug, d] as const))("%s has the running time and the size its header states", (slug, v) => {
    const file = readFileSync(join(SITE, "public", videoSrc(slug)));
    expect(Math.abs(v.seconds - mp4Seconds(file))).toBeLessThan(1);
    expect(mp4Size(file)).toEqual({ width: v.width, height: v.height });
  });

  it.each(demoVideos.map((d) => [d.slug, d] as const))("%s's moments are the cue file's own words, one for each cue", (_slug, v) => {
    // `demo-<slug>-cues.json` has the caption cues of the demo video; its `narration` lines belong to an earlier take and are not used.
    expect(v.momentsSource).toBe(`marketing/reels/demo-${v.slug}-cues.json`);
    const cues = JSON.parse(readFileSync(join(REPO, v.momentsSource), "utf8")).cues as { start: number; onscreen: string }[];
    expect(v.moments, "a cue moved or changed: update `moments`").toEqual(cues.map((c) => ({ at: c.start, text: c.onscreen })));
    expect(v.moments.length).toBeGreaterThan(2);
    expect(v.moments[v.moments.length - 1].at).toBeLessThan(v.seconds);
  });
});

describe("only a video's own page carries its VideoObject", () => {
  // Google indexes a video only on a page where it is the main content ("Video is not the main
  // content of the page" otherwise), so no page but a watch page may carry one: not the hub, not
  // the home page, not the DJ page that plays the demos.
  const code = (src: string) => src.replace(/\/\*[\s\S]*?\*\//g, "").replace(/^\s*\/\/.*$/gm, "");

  /** Every authored `.ts` / `.tsx` file under `dir`, without tests, as `[path from website/, code]`. */
  function sources(dir: string): [string, string][] {
    return readdirSync(join(SITE, dir)).flatMap((name) => {
      const rel = `${dir}/${name}`;
      if (statSync(join(SITE, rel)).isDirectory()) return name === "node_modules" || name === ".next" ? [] : sources(rel);
      return /\.tsx?$/.test(name) && !/\.test\.tsx?$/.test(name) ? [[rel, code(readFileSync(join(SITE, rel), "utf8"))] as [string, string]] : [];
    });
  }

  it("is made in one place, and rendered in one: the player of a watch page", () => {
    const all = [...sources("app"), ...sources("components")];
    expect(all.length).toBeGreaterThan(30);
    const callers = all.filter(([, src]) => src.includes("videoObjectJsonLd(")).map(([f]) => f);
    expect(callers).toEqual(["components/landing/site-video.tsx"]);
    const players = all.filter(([, src]) => src.includes("<WatchPlayer")).map(([f]) => f);
    expect(players).toEqual(["app/videos/[slug]/page.tsx"]);
    // And the markup type is named nowhere else in the pages and components.
    expect(all.filter(([, src]) => /VideoObject/.test(src)).map(([f]) => f)).toEqual([]);
  });

  it("is not in the markup of the demo cards, which play on the home page and the DJ page", () => {
    const demo = demos[0];
    const card = renderToStaticMarkup(createElement(DemoCard, { demo, index: 0 }));
    expect(card).toContain("<video");
    expect(card).not.toContain("application/ld+json");
    expect(card).not.toContain("VideoObject");
    // They link to the page that has it.
    expect(card).toContain(`href="/videos/${demo.slug}"`);
    expect(card).toContain("Watch on its own page");

    const section = renderToStaticMarkup(createElement(RealDemos));
    expect(section.match(/<video\b/g)?.length).toBe(demos.length);
    expect(section).not.toContain("application/ld+json");
    for (const d of demos) expect(section).toContain(`href="/videos/${d.slug}"`);
  });
});
