/**
 * The demo manifest against the files it names.
 *
 * A missing video is invisible to the build: Next serves `public/` as
 * it finds it, so an entry whose `.mp4` or poster never landed renders
 * a card that 404s, and an `.mp4` dropped into the folder without an
 * entry ships bytes no page plays. Both are checked here, against the
 * real folder rather than a fixture.
 */

import { existsSync, readdirSync, statSync } from "node:fs";
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
