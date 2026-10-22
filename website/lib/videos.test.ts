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

import { describe, expect, it } from "vitest";

import { demos } from "./demos";
import { mp4Seconds, mp4Size } from "./mp4-header";
import { srtToVtt } from "./srt-to-vtt";
import { videoCaptionsSrc, videoPosterSrc, videoSrc, videos } from "./videos";

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
  it("has the launch video and the five plain reels, in that order", () => {
    expect(videos.map((v) => v.slug)).toEqual([
      "launch-video",
      "reel-highlights",
      "reel-teaser",
      "reel-beatmatched-transition",
      "reel-extended-club-intro",
      "reel-mini-mix",
    ]);
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
    // They share a page and a search result: a reel of a demo is "the short cut", not a second copy of it.
    const names = [...videos.map((v) => v.title), ...demos.map((d) => d.title)];
    expect(new Set(names).size).toBe(names.length);
  });

  it("names a vertical reel 9:16 and the launch video 16:9", () => {
    for (const v of videos) {
      const ratio = v.width / v.height;
      expect(ratio, v.slug).toBeCloseTo(v.kind === "launch" ? 16 / 9 : 9 / 16, 3);
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
    expect(v.source).toMatch(/^marketing\/(?:reels\/reel-[a-z-]+|launch-video\/out\/launch-[a-z]+)\.mp4$/);
    expect(v.source).not.toMatch(/narrated|picture-only|\/audio\/|\/sfx\/|archive|narration/i);
    expect(isNonEmptyFile(join(REPO, v.source)), `${v.source} is gone: was it re-cut and moved?`).toBe(true);
  });

  it.each(videos.map((v) => [v.slug, v] as const))("%s is byte-identical to its source", (slug, v) => {
    // A re-cut in marketing/ has to be copied here too (CLAUDE.md).
    expect(sha256(onDisk(videoSrc(slug))), `public${videoSrc(slug)} differs from ${v.source}: copy the re-cut`).toBe(
      sha256(join(REPO, v.source)),
    );
  });

  it("has captions that are the source's own, converted to WebVTT", () => {
    const withCaptions = videos.filter((v) => v.captionsSource);
    expect(withCaptions.map((v) => v.slug)).toEqual(["launch-video"]);
    for (const v of withCaptions) {
      const source = v.captionsSource as string;
      expect(source).toMatch(/^marketing\/launch-video\/out\/launch-[a-z]+\.srt$/);
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
  it.each(videos.map((v) => [v.slug, v] as const))("%s has one moment for each cue of its source", (_slug, v) => {
    expect(v.moments.length).toBeGreaterThan(2);
    const times = v.moments.map((m) => m.at);
    expect([...times].sort((a, b) => a - b)).toEqual(times);
    for (const m of v.moments) {
      expect(m.text.length).toBeGreaterThan(8);
      expect(m.at).toBeGreaterThanOrEqual(0);
      expect(m.at).toBeLessThan(v.seconds);
    }

    if (v.kind === "launch") {
      // The cues are the video's captions: one moment for each.
      const starts = srtStarts(readFileSync(join(REPO, v.captionsSource as string), "utf8"));
      expect(v.moments.map((m) => m.at), "a caption moved or was added: update `moments`").toEqual(starts);
    } else {
      // The cues are the reel's cue file. A reel's moments are the words it shows, as written.
      expect(v.cuesSource).toBe(v.source.replace(/\.mp4$/, "-cues.json"));
      const cues = JSON.parse(readFileSync(join(REPO, v.cuesSource as string), "utf8")).cues as {
        start: number;
        onscreen: string;
      }[];
      expect(v.moments, "a cue moved or changed: update `moments`").toEqual(
        cues.map((c) => ({ at: c.start, text: c.onscreen })),
      );
    }
  });

  it("gives only the reels a cue file, and the launch video its captions", () => {
    for (const v of videos) {
      expect(Boolean(v.cuesSource), v.slug).toBe(v.kind === "reel");
      expect(Boolean(v.captionsSource), v.slug).toBe(v.kind === "launch");
    }
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
