/**
 * The split user guide: an index plus one page per entry of `guidePages`.
 *
 * Held here: every page has prose and a route (a slug that is not a page is
 * a 404); the headings the data lists are the `h2` ids in the prose, and the
 * prose has no other `h2` with an id the data does not know; the two anchors
 * the single page ever had (`#transcription`, `#chatgpt-plan`) map to the
 * page that has them; and nothing on the site still links to the old
 * `/docs/user-guide#...` form.
 */
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

import { hashTargets, resolveHash } from "./docs/reference";
import { USER_GUIDE_ALIASES, userGuideReference } from "./docs/references";
import { guidePages } from "./docs/user-guide-data";

const ROOT = fileURLToPath(new URL("../", import.meta.url));
const read = (rel: string) => readFileSync(join(ROOT, rel), "utf8");

describe("the user guide's pages", () => {
  it("has a prose file for every page, and a page for every prose file", () => {
    const files = readdirSync(join(ROOT, "components/docs/user-guide"))
      .filter((f) => f.endsWith(".tsx"))
      .map((f) => f.replace(/\.tsx$/, ""))
      .sort();
    expect(files).toEqual(guidePages.map((p) => p.slug).sort());
    const registry = read("components/docs/user-guide/index.ts");
    for (const p of guidePages) expect(registry, p.slug).toMatch(new RegExp(`(^|\\s)"?${p.slug}"?:`, "m"));
  });

  it("lists as headings exactly the h2 ids of the prose", () => {
    for (const p of guidePages) {
      const source = read(`components/docs/user-guide/${p.slug}.tsx`);
      const ids = [...source.matchAll(/<h2 id="([^"]+)">/g)].map((m) => m[1]);
      expect(ids, p.slug).toEqual(p.headings.map((h) => h.id));
      if (p.headings.length === 0) expect(source, `${p.slug} has an h2 the page title repeats`).not.toContain("<h2");
    }
  });

  it("keeps each page to a phone-sized read", () => {
    for (const p of guidePages) {
      const words = read(`components/docs/user-guide/${p.slug}.tsx`).replace(/<[^>]+>/g, " ").split(/\s+/).length;
      expect(words, p.slug).toBeLessThan(2000);
    }
  });
});

describe("the old anchors of /docs/user-guide", () => {
  const targets = hashTargets(userGuideReference.areas, USER_GUIDE_ALIASES);

  it("send #transcription and #chatgpt-plan to their pages", () => {
    expect(resolveHash("#transcription", targets)).toBe("speech#transcription");
    expect(resolveHash("#chatgpt-plan", targets)).toBe("chatgpt-plan");
  });

  it("send every heading of a multi-heading page to it", () => {
    for (const p of guidePages.filter((g) => g.headings.length > 1)) {
      for (const h of p.headings) expect(resolveHash(`#${h.id}`, targets), h.id).toBe(`${p.slug}#${h.id}`);
    }
  });
});

describe("links into the guide", () => {
  it("never use the old single-page anchor form", () => {
    const hits: string[] = [];
    const walk = (dir: string) => {
      for (const name of readdirSync(dir)) {
        if (name === "node_modules" || name === ".next") continue;
        const path = join(dir, name);
        if (statSync(path).isDirectory()) walk(path);
        else if (/\.(tsx?|md)$/.test(name) && !name.endsWith("userGuide.test.ts") && /user-guide#/.test(readFileSync(path, "utf8"))) hits.push(path);
      }
    };
    for (const d of ["app", "components", "lib"]) walk(join(ROOT, d));
    expect(hits).toEqual([]);
  });
});
