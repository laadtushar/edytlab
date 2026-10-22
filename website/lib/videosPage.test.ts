/**
 * What a visitor reads on /videos and on each video's page.
 *
 * Both pages were written as notes about how the videos were made
 * ("Every app shot is the real v0.7.0 frontend with a scripted assistant",
 * "Nothing plays until you press play", a card labelled "LAUNCH VIDEO"
 * over a title that began "Launch video:"). A public page says what you
 * will see and leaves the making-of to one small line, which is data
 * (`disclosure` in `lib/videos.ts`) so a re-recording changes the line
 * and not the layout. These tests hold the pages to that, and keep the
 * words under a video's name in the data where a re-recording can reach
 * them.
 *
 * They render the real pages on the server, as a crawler and the first
 * paint see them.
 */

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import VideoPage from "../app/videos/[slug]/page";
import VideosPage from "../app/videos/page";
import { FeaturedVideo } from "../components/landing/featured-video";
import {
  cardTitle,
  demoVideos,
  forViewers,
  formatDuration,
  launchVideo,
  relatedVideos,
  releaseVideo,
  reels,
  videoPath,
  videos,
  watchVideos,
} from "./videos";

const ROOT = fileURLToPath(new URL("../", import.meta.url));

/** The text of some markup, as a reader sees it. */
const textOf = (html: string) =>
  html
    .replace(/<script[\s\S]*?<\/script>/g, " ")
    .replace(/<[^>]+>/g, " ")
    .replace(/&#x27;/g, "'")
    .replace(/&quot;/g, '"')
    .replace(/&amp;/g, "&")
    .replace(/\s+/g, " ");

/** What a card or a page may not say: the making-of, in a visitor's face. */
const INTERNAL = /Nothing plays|\bfrontend\b|\(#\d+\)|\bTODO\b|mock-up/i;

describe("the words under a video's name are data", () => {
  it.each(watchVideos.map((v) => [v.slug, v] as const))("%s has a one-line blurb and a one-line disclosure", (_slug, v) => {
    expect(v.blurb.length).toBeGreaterThanOrEqual(30);
    expect(v.blurb.length).toBeLessThanOrEqual(110);
    expect(v.blurb).toMatch(/[.]$/);
    // A card shows the length on its poster and the page says "vertical" in its shape: the line says what happens.
    expect(v.blurb).not.toMatch(/^An? (?:\d+-second|one-minute|fifteen-second|vertical|screen recording)/i);
    expect(v.blurb).not.toMatch(/\bvideo of\b/i);
    expect(v.disclosure.length).toBeGreaterThanOrEqual(20);
    expect(v.disclosure.length).toBeLessThanOrEqual(110);
    expect(v.disclosure).toMatch(/[.]$/);
    expect(`${v.blurb} ${v.disclosure}`).not.toMatch(INTERNAL);
    // The accuracy rules of marketing/README.md, as videos.test.ts holds the caption to them.
    const words = `${v.blurb} ${v.disclosure}`;
    expect(words).not.toMatch(/\\bthe only\\b|\\b(?:best|better than|ultimate)\\b/i);
    expect(words).not.toMatch(/\\bmashups?\\b|vocals over|isolat|stem separation|\\bstems\\b|\\btranscri|filler/i);
  });

  it("says 'real Claude (Sonnet 5.5)' for every v0.7.0 video, and for no other, and 'scripted' for none", () => {
    for (const v of videos) {
      const promo = v.source.startsWith("marketing/promo-v0.7.0/");
      expect(/real Claude \(Sonnet 5\.5\)/.test(v.disclosure), `${v.slug}: ${v.disclosure}`).toBe(promo);
      expect(/scripted/i.test(v.disclosure), `${v.slug}: ${v.disclosure}`).toBe(false);
    }
  });

  it("gives one blurb to a video: no two cards say the same thing", () => {
    const blurbs = watchVideos.map((v) => v.blurb);
    expect(new Set(blurbs).size).toBe(blurbs.length);
  });

  it("titles the launch video for what it shows, not 'Launch video:'", () => {
    expect(launchVideo.title).not.toMatch(/^launch video/i);
    expect(launchVideo.title.length).toBeLessThanOrEqual(55);
  });
});

describe("the helpers", () => {
  it("drops '(short reel)' from a card's title, and only that", () => {
    expect(cardTitle({ title: "A three-track mini-mix (short reel)" } as never)).toBe("A three-track mini-mix");
    expect(cardTitle({ title: "Beatmatch and blend two tracks" } as never)).toBe("Beatmatch and blend two tracks");
    expect(cardTitle({ title: "What's new in v0.7.0 (short reel)" } as never)).toBe("What's new in v0.7.0");
  });

  it("drops tracker numbers from a caption and keeps the rest of the sentence", () => {
    expect(forViewers("Faders that stay where you put them (#596); the history graph (#602, #603) keeps up.")).toBe(
      "Faders that stay where you put them; the history graph keeps up.",
    );
    expect(forViewers("No numbers here.")).toBe("No numbers here.");
  });

  it("offers three videos of the same shape, four for a reel, never the video itself", () => {
    for (const v of watchVideos) {
      const related = relatedVideos(v);
      const vertical = v.height > v.width;
      expect(related, v.slug).toHaveLength(v.kind === "reel" ? 4 : 3);
      expect(related.map((r) => r.slug), v.slug).not.toContain(v.slug);
      expect(new Set(related.map((r) => r.slug)).size).toBe(related.length);
      for (const r of related) expect(r.height > r.width, `${v.slug} -> ${r.slug}`).toBe(vertical);
    }
  });

  it("leads each demo to the other demos first, and each reel to the reels after it", () => {
    const [a, b, c] = demoVideos;
    expect(relatedVideos(a).slice(0, 2).map((v) => v.slug)).toEqual([b.slug, c.slug]);
    expect(relatedVideos(reels[0]).map((v) => v.slug)).toEqual(reels.slice(1, 5).map((v) => v.slug));
    // Wrapping round: the last reel is followed by the first ones.
    expect(relatedVideos(reels[reels.length - 1])[0].slug).toBe(reels[0].slug);
  });
});

describe("/videos", () => {
  const html = renderToStaticMarkup(createElement(VideosPage));
  const body = html.replace(/<script[\s\S]*?<\/script>/g, " ");
  const text = textOf(html);

  it("opens with a headline about what you will see, an action, and the featured video", () => {
    expect(body.match(/<h1\b/g)).toHaveLength(1);
    expect(body).toContain("Watch edytlab edit audio from a sentence");
    expect(body.indexOf("Download edytlab")).toBeGreaterThan(-1);
    expect(body.indexOf("Download edytlab")).toBeLessThan(body.indexOf("Full walkthroughs"));
    expect(body.indexOf("Read the docs")).toBeLessThan(body.indexOf("Full walkthroughs"));
    // The featured video comes before every section, with its title linking to its own page.
    expect(body.indexOf(launchVideo.title)).toBeGreaterThan(-1);
    expect(body.indexOf(launchVideo.title)).toBeLessThan(body.indexOf("Full walkthroughs"));
    expect(body).toContain(`href="${videoPath(launchVideo.slug)}"`);
    expect(text).toContain(launchVideo.blurb);
    expect(text).toContain(launchVideo.disclosure);
  });

  it("has the sections a visitor expects, in order, each with a short heading", () => {
    const order = ["Full walkthroughs", "The latest release", "Short clips", "Try it on your own audio"].map((h) => body.indexOf(`>${h}<`));
    for (const i of order) expect(i).toBeGreaterThan(-1);
    expect([...order].sort((x, y) => x - y)).toEqual(order);
    const headings = [...body.matchAll(/<h2\b[^>]*>([\s\S]*?)<\/h2>/g)].map((m) => textOf(m[1]).trim());
    expect(headings).toEqual([launchVideo.title, "Full walkthroughs", "The latest release", "Short clips", "Try it on your own audio"]);
  });

  it("lists every video once as a card, except the featured one, whose title is its heading", () => {
    for (const v of [...demoVideos, releaseVideo, ...reels]) {
      expect(body.match(new RegExp(`href="${videoPath(v.slug)}"`, "g")), v.slug).toHaveLength(1);
      expect(text, v.slug).toContain(v.blurb);
      expect(text, v.slug).toContain(v.disclosure);
      expect(text, v.slug).toContain(formatDuration(v.seconds));
    }
    // Two links to the featured video: its poster and its title.
    expect(body.match(new RegExp(`href="${videoPath(launchVideo.slug)}"`, "g"))).toHaveLength(2);
  });

  it("puts no type label over a card and no 'Demo 01' / 'Reel 02' counter", () => {
    expect(text).not.toMatch(/\b(?:Demo|Reel) 0\d\b/);
    expect(text).not.toMatch(/\bLaunch video\b/i);
    expect(body).not.toMatch(/uppercase[^"]*"[^>]*>\s*(?:<span[^>]*><\/span>)?\s*<span>(?:Demo|Reel|Launch video|v0\.7\.0)/i);
    // A reel's card is titled for what happens, not "(short reel)".
    expect(text).not.toMatch(/\(short reel\)/i);
  });

  it("reads as public copy: no making-of in the sentences, only the one disclosure under each video", () => {
    const withoutDisclosures = watchVideos.reduce((t, v) => t.split(v.disclosure).join(" "), text);
    expect(withoutDisclosures).not.toMatch(INTERNAL);
    expect(withoutDisclosures).not.toMatch(/scripted|synthetic|sped up/i);
  });

  it("holds no <video> in its markup: the featured poster is a link until it is pressed", () => {
    expect(body).not.toMatch(/<video\b/);
    expect(html).not.toContain("VideoObject");
    const featured = renderToStaticMarkup(
      createElement(FeaturedVideo, {
        href: "/videos/x",
        title: "X",
        src: "/videos/x.mp4",
        poster: "/videos/x.jpg",
        captions: "/videos/x.vtt",
        width: 1280,
        height: 720,
      }),
    );
    expect(featured).not.toMatch(/<video\b/);
    expect(featured).toContain('href="/videos/x"');
    expect(featured).toContain('aria-label="Play the video: X"');
    expect(featured).toMatch(/<img [^>]*src="\/videos\/x\.jpg"[^>]*width="1280"[^>]*height="720"/);
    expect(featured).toMatch(/fetchpriority="high"/i);
  });

  it("starts a video only when it is pressed", () => {
    const source = readFileSync(join(ROOT, "components/landing/featured-video.tsx"), "utf8")
      .replace(/\/\*[\s\S]*?\*\//g, "")
      .replace(/^\s*\/\/.*$/gm, "");
    // autoPlay is on the player that replaces the poster after a press, and nowhere before it.
    expect(source.indexOf("setPlaying(true)")).toBeGreaterThan(-1);
    expect(source.match(/<video\b/g)).toHaveLength(1);
    expect(source).toContain("const [playing, setPlaying] = useState(false)");
    expect(source).toContain("if (playing)");
    expect(source.indexOf("autoPlay")).toBeGreaterThan(source.indexOf("if (playing)"));
    // A new-tab click is left alone.
    expect(source).toMatch(/metaKey \|\| event\.ctrlKey/);
    expect(source).not.toMatch(/<track[^>]*\bdefault\b/);
  });

  it("fills its grids: three walkthroughs across, reels two across on a phone and four from lg, last row centred", () => {
    expect(body).toContain("md:grid-cols-3");
    expect(demoVideos).toHaveLength(3);
    expect(body).toMatch(/flex flex-wrap justify-center gap-4/);
    expect(body).toContain("w-[calc((100%-1rem)/2)]");
    expect(body).toContain("lg:w-[calc((100%-3rem)/4)]");
    expect(reels.length % 4 === 0 || reels.length % 2 === 0).toBe(true);
  });

  it("closes with a way to download, and the two limits a downloader meets", () => {
    const close = text.slice(text.indexOf("Try it on your own audio"));
    expect(close).toContain("free and open source (MIT) for macOS, Windows and Linux");
    expect(close).toContain("Download edytlab");
    expect(close).toContain("Read the docs");
    expect(close).toContain("unsigned");
    expect(close).toMatch(/Stem separation and transcription are not built yet/);
  });
});

describe("each video's page", () => {
  it.each(watchVideos.map((v) => [v.slug, v] as const))("%s reads as public copy, with the player first", async (_slug, video) => {
    const html = renderToStaticMarkup(await VideoPage({ params: Promise.resolve({ slug: video.slug }) }));
    const body = html.replace(/<script[\s\S]*?<\/script>/g, " ");
    const text = textOf(html);

    expect(body.match(/<h1\b/g)).toHaveLength(1);
    // The heading is the title a visitor reads; "(short reel)" stays in <title>, where it keeps titles unique.
    const h1 = body.slice(body.indexOf("<h1"), body.indexOf("</h1>") + "</h1>".length);
    expect(h1).toContain(`>${cardTitle(video).replace(/'/g, "&#x27;")}<`);
    expect(h1).not.toContain("short reel");
    expect(text).toContain(video.blurb);
    // One player, one VideoObject, and the player before every section.
    expect(body.match(/<video\b/g)).toHaveLength(1);
    expect(html.match(/"@type":"VideoObject"/g)).toHaveLength(1);
    expect(body.indexOf("<video")).toBeLessThan(body.indexOf("About this video"));
    // The one line of honesty is under the player, with the length.
    expect(text).toContain(`${formatDuration(video.seconds)}`);
    expect(text).toContain(video.disclosure);
    // No notes to ourselves, no tracker numbers, and the page's own words are a visitor's.
    expect(text.split(video.disclosure).join(" ")).not.toMatch(INTERNAL);
    expect(text).not.toContain("Nothing plays");
    expect(text).not.toMatch(/\(#\d+/);
    // What happens, as a list a viewer can follow.
    expect(text).toContain("What you will see");
    expect(body.match(/<li class="flex gap-4/g)).toHaveLength(video.moments.length);
    // More to watch: cards that link to pages, not players, then the way to download.
    const more = body.slice(body.indexOf("More to watch"));
    expect(more).not.toMatch(/<video\b/);
    for (const r of relatedVideos(video)) expect(more).toContain(`href="${videoPath(r.slug)}"`);
    expect(more).toContain('href="/videos"');
    expect(more).toContain("Download edytlab");
  });
});
