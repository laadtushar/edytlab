/**
 * The demo manifest against the files it names.
 *
 * A missing video is invisible to the build: Next serves `public/` as
 * it finds it, so an entry whose `.mp4` or poster never landed renders
 * a card that 404s, and an `.mp4` dropped into the folder without an
 * entry ships bytes no page plays. Both are checked here, against the
 * real folder rather than a fixture.
 */

import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import { demos } from "./demos";

const DEMOS_DIR = fileURLToPath(new URL("../public/demos/", import.meta.url));

/** True for a non-empty regular file, so a zero-byte placeholder fails. */
function isNonEmptyFile(path: string) {
  return existsSync(path) && statSync(path).isFile() && statSync(path).size > 0;
}

describe("demos manifest", () => {
  it("has at least one demo", () => {
    expect(demos.length).toBeGreaterThan(0);
  });

  it("uses each slug once", () => {
    const slugs = demos.map((d) => d.slug);
    expect(new Set(slugs).size).toBe(slugs.length);
  });

  it.each(demos.map((d) => d.slug))("uses a kebab-case slug: %s", (slug) => {
    expect(slug).toMatch(/^[a-z0-9]+(?:-[a-z0-9]+)*$/);
  });
});

describe("public/demos", () => {
  it.each(demos.map((d) => d.slug))("has the video and poster for %s", (slug) => {
    expect(isNonEmptyFile(join(DEMOS_DIR, `${slug}.mp4`)), `missing public/demos/${slug}.mp4`).toBe(true);
    expect(isNonEmptyFile(join(DEMOS_DIR, `${slug}.jpg`)), `missing public/demos/${slug}.jpg`).toBe(true);
  });

  it("holds no video the manifest does not list", () => {
    const listed = new Set(demos.map((d) => `${d.slug}.mp4`));
    const videos = existsSync(DEMOS_DIR)
      ? readdirSync(DEMOS_DIR).filter((f) => f.toLowerCase().endsWith(".mp4"))
      : [];
    expect(videos.filter((f) => !listed.has(f))).toEqual([]);
  });
});

/**
 * The running time stored in an MP4's own header: `moov` > `mvhd` holds a
 * timescale and a duration in those units. Read here so the manifest's
 * length is checked against the file with no tool installed.
 */
function mp4Seconds(file: Buffer): number {
  /** The first box of `type` between `start` and `end`, as `[payload start, box end]`. */
  const find = (start: number, end: number, type: string): [number, number] | null => {
    let at = start;
    while (at + 8 <= end) {
      let size = file.readUInt32BE(at);
      let header = 8;
      if (size === 1) {
        size = Number(file.readBigUInt64BE(at + 8));
        header = 16;
      } else if (size === 0) {
        size = end - at;
      }
      if (size < header) return null;
      if (file.toString("latin1", at + 4, at + 8) === type) return [at + header, at + size];
      at += size;
    }
    return null;
  };
  const moov = find(0, file.length, "moov");
  const mvhd = moov && find(moov[0], moov[1], "mvhd");
  if (!mvhd) throw new Error("no moov/mvhd box: not an MP4 this test can read");
  const at = mvhd[0];
  const version = file.readUInt8(at);
  const timescale = file.readUInt32BE(at + (version === 1 ? 20 : 12));
  const duration = version === 1 ? Number(file.readBigUInt64BE(at + 24)) : file.readUInt32BE(at + 16);
  return duration / timescale;
}

describe("demo structured data fields", () => {
  it.each(demos.map((d) => [d.slug, d] as const))("%s names what a VideoObject needs", (_slug, demo) => {
    expect(demo.description.length).toBeGreaterThan(40);
    expect(demo.description.length).toBeLessThanOrEqual(300);
    expect(demo.description).not.toMatch(/\b(?:best|only|better than|stem separation|transcri)/i);
    expect(demo.uploadDate).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(Number.isNaN(new Date(demo.uploadDate).getTime())).toBe(false);
    // Not in the future, and not before there was a site.
    expect(demo.uploadDate <= new Date().toISOString().slice(0, 10)).toBe(true);
    expect(demo.uploadDate >= "2026-05-01").toBe(true);
    expect(Number.isInteger(demo.seconds) && demo.seconds > 0).toBe(true);
  });

  it.each(demos.map((d) => [d.slug, d] as const))("%s states the length the video has", (_slug, demo) => {
    const actual = mp4Seconds(readFileSync(join(DEMOS_DIR, `${demo.slug}.mp4`)));
    // Rounded to the second, as the card shows it.
    expect(Math.abs(demo.seconds - actual), `${demo.slug}.mp4 is ${actual.toFixed(2)} s long`).toBeLessThan(1);
    if (demo.duration) {
      const shown = `${Math.floor(demo.seconds / 60)}:${String(demo.seconds % 60).padStart(2, "0")}`;
      expect(demo.duration, "the card's running time disagrees with `seconds`").toBe(shown);
    }
  });
});
