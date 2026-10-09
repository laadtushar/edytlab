/**
 * The marketing routes against the files that back them.
 *
 * A footer link to a page that was renamed, or a launch-kit title that
 * grew past the platform's limit after an edit, is invisible to the
 * build: the first is a 404 for a visitor, the second a rejected
 * submission on launch day. Both are checked here against the real
 * files.
 */

import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

const WEBSITE = fileURLToPath(new URL("../", import.meta.url));
const MARKETING = join(WEBSITE, "..", "marketing");

const read = (path: string) => readFileSync(path, "utf8");

/** Internal `href="/..."` targets in a source file, without hashes. */
function internalHrefs(source: string): string[] {
  const out = new Set<string>();
  for (const m of source.matchAll(/href="(\/[^"#?]*)/g)) out.add(m[1]);
  return [...out];
}

/** The fenced code blocks of a markdown file, in order. */
function fences(markdown: string): string[] {
  return [...markdown.matchAll(/```[a-z]*\n([\s\S]*?)\n```/g)].map((m) => m[1]);
}

describe("footer and marketing pages", () => {
  const files = [
    "components/landing/footer.tsx",
    "app/press/page.tsx",
    "app/use-cases/dj/page.tsx",
    "app/use-cases/local-ai-audio-editor/page.tsx",
    "components/landing/page-shell.tsx",
  ];

  it.each(files)("links only to routes that exist: %s", (file) => {
    for (const href of internalHrefs(read(join(WEBSITE, file)))) {
      if (href === "/") continue;
      const route = href.replace(/^\//, "");
      const isPage = existsSync(join(WEBSITE, "app", route, "page.tsx"));
      const isPublicFile = existsSync(join(WEBSITE, "public", route));
      expect(isPage || isPublicFile, `${file} links to ${href}`).toBe(true);
    }
  });

  it("links the footer to both use-case pages and the press page", () => {
    const footer = read(join(WEBSITE, "components/landing/footer.tsx"));
    expect(footer).toContain('href="/use-cases/dj"');
    expect(footer).toContain('href="/use-cases/local-ai-audio-editor"');
    expect(footer).toContain('href="/press"');
  });

  it("serves every asset the press page offers for download", () => {
    const page = read(join(WEBSITE, "app/press/page.tsx"));
    const hrefs = [...page.matchAll(/href: "(\/[^"]+)"/g)].map((m) => m[1]);
    expect(hrefs.length).toBeGreaterThan(0);
    for (const href of hrefs) {
      expect(existsSync(join(WEBSITE, "public", href)), href).toBe(true);
    }
  });

  it("gives each new page its own canonical URL on the site origin", () => {
    const canonicals = [
      ["app/press/page.tsx", "/press"],
      ["app/use-cases/dj/page.tsx", "/use-cases/dj"],
      [
        "app/use-cases/local-ai-audio-editor/page.tsx",
        "/use-cases/local-ai-audio-editor",
      ],
    ] as const;
    for (const [file, path] of canonicals) {
      const source = read(join(WEBSITE, file));
      expect(source).toContain(`\`\${siteConfig.url}${path}\``);
      expect(source).toContain("alternates: { canonical: url }");
    }
  });
});

describe("launch kit limits", () => {
  it("keeps every Show HN title within 80 characters", () => {
    const text = read(join(MARKETING, "launch/show-hn.md"));
    const titles = [...text.matchAll(/^\d\. `(Show HN: [^`]+)`/gm)].map(
      (m) => m[1],
    );
    expect(titles.length).toBeGreaterThan(0);
    for (const t of titles) expect(t.length, t).toBeLessThanOrEqual(80);
  });

  it("keeps the Product Hunt tagline within 60 and description within 260", () => {
    const [tagline, description] = fences(
      read(join(MARKETING, "launch/product-hunt.md")),
    );
    expect(tagline.length).toBeLessThanOrEqual(60);
    expect(description.length).toBeLessThanOrEqual(260);
  });

  it("keeps every post in the X thread within 280 characters", () => {
    const posts = fences(read(join(MARKETING, "social/x-thread.md")));
    expect(posts.length).toBe(8);
    for (const p of posts) expect(p.length, p).toBeLessThanOrEqual(280);
  });

  it("names the exact links in the files that need them", () => {
    for (const file of [
      "launch/show-hn.md",
      "launch/product-hunt.md",
      "launch/reddit.md",
      "social/x-thread.md",
      "social/linkedin.md",
      "press-kit.md",
    ]) {
      const text = read(join(MARKETING, file));
      expect(text, file).toContain("https://edytlab.com");
      expect(text, file).toContain(
        "https://github.com/laadtushar/edytlab/releases/latest",
      );
    }
  });
});
