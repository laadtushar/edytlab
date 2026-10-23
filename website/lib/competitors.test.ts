/**
 * What the comparison pages may say, held to the files that say it.
 *
 * Named comparison pages are the part of the site most likely to be wrong
 * in a way a reader can check: a price that moved, a feature that
 * shipped, a sentence that says edytlab does what it does not. These
 * tests make that harder to do by accident:
 *
 * - `lib/competitors.ts` is the one place a competitor fact is written
 *   down, and every fact in it has a source link on the product's own
 *   page and the day it was read; none may be older than 100 days (the
 *   pages say they are re-checked every 90);
 * - the pages and components that make a comparison use no unqualified
 *   superlative, and never say edytlab can do what it cannot (stem
 *   separation, MIDI, plugin hosting; transcription and the editing that
 *   works from it shipped in v0.8.0 and are no longer on the list) outside a sentence that also denies it;
 * - no competitor's logo or screenshot is in the repository's served
 *   files;
 * - the pages carry what the research asked of every comparison: where
 *   the other product is better comes first, the "not built yet" note,
 *   the disclaimer, a byline and the day the facts were checked.
 *
 * The rules are in `scratchpad/seo-research/report.md`, sections 5.0 and
 * 7 (summarised on `/about`). This file does not replace reading them.
 */

import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { Disclaimer } from "../components/compare/disclaimer";
import { FactsTable } from "../components/compare/facts-table";
import { LastVerified } from "../components/compare/last-verified";
import { NotBuiltYet } from "../components/compare/not-built-yet";
import { SourcesList } from "../components/compare/sources-list";
import { Verdict } from "../components/compare/verdict";
import AboutPage from "../app/about/page";
import AudacityAlternativePage from "../app/alternatives/audacity/page";
import {
  REFRESH_AFTER_DAYS,
  claimFor,
  competitorBySlug,
  competitors,
  displayDate,
  hostAndPath,
  newestCheck,
  type Competitor,
} from "./competitors";
import { PAGE_UPDATED, absoluteUrl } from "./seo";
import { siteConfig } from "./site";

const ROOT = fileURLToPath(new URL("../", import.meta.url));
const read = (path: string) => readFileSync(join(ROOT, path), "utf8");

/** Every file under `dir` (relative to the site root), as paths relative to it. */
function filesUnder(dir: string): string[] {
  const out: string[] = [];
  const walk = (d: string) => {
    for (const name of readdirSync(join(ROOT, d))) {
      if (name === "node_modules" || name === ".next") continue;
      const rel = join(d, name);
      if (statSync(join(ROOT, rel)).isDirectory()) walk(rel);
      else out.push(rel.split(sep).join("/"));
    }
  };
  if (existsSync(join(ROOT, dir))) walk(dir);
  return out;
}

/** A source file without its comments: a comment may say what a page must not. */
function code(rel: string): string {
  return read(rel)
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/^\s*\/\/.*$/gm, "");
}

/** Today, as the tests see it. Facts are read on the day they are checked. */
const TODAY = new Date();
const DAY = 86_400_000;
const daysSince = (iso: string) => (TODAY.getTime() - Date.parse(`${iso}T00:00:00Z`)) / DAY;

/** Every `{ url | sourceUrl, checkedOn }` pair in a competitor, with a label for the message. */
function sources(c: Competitor): { label: string; url: string; checkedOn: string }[] {
  return [
    ...(["platforms", "pricing", "openSource", "localOrCloud", "aiFeatures"] as const).map((k) => ({
      label: `${c.slug}.${k}`,
      url: c[k].url,
      checkedOn: c[k].checkedOn,
    })),
    ...c.claims.map((claim, i) => ({
      label: `${c.slug}.claims[${i}]`,
      url: claim.sourceUrl,
      checkedOn: claim.checkedOn,
    })),
  ];
}

describe("the competitor data", () => {
  it("seeds the eight products the comparison hub will row", () => {
    expect(competitors.map((c) => c.slug)).toEqual([
      "audacity",
      "adobe-audition",
      "descript",
      "auphonic",
      "audionaut",
      "cleanvoice",
      "adobe-podcast",
      "ocenaudio",
    ]);
  });

  it("gives each a unique kebab-case slug, a plain-text name and every field filled in", () => {
    expect(new Set(competitors.map((c) => c.slug)).size).toBe(competitors.length);
    for (const c of competitors) {
      expect(c.slug, c.name).toMatch(/^[a-z0-9]+(?:-[a-z0-9]+)*$/);
      expect(c.name.trim(), c.slug).not.toBe("");
      expect(c.category.trim(), c.slug).not.toBe("");
      for (const k of ["platforms", "pricing", "openSource", "localOrCloud", "aiFeatures"] as const) {
        expect(c[k].text.trim().length, `${c.slug}.${k}`).toBeGreaterThan(10);
      }
      expect(c.claims.length, `${c.slug} has no claims`).toBeGreaterThan(0);
      for (const claim of c.claims) expect(claim.text.trim().length, c.slug).toBeGreaterThan(10);
    }
  });

  it("looks a product up by slug, and a claim by its topic", () => {
    expect(competitorBySlug("audacity").name).toBe("Audacity");
    expect(() => competitorBySlug("not-a-product")).toThrow();
    expect(claimFor(competitorBySlug("audacity"), "privacy-notice").sourceUrl).toBe(
      "https://www.audacityteam.org/legal/privacy-notice/",
    );
    expect(() => claimFor(competitorBySlug("audacity"), "no-such-topic")).toThrow(/no claim/);
  });

  it("gives each claim a topic that is unique within its product", () => {
    for (const c of competitors) {
      const topics = c.claims.map((claim) => claim.topic);
      for (const t of topics) expect(t, c.slug).toMatch(/^[a-z0-9]+(?:-[a-z0-9]+)*$/);
      expect(new Set(topics).size, `${c.slug} repeats a topic`).toBe(topics.length);
    }
  });

  it("sources every fact on https, to a page that is not ours and carries no tracking", () => {
    for (const c of competitors) {
      for (const s of sources(c)) {
        const url = new URL(s.url); // throws on a malformed one
        expect(url.protocol, `${s.label}: ${s.url}`).toBe("https:");
        expect(url.hostname, s.label).not.toMatch(/edytlab\.com$/);
        expect(url.search, `${s.label} carries a query string: ${s.url}`).not.toMatch(/utm_|mtm_|fbclid|gclid/);
      }
    }
  });

  it("dates every fact, and none is more than 100 days old", () => {
    // The pages promise a re-check every 90 days (`REFRESH_AFTER_DAYS`);
    // the test allows ten more before it fails, so a build is not red on
    // the 91st day but is by the 101st.
    expect(REFRESH_AFTER_DAYS).toBe(90);
    for (const c of competitors) {
      for (const s of sources(c)) {
        expect(s.checkedOn, s.label).toMatch(/^\d{4}-\d{2}-\d{2}$/);
        const age = daysSince(s.checkedOn);
        expect(age, `${s.label} was checked on ${s.checkedOn}, in the future`).toBeGreaterThanOrEqual(-1);
        expect(age, `${s.label} was checked on ${s.checkedOn}: re-read its source and move the date`).toBeLessThanOrEqual(100);
      }
    }
  });

  it("reports the newest check, and writes a date the same in every time zone", () => {
    const audacity = competitorBySlug("audacity");
    const newest = newestCheck(audacity);
    expect(sources(audacity).map((s) => s.checkedOn)).toContain(newest);
    expect(displayDate("2026-10-22")).toBe("22 October 2026");
    expect(displayDate("2026-01-05")).toBe("5 January 2026");
  });

  it("shortens a source link to its host and path", () => {
    expect(hostAndPath("https://www.audacityteam.org/download/")).toBe("audacityteam.org/download");
    expect(hostAndPath("https://plugins.audacityteam.org/ai-plugins/ai-plugins")).toBe(
      "plugins.audacityteam.org/ai-plugins/ai-plugins",
    );
    expect(hostAndPath("https://www.audacityteam.org/")).toBe("audacityteam.org");
  });

  it("states Audacity's AI plugins, and does not say it lacks AI", () => {
    const text = JSON.stringify(competitorBySlug("audacity"));
    expect(text).toMatch(/OpenVINO/);
    expect(text).toMatch(/music separation/i);
    expect(text).toMatch(/Whisper/);
    expect(text).toMatch(/100% locally/);
    expect(text).not.toMatch(/\b(?:lacks|has no|without) (?:any )?(?:AI|machine learning)\b/i);
  });
});

/**
 * The scanners, kept as functions so each can be tried on a sentence it
 * must catch and one it must let through. A guard that never fires is no
 * guard.
 */

/** Text a source quotes from a product's own page: exempt, because it is theirs. */
function withoutQuotes(text: string): string {
  return text
    .replace(/&ldquo;[\s\S]*?&rdquo;/g, " ")
    .replace(/<q>[\s\S]*?<\/q>/g, " ")
    .replace(/\u201c[^\u201d]*\u201d/g, " ");
}

const SUPERLATIVE = /\b(?:best|worst|cheapest|better than|beats|kills?)\b/i;

function superlatives(source: string): string[] {
  return [...withoutQuotes(source).matchAll(new RegExp(SUPERLATIVE.source, "gi"))].map((m) => m[0]);
}

/** What edytlab may never be said to do (the capability ledger, `report.md` 4.2). */
const NEVER_CLAIM: RegExp[] = [
  /\bstem separation\b/i,
  /\bseparat(?:e|es|ed|ing) (?:the |a |your )?(?:stems|vocals)\b/i,
  /\bvocal isolation\b/i,
  /\bisolat(?:e|es|ing) (?:the |a |your )?vocals?\b/i,
  /\bmash-?ups?\b/i,
  /\bspeaker detection\b/i,
  /\bdetects? speakers\b/i,
  /\bspeech enhancement\b/i,
  /\bMIDI\b/,
  /\bplugin hosting\b/i,
  /\blive DJ\b/i,
  /\b(?<!un)signed (?:installers?|builds?)\b/i,
  /\btrue[- ]peak (?:compliance|compliant)\b/i,
];

/** Words that make a sentence a denial, or a statement of a limit. */
const DENIES = /\b(?:not|no|never|cannot|without|neither|nor|none)\b|n['\u2019]t\b|has not shipped|not yet/i;

/** Source text as plain sentences: entities and JSX braces flattened, quotes dropped. */
function sentences(source: string): string[] {
  return withoutQuotes(source)
    .replace(/&apos;|&rsquo;/g, "'")
    .replace(/&amp;/g, "&")
    .replace(/\{" "\}|\{"\\u00a0"\}/g, " ")
    .replace(/\s+/g, " ")
    .split(/(?<=[.!?])\s+/);
}

function unqualifiedNeverClaims(source: string): string[] {
  return sentences(source).filter((s) => NEVER_CLAIM.some((re) => re.test(s)) && !DENIES.test(s));
}

/** A competitor's logo or screenshot, by the name of the file that would hold it. */
const COMPETITOR_FILE = /audacity|audition|adobe|descript|auphonic|audionaut|cleanvoice|ocenaudio|musehub|openvino/;
const IMAGE_FILE = /\.(?:png|jpe?g|gif|webp|avif|svg|ico|bmp)$/i;

function competitorImages(paths: string[]): string[] {
  return paths.filter((p) => IMAGE_FILE.test(p) && COMPETITOR_FILE.test(p.toLowerCase().replace(/[^a-z0-9/.]/g, "")));
}

describe("the scanners", () => {
  it("catches an unqualified superlative, and lets a quoted one through", () => {
    expect(superlatives("edytlab is the best AI audio editor.")).toEqual(["best"]);
    expect(superlatives("It is better than Audacity and beats Descript.")).toEqual(["better than", "beats"]);
    expect(superlatives("Where Audacity is better, and where edytlab differs.")).toEqual([]);
    expect(superlatives("Its page says &ldquo;the best audio cleanup&rdquo; in a quote.")).toEqual([]);
    expect(superlatives("Its page says \u201cthe best audio cleanup\u201d in a quote.")).toEqual([]);
  });

  it("catches a capability edytlab does not have, unless the sentence denies it", () => {
    expect(unqualifiedNeverClaims("edytlab does stem separation on your tracks.")).toHaveLength(1);
    expect(unqualifiedNeverClaims("Ask it to isolate the vocals and detect speakers in the interview.")).toHaveLength(1);
    expect(unqualifiedNeverClaims("It hosts VST3 plugins. It has MIDI editing.")).toHaveLength(1);
    expect(unqualifiedNeverClaims("Stem separation has not shipped. Speaker detection is not available.")).toEqual([]);
    // Shipped in v0.8.0: no longer a claim the pages must deny.
    expect(unqualifiedNeverClaims("Ask it to transcribe the interview and remove filler words.")).toEqual([]);
    expect(unqualifiedNeverClaims("edytlab has no MIDI and no plugin hosting.")).toEqual([]);
    expect(unqualifiedNeverClaims("Its installers are unsigned, so macOS warns.")).toEqual([]);
    expect(unqualifiedNeverClaims("It ships signed installers for every platform.")).toHaveLength(1);
    expect(unqualifiedNeverClaims("The plugin page lists &ldquo;Whisper Transcription&rdquo; for Audacity.")).toEqual([]);
    expect(unqualifiedNeverClaims("Audacity lists Whisper transcription, which edytlab does not.")).toEqual([]);
  });

  it("finds a competitor's image by the name of its file, and no other image", () => {
    expect(competitorImages(["public/audacity-logo.png", "components/compare/adobe_audition.svg"])).toHaveLength(2);
    expect(competitorImages(["public/Audacity Screenshot.JPG"])).toHaveLength(1);
    expect(competitorImages(["public/logo.png", "public/demos/dj-mini-mix.jpg", "components/compare/facts-table.tsx"])).toEqual([]);
  });
});

/** The pages and components that make a comparison, and the data they read. */
function comparisonSources(): string[] {
  return [
    "lib/competitors.ts",
    ...filesUnder("components/compare").filter((f) => f.endsWith(".tsx")),
    ...filesUnder("app/alternatives").filter((f) => f.endsWith(".tsx")),
    ...filesUnder("app/compare").filter((f) => f.endsWith(".tsx")),
    "app/about/page.tsx",
  ];
}

/** The same, without the data file: it describes other products, so it may name what they can do. */
const proseSources = () => comparisonSources().filter((f) => f !== "lib/competitors.ts");

describe("what the comparison sources say", () => {
  it("reads the components, the /about page and the Audacity page", () => {
    const files = comparisonSources();
    for (const f of [
      "lib/competitors.ts",
      "components/compare/facts-table.tsx",
      "components/compare/verdict.tsx",
      "components/compare/not-built-yet.tsx",
      "components/compare/sources-list.tsx",
      "components/compare/disclaimer.tsx",
      "components/compare/last-verified.tsx",
      "app/about/page.tsx",
      "app/alternatives/audacity/page.tsx",
    ]) {
      expect(files, f).toContain(f);
    }
  });

  it("uses no unqualified superlative", () => {
    // `best`, `worst`, `cheapest`, `better than`, `beats`, `kill`: a
    // comparison to every other product, which no source here can back.
    // A product's own words, quoted, are its claim and not ours.
    const found = comparisonSources().flatMap((f) => superlatives(code(f)).map((w) => `${f}: ${w}`));
    expect(found).toEqual([]);
  });

  it("never says edytlab does what it cannot, outside a sentence that denies it", () => {
    const found = proseSources().flatMap((f) => unqualifiedNeverClaims(code(f)).map((s) => `${f}: ${s}`));
    expect(found).toEqual([]);
  });

  it("hotlinks no competitor's picture", () => {
    const hotlink = /https?:\/\/[^"'`\s)]*(?:audacityteam|adobe|descript|auphonic|audionaut|cleanvoice|ocenaudio|musehub)[^"'`\s)]*\.(?:png|jpe?g|gif|svg|webp|avif|ico)/i;
    const found = comparisonSources().filter((f) => hotlink.test(code(f)));
    expect(found).toEqual([]);
  });

  it("holds no competitor's logo or screenshot in public/ or components/", () => {
    const files = [...filesUnder("public"), ...filesUnder("components")];
    expect(files.length).toBeGreaterThan(10);
    expect(competitorImages(files)).toEqual([]);
  });

  it("uses a competitor's name as plain text, not in a path of the site", () => {
    // `/alternatives/<slug>` is the page for a product; nothing else of
    // ours is named after one (no domain, no app name, no handle).
    expect(siteConfig.name).toBe("edytlab");
    expect(siteConfig.url).not.toMatch(/audacity|audition|descript|auphonic/i);
    for (const rel of filesUnder("app").filter((f) => f.endsWith("page.tsx"))) {
      const route = "/" + relative("app", rel.replace(/\/page\.tsx$/, ""));
      if (COMPETITOR_FILE.test(route)) expect(route, rel).toMatch(/^\/alternatives\//);
    }
  });
});

describe("the components", () => {
  const audacity = competitorBySlug("audacity");

  it("puts a source link and a checked date on every row of a facts table", () => {
    const html = renderToStaticMarkup(
      createElement(FactsTable, {
        theirName: "Audacity",
        rows: [
          {
            label: "Price",
            ours: "Free app",
            oursSource: { href: "/docs/getting-started", label: "edytlab docs" },
            theirs: audacity.pricing.text,
            source: { href: audacity.pricing.url },
            checkedOn: audacity.pricing.checkedOn,
          },
          {
            label: "License",
            ours: "MIT",
            theirs: audacity.openSource.text,
            source: { href: audacity.openSource.url },
            checkedOn: audacity.openSource.checkedOn,
          },
        ],
      }),
    );
    const rows = [...html.matchAll(/<tr[\s\S]*?<\/tr>/g)].map((m) => m[0]).filter((r) => r.includes("<td"));
    expect(rows).toHaveLength(2);
    expect(html).toContain("<table");
    expect(html).toContain("<th");
    expect(html).toContain("Audacity");
    expect(html).toContain("edytlab");
    for (const row of rows) {
      expect(row).toMatch(/<a [^>]*href="https:\/\/[^"]+"/);
      expect(row).toMatch(/<time [^>]*datetime="\d{4}-\d{2}-\d{2}"/i);
    }
    expect(rows[0]).toContain(`href="${audacity.pricing.url}"`);
    expect(rows[0]).toContain('href="/docs/getting-started"');
    expect(html).toContain("22 October 2026");
  });

  it("does not make a table wider than a phone", () => {
    const html = renderToStaticMarkup(
      createElement(FactsTable, {
        theirName: "Audacity",
        rows: [{ label: "Price", ours: "Free", theirs: "Free", source: { href: audacity.pricing.url }, checkedOn: "2026-10-22" }],
      }),
    );
    expect(html.match(/\bmin-w-\[[^\]]*\]/g) ?? []).toEqual([]);
    expect(html).not.toMatch(/overflow-x-(?:auto|scroll)/);
  });

  it("states the limit in NotBuiltYet: stem separation has not shipped, with its issue", () => {
    const html = renderToStaticMarkup(createElement(NotBuiltYet));
    expect(html).toMatch(/Not built yet/i);
    expect(html).toMatch(/stem separation/i);
    expect(html).toMatch(/have not shipped|has not shipped|not shipped/i);
    expect(html).toContain("/issues/385");
    // Transcription shipped in v0.8.0: the note must not list it as missing.
    expect(html).not.toMatch(/transcri/i);
    // And it leads with what is missing, not with what is possible.
    expect(unqualifiedNeverClaims(code("components/compare/not-built-yet.tsx"))).toEqual([]);
  });

  it("writes the disclaimer the research set, naming the product and the day", () => {
    const html = renderToStaticMarkup(createElement(Disclaimer, { competitor: "Audacity", checkedOn: "2026-10-22" }));
    const text = html.replace(/<[^>]+>/g, " ").replace(/&#x27;|&apos;/g, "'").replace(/\s+/g, " ");
    expect(text).toContain("edytlab is an independent open-source project");
    expect(text).toContain("not affiliated with, endorsed by or sponsored by Audacity or its owner");
    expect(text).toContain("trademarks of their respective owners");
    expect(text).toContain("only to identify the products being compared");
    expect(text).toContain("was last checked on 22 October 2026");
    expect(text).toContain("check Audacity's site before you decide");
    expect(text).toContain("Found a mistake?");
    expect(html).toContain('href="/about#corrections"');
    expect(html).toContain(`href="${siteConfig.github}/issues"`);
  });

  it("lists each source with its host, its path and its date", () => {
    const html = renderToStaticMarkup(
      createElement(SourcesList, {
        competitor: "Audacity",
        sources: [
          { label: "Audacity downloads", href: "https://www.audacityteam.org/download/", checkedOn: "2026-10-22" },
          { label: "Audacity AI plugins", href: "https://plugins.audacityteam.org/ai-plugins/ai-plugins", checkedOn: "2026-10-22" },
        ],
      }),
    );
    expect(html.match(/<li/g)).toHaveLength(2);
    expect(html).toContain('href="https://www.audacityteam.org/download/"');
    expect(html).toContain("audacityteam.org/download");
    expect(html).toContain("plugins.audacityteam.org/ai-plugins/ai-plugins");
    expect(html.match(/datetime="2026-10-22"/gi)).toHaveLength(2);
  });

  it("gives a verdict both ways, naming the other product first", () => {
    const html = renderToStaticMarkup(
      createElement(Verdict, {
        competitor: "Audacity",
        theirs: ["you record"],
        ours: ["you describe the edit"],
      }),
    );
    const text = html.replace(/<[^>]+>/g, " ").replace(/\s+/g, " ");
    expect(text.indexOf("Choose Audacity if")).toBeGreaterThan(-1);
    expect(text.indexOf("Choose edytlab if")).toBeGreaterThan(text.indexOf("Choose Audacity if"));
    expect(text).toContain("you record");
    expect(text).toContain("you describe the edit");
  });

  it("shows the byline and the day the facts were checked", () => {
    const html = renderToStaticMarkup(createElement(LastVerified, { checkedOn: "2026-10-22" }));
    const text = html.replace(/<[^>]+>/g, " ").replace(/\s+/g, " ");
    expect(text).toContain("Tushar Laad");
    expect(text).toContain("checked on 22 October 2026");
    expect(html).toContain('href="https://www.tusharlaad.com"');
    expect(html).toMatch(/<time [^>]*datetime="2026-10-22"/i);
    expect(html).toContain('href="/about#how-we-compare"');
  });
});

describe("/about", () => {
  const src = read("app/about/page.tsx");
  const html = renderToStaticMarkup(createElement(AboutPage));
  const text = html.replace(/<script[\s\S]*?<\/script>/g, " ").replace(/<[^>]+>/g, " ").replace(/&#x27;/g, "'").replace(/\s+/g, " ");

  it("names who builds it, from lib/family.ts", () => {
    expect(text).toContain("Tushar Laad");
    expect(text).toContain("LabyNator");
    expect(html).toContain('href="https://www.tusharlaad.com"');
    expect(html).toContain('href="https://www.labynator.com"');
  });

  it("says how a comparison is made: sourced, dated, re-checked, the other product first", () => {
    expect(html).toContain('id="how-we-compare"');
    expect(text).toMatch(/own pages/);
    expect(text).toMatch(/every 90 days/);
    expect(text).toMatch(/where the other product is better/i);
    expect(text).toMatch(/no affiliate links/i);
    expect(text).toMatch(/paid placement/i);
    expect(text).toMatch(/logos/);
  });

  it("discloses the AI help in the words the owner chose, and claims no review it cannot back", () => {
    expect(text).toContain(
      "edytlab's code, docs and this site are written with AI coding assistants (Claude Code) under the maintainer's direction",
    );
    // Google asks how automation was used. The page says so; it does not
    // add that each page is checked by a person, which nothing guarantees.
    expect(text).not.toMatch(/reviewed by (?:the )?(?:owner|maintainer|founder|a person|a human|Tushar)/i);
    expect(text).not.toMatch(/(?:every|each) page is (?:reviewed|checked|verified|edited) by/i);
  });

  it("has a corrections policy that links the issue tracker", () => {
    expect(html).toContain('id="corrections"');
    expect(html).toContain(`href="${siteConfig.github}/issues"`);
  });

  it("says what edytlab is not, including the thing that has not shipped", () => {
    expect(text).toMatch(/not a DAW/i);
    expect(text).toMatch(/stem separation has not shipped/i);
    expect(text).toMatch(/unsigned/i);
  });

  it("is an AboutPage with a trail from the home page, and no rating or review", () => {
    expect(src).toContain('"@type": "AboutPage"');
    expect(src).toContain("breadcrumbJsonLd(pageBreadcrumbs(");
    const blocks = [...html.matchAll(/<script type="application\/ld\+json">([\s\S]*?)<\/script>/g)].map((m) => JSON.parse(m[1]));
    expect(blocks.map((b) => b["@type"]).sort()).toEqual(["AboutPage", "BreadcrumbList"]);
    const about = blocks.find((b) => b["@type"] === "AboutPage");
    expect(about.url).toBe(absoluteUrl("/about"));
    expect(about.dateModified).toBe(PAGE_UPDATED["/about"]);
    expect(JSON.stringify(blocks)).not.toMatch(/rating|review|Product/i);
  });
});

describe("/alternatives/audacity", () => {
  const path = "/alternatives/audacity";
  const src = read("app/alternatives/audacity/page.tsx");
  const html = renderToStaticMarkup(createElement(AudacityAlternativePage));
  const body = html.replace(/<script[\s\S]*?<\/script>/g, " ");
  const text = body.replace(/<[^>]+>/g, " ").replace(/&#x27;/g, "'").replace(/&quot;/g, '"').replace(/&amp;/g, "&").replace(/\s+/g, " ");

  it("sets the title and description the research chose, within the limits a result shows", () => {
    const title = src.match(/const title = "([^"]+)"/)?.[1] ?? "";
    const description = src.match(/const description =\s*"([^"]+)"/)?.[1] ?? "";
    expect(title).toBe("Audacity alternative: free, open source, AI-driven");
    expect(title.length).toBeLessThanOrEqual(50);
    expect(description.length).toBeGreaterThan(100);
    expect(description.length).toBeLessThanOrEqual(160);
    expect(description).toMatch(/where Audacity is better/i);
    expect(src).toContain(`alternates: pageAlternates("${path}")`);
    expect(src).toContain(`...pageSocial("${path}"`);
  });

  it("has one h1, the one the research wrote", () => {
    expect(body.match(/<h1\b/g)).toHaveLength(1);
    expect(body).toContain("An Audacity alternative you control by describing the edit");
  });

  it("answers first, in 40 to 70 words, and pairs 'free' with the cost of a hosted provider", () => {
    const lead = body.match(/<h1[\s\S]*?<\/h1>\s*<p[^>]*>([\s\S]*?)<\/p>/)?.[1] ?? "";
    const words = lead.replace(/<[^>]+>/g, " ").split(/\s+/).filter(Boolean);
    expect(words.length).toBeGreaterThanOrEqual(40);
    expect(words.length).toBeLessThanOrEqual(70);
    expect(lead).toMatch(/Audacity is also free and open source/);
    expect(lead).toMatch(/unless you use Ollama/);
  });

  it("says where Audacity is better before it says where edytlab differs", () => {
    const better = body.indexOf("Where Audacity is better");
    const differs = body.indexOf("Where edytlab differs");
    expect(better, "no 'Where Audacity is better' heading").toBeGreaterThan(-1);
    expect(differs, "no 'Where edytlab differs' heading").toBeGreaterThan(-1);
    expect(better).toBeLessThan(differs);
    // In the first third of the page, as the research asks.
    expect(better / body.length).toBeLessThan(0.45);
  });

  it("states Audacity's AI plugins from its plugin page, and edytlab's lack of music separation in the same breath", () => {
    expect(text).toMatch(/OpenVINO/);
    expect(text).toMatch(/music separation/i);
    expect(text).toMatch(/noise suppression/i);
    expect(text).toMatch(/Whisper/);
    expect(text).toMatch(/run 100% locally/);
    expect(text).toMatch(/edytlab has not shipped stem separation/);
    expect(html).toContain('href="https://plugins.audacityteam.org/ai-plugins/ai-plugins"');
    expect(text).not.toMatch(/Audacity (?:has no|lacks|does not (?:have|offer|include)) (?:any )?(?:AI|machine)/i);
  });

  it("explains the Audacity label interop, from Audacity's manual", () => {
    expect(text).toMatch(/import_labels/);
    expect(text).toMatch(/export_labels/);
    expect(html).toContain('href="https://manual.audacityteam.org/man/importing_and_exporting_labels.html"');
  });

  it("takes its privacy paragraph from Audacity's current notice, and from nowhere else", () => {
    expect(html).toContain('href="https://www.audacityteam.org/legal/privacy-notice/"');
    expect(text).toMatch(/update checking/i);
    expect(text).toMatch(/31 August 2026/);
    // Never imply Audacity collects more than its own notice says.
    expect(text).not.toMatch(/Audacity (?:collects|tracks|spies|harvests|sells)/i);
    expect(text).not.toMatch(/\bunsafe\b|\bspyware\b/i);
  });

  it("states the cost: free, but a hosted provider bills tokens; Ollama is free and local", () => {
    expect(text).toMatch(/hosted provider bills (?:you )?(?:for )?tokens/i);
    expect(text).toMatch(/Ollama is free and runs on your (?:own )?(?:machine|computer)/i);
  });

  it("carries the not-built-yet note, the verdict, the sources, the disclaimer and the byline", () => {
    for (const component of ["NotBuiltYet", "Verdict", "SourcesList", "Disclaimer", "LastVerified", "FactsTable"]) {
      expect(src, component).toContain(`<${component}`);
    }
    expect(text).toMatch(/Not built yet/);
    expect(text).toContain("Choose Audacity if");
    expect(text).toContain("Choose edytlab if");
    expect(text).toContain("is not affiliated with, endorsed by or sponsored by Audacity or its owner");
    expect(text).toContain("Tushar Laad");
    expect(text).toMatch(/checked on 22 October 2026/);
  });

  it("reads its Audacity facts from lib/competitors.ts, so they are dated and sourced there", () => {
    expect(src).toContain('competitorBySlug("audacity")');
  });

  it("lists, in its Sources, every Audacity page it links to", () => {
    const section = body.match(/<section aria-labelledby="sources">[\s\S]*?<\/section>/)?.[0] ?? "";
    expect(section, "no Sources section").not.toBe("");
    const linked = new Set(
      [...body.matchAll(/href="(https:\/\/[^"]+)"/g)]
        .map((m) => m[1])
        .filter((u) => /audacityteam\.org|xDarkzx/.test(u)),
    );
    expect(linked.size).toBeGreaterThanOrEqual(8);
    for (const url of linked) expect(section, `${url} is linked in the text but not listed under Sources`).toContain(`href="${url}"`);
  });

  it("quotes Audacity only in words that are in lib/competitors.ts, which was checked against its pages", () => {
    // A page's own wording is the one thing a reader can hold us to word
    // for word. Each quotation of Audacity here must be on a line of the
    // data file, where its source and date are. (The one example prompt is
    // edytlab's, not Audacity's.)
    const data = read("lib/competitors.ts").replace(/\s+/g, " ").replace(/[\u2018\u2019]/g, "'").toLowerCase();
    const quotes = [...text.matchAll(/\u201c([^\u201d]+)\u201d/g)].map((m) => m[1]).filter((q) => !/^match this track/.test(q));
    expect(quotes.length).toBeGreaterThan(8);
    const missing = quotes.filter((q) => {
      const plain = q.replace(/[\u2018\u2019]/g, "'").toLowerCase().replace(/[.,;:!]+$/, "");
      if (data.includes(plain)) return false;
      // The page may quote a shorter piece of a longer quotation in the data.
      return !data.includes(plain.replace(/^[a-z]+ /, ""));
    });
    expect(missing).toEqual([]);
  });

  it("links the evidence, /about and the pages it names, all of which exist", () => {
    for (const href of ["/videos/dj-mini-mix", "/about", "/docs/getting-started", "/use-cases/local-ai-audio-editor", "/privacy"]) {
      expect(html, href).toContain(`href="${href}`);
      const route = href.split("#")[0];
      expect(existsSync(join(ROOT, "app", route.slice(1), "page.tsx")) || route.startsWith("/videos/"), href).toBe(true);
    }
    expect(existsSync(join(ROOT, "public/demos/dj-mini-mix.jpg"))).toBe(true);
  });

  it("targets the long tail in plain prose: Linux, macOS, Windows, open source, free", () => {
    for (const phrase of [
      /Audacity alternative for Linux/i,
      /Audacity alternative (?:for|on) (?:macOS|Mac)/i,
      /Audacity alternative (?:for|on) Windows/i,
      /open[- ]source Audacity alternative/i,
      /free Audacity alternative/i,
    ]) {
      expect(text, String(phrase)).toMatch(phrase);
    }
  });

  it("is at least 800 words of prose", () => {
    const words = text.split(" ").filter((w) => /[a-z]/i.test(w));
    expect(words.length).toBeGreaterThanOrEqual(800);
  });

  it("is an Article by the founder with a trail from the home page, dated like the sitemap", () => {
    expect(src).toContain("breadcrumbJsonLd(pageBreadcrumbs(");
    expect(src).toContain("articleJsonLd({ path");
    const blocks = [...html.matchAll(/<script type="application\/ld\+json">([\s\S]*?)<\/script>/g)].map((m) => JSON.parse(m[1]));
    expect(blocks.map((b) => b["@type"]).sort()).toEqual(["Article", "BreadcrumbList"]);
    const article = blocks.find((b) => b["@type"] === "Article");
    expect(article.author).toEqual({ "@type": "Person", name: "Tushar Laad", url: "https://www.tusharlaad.com" });
    expect(article.dateModified).toBe(PAGE_UPDATED[path]);
    expect(article.datePublished <= article.dateModified).toBe(true);
    expect(article.headline.length).toBeLessThanOrEqual(110);
    const trail = blocks.find((b) => b["@type"] === "BreadcrumbList");
    expect(trail.itemListElement.map((i: { item: string }) => i.item)).toEqual([siteConfig.url, absoluteUrl(path)]);
    expect(JSON.stringify(blocks)).not.toMatch(/rating|review|Product/i);
  });

  it("shows the video as a link to its own page, not a second player", () => {
    expect(body).not.toMatch(/<video\b/);
    expect(html).toContain('href="/videos/dj-mini-mix"');
  });
});
