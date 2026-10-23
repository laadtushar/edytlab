/**
 * Copy that says something about other products, or about what edytlab
 * can do, has to be something a reader can check.
 *
 * Named comparison pages are coming, and they only stay honest if the
 * pages that already exist are. These are the claims that were found
 * without a source (or, for the table, contradicted by a competitor's
 * own help page), and the shapes they had, so a new page cannot bring
 * one back under different words without a test noticing:
 *
 * - verdicts on a class of products ("AI tools: shallow", "all upload
 *   your audio") with no product named and nothing to check them by;
 * - superlatives in a title ("the only approach that matters", "the
 *   best tools");
 * - a capability the app does not have (a mashup of one track's vocals
 *   over another's drums, stems) sold as a feature, or a feature that
 *   waits on something unshipped with the limit left out of its
 *   headline.
 *
 * The rules are in `marketing/README.md` ("The accuracy rules") and the
 * SEO research behind them. This file does not replace reading them: it
 * pins the specific sentences that were wrong.
 */

import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import { posts, type Block } from "./blog";

const ROOT = fileURLToPath(new URL("../", import.meta.url));
const REPO = join(ROOT, "..");
const SELF = "honest-claims.test.ts";

function read(rel: string): string {
  return readFileSync(join(ROOT, rel), "utf8");
}

/** A source file without its comments: a comment may say what a thing used to say. */
function code(rel: string): string {
  return read(rel)
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/^\s*\/\/.*$/gm, "");
}

/** Every authored `.ts` / `.tsx` file of the site, as `[path relative to website/, source]`. */
function siteSources(dir = ROOT): [string, string][] {
  const out: [string, string][] = [];
  for (const name of readdirSync(dir)) {
    if (name === "node_modules" || name === ".next" || name === "public") continue;
    const path = join(dir, name);
    if (statSync(path).isDirectory()) out.push(...siteSources(path));
    else if (/\.tsx?$/.test(name) && name !== SELF) {
      out.push([path.slice(ROOT.length), readFileSync(path, "utf8")]);
    }
  }
  return out;
}

function textOf(blocks: Block[]): string {
  return blocks.map((b) => (b.type === "ul" || b.type === "ol" ? b.items.join(" ") : b.text)).join("\n");
}

describe("the landing comparison", () => {
  const src = code("components/landing/comparison.tsx");

  it("says nothing about products it does not name", () => {
    // Two columns of category verdicts ("Traditional DAW", "AI Audio
    // Tools") had no source. "MCP extensibility: no" for AI tools is
    // contradicted by Descript's own help page, which says prompts can
    // be sent "via the Descript MCP".
    expect(src).not.toMatch(/Traditional DAW|AI Audio Tools/);
    expect(src).not.toMatch(/\bdaw:|\baiTool:/);
    expect(src).not.toContain("100+ hours");
    expect(src).not.toMatch(/label:\s*"(?:Audio quality|Learning curve)"/);
  });

  it("names no competitor", () => {
    const names = /Descript|Audacity|Adobe|Audition|Moises|LALAL|Auphonic|iZotope|REAPER|Pro Tools|Ableton|Logic Pro|GarageBand|Riverside|Cleanvoice/i;
    expect(src).not.toMatch(names);
  });

  it("still states the thing edytlab does not do yet", () => {
    expect(src).toMatch(/Stem separation/);
    expect(src).toMatch(/Not yet/);
    // Transcription shipped in v0.8.0 (#384): its row says so, not "Not yet".
    const row = src.match(/label: "Transcription and text-based editing",\s*\n\s*edytlab: "([^"]+)"/)?.[1];
    expect(row, "the transcription row moved; update this test").toBe("yes");
  });

  it("does not hang the section on a verdict about other tools", () => {
    expect(src).not.toMatch(/slow to learn|are easy but shallow|but shallow/i);
  });
});

describe("the feature grid", () => {
  const src = code("components/landing/feature-grid.tsx");

  it("puts the limit of text-based editing in its headline", () => {
    // The body says the speech model is a one-time download (#384); the
    // headline is what snippets and assistants quote, so it has to say it too.
    const title = src.match(/title:\s*"([^"]+)",\s*\n\s*body:\s*"Transcribe a take on your computer/)?.[1];
    expect(title, "the text-editing card moved; update this test").toBeDefined();
    expect(title).toMatch(/download/i);
  });

  it("does not sell a mashup of one track's vocals over another's drums", () => {
    expect(src).not.toMatch(/Mash A|vocals over|over B's drums/i);
  });
});

describe("claims about other tools", () => {
  const banned: [RegExp, string][] = [
    [/Almost every one of them routes your audio/i, "an unsourced blanket claim about AI audio tools"],
    [/Most AI audio tools upload your files/i, "an unsourced blanket claim about AI audio tools"],
    [/all upload your audio/i, "an unsourced claim about named cloud tools"],
    [/Only Approach That Matters/i, "an unqualified superlative"],
    [/Best AI Audio Tools Let You/i, "an unqualified superlative"],
  ];

  it.each(banned)("the site does not say %s", (pattern, why) => {
    const hits = siteSources()
      .filter(([, src]) => pattern.test(src))
      .map(([path]) => path);
    expect(hits, why).toEqual([]);
  });

  it("gives no post a title with a superlative or an 'only'", () => {
    for (const p of posts) {
      expect(p.title, p.slug).not.toMatch(/\b(?:best|only|ultimate|perfect|greatest)\b/i);
    }
  });

  it("sources the claim that cloud stem tools take an upload", () => {
    const post = posts.find((p) => p.slug === "stem-separation-explained-demucs");
    expect(post, "the stem post is gone; update this test").toBeDefined();
    const text = textOf(post!.body);
    // Each tool it names is linked to that tool's own page.
    expect(text).toMatch(/\[LALAL\.AI\]\(https:\/\/www\.lalal\.ai\//);
    expect(text).toMatch(/\[Moises\]\(https:\/\/moises\.ai\//);
    // The not-shipped statement stays where a reader sees it first.
    expect(post!.description).toMatch(/not shipped yet|has not shipped/i);
  });

  it("keeps the URLs of the retitled posts", () => {
    const slugs = posts.map((p) => p.slug);
    expect(slugs).toContain("ai-audio-editing-local-first");
    expect(slugs).toContain("open-source-audio-editor-byo-llm");
  });
});

describe("the repository README", () => {
  const readme = existsSync(join(REPO, "README.md")) ? readFileSync(join(REPO, "README.md"), "utf8") : "";
  // The opening: everything above the first section heading.
  const opening = readme.split(/^## /m)[0];

  it("is there to be read", () => {
    expect(opening.length).toBeGreaterThan(200);
  });

  it("opens with what ships, not a mashup or stems", () => {
    // `marketing/README.md`: no "mashup A's vocals over B's drums", no
    // stems, until `separate_stems` does something (#385).
    expect(opening).not.toMatch(/mashup/i);
    expect(opening).not.toMatch(/drop stems/i);
  });

  it("says near the top what it does not do yet", () => {
    expect(opening).toMatch(/does not do yet/i);
    expect(opening).toMatch(/stem separation/i);
    expect(opening).toMatch(/not shipped/i);
    expect(opening).toContain("/issues/385");
    // Transcription shipped in v0.8.0 and says what it needs: the download.
    expect(opening).toMatch(/transcription/i);
    expect(opening).toMatch(/speech model/i);
  });
});
