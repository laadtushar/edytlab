/**
 * What the site's JavaScript costs a visitor at load, held to the files
 * that decide it.
 *
 * Lighthouse (mobile) found three things on the home page that a build
 * never shows:
 *
 * - "Legacy JavaScript, 13 KiB" in the entry chunk: the polyfills Next
 *   puts in every page, for features every browser Next targets has;
 * - "Forced reflow", about 340 ms, nearly all in GSAP's CSS plugin: each
 *   of the page's dozens of `gsap.from()` and `ScrollTrigger` calls,
 *   made at hydration for sections a screen or ten below the fold,
 *   wrote styles and then read layout, so the browser recalculated the
 *   whole page's style once for each;
 * - the long task that followed (total blocking time 1.1 to 2.2 s on the
 *   throttled run).
 *
 * The first is removed by an alias in `next.config.mjs`, the other two
 * by preparing a section's motion when the reader is a screen away from
 * it (`useGSAPWhenNear`) instead of all at once. Most checks read the
 * source, as the rest of `lib/*.test.ts` does; what they hold is the
 * reason each change is safe, so a Next upgrade or a new component
 * cannot undo it unnoticed.
 */

import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import nextConfig from "../next.config.mjs";

const ROOT = fileURLToPath(new URL("../", import.meta.url));
const read = (path: string) => readFileSync(join(ROOT, path), "utf8");
const require = createRequire(import.meta.url);
const NEXT = join(ROOT, "node_modules/next");

/** A source file without its comments: a comment may say what the code must not do. */
const code = (path: string) =>
  read(path)
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/^\s*\/\/.*$/gm, "");

describe("the polyfills Next puts in every page", () => {
  const SPECIFIER = "../build/polyfills/polyfill-module";
  const polyfills = readFileSync(join(NEXT, "dist/build/polyfills/polyfill-module.js"), "utf8");

  /**
   * What `polyfill-module.js` stands in for, and the first version of each browser that has the
   * real thing (MDN browser-compat-data, read 2026-10-22). Next's own default targets are
   * `chrome 111, edge 111, firefox 111, safari 16.4`.
   */
  const FEATURES: Record<string, { chrome: number; firefox: number; safari: number }> = {
    "String.prototype.trimStart": { chrome: 66, firefox: 61, safari: 12 },
    "String.prototype.trimEnd": { chrome: 66, firefox: 61, safari: 12 },
    "Symbol.prototype.description": { chrome: 70, firefox: 63, safari: 12.1 },
    "Array.prototype.flat": { chrome: 69, firefox: 62, safari: 12 },
    "Array.prototype.flatMap": { chrome: 69, firefox: 62, safari: 12 },
    "Promise.prototype.finally": { chrome: 63, firefox: 58, safari: 11.1 },
    "Object.fromEntries": { chrome: 73, firefox: 63, safari: 12.1 },
    "Array.prototype.at": { chrome: 92, firefox: 90, safari: 15.4 },
    "Object.hasOwn": { chrome: 93, firefox: 92, safari: 15.4 },
    "URL.canParse": { chrome: 120, firefox: 115, safari: 17 },
  };

  /** The features the file defines, read from its assignments. */
  function polyfilled(source: string): string[] {
    const found = new Set<string>();
    for (const m of source.matchAll(/\b(String|Array|Promise|Object|URL)(\.prototype)?\.(\w+)=/g)) found.add(`${m[1]}${m[2] ?? ""}.${m[3]}`);
    if (/Object\.defineProperty\(Symbol\.prototype,"description"/.test(source)) found.add("Symbol.prototype.description");
    return [...found].sort();
  }

  it("is aliased to an empty module, under the name Next imports it by", () => {
    const alias = (nextConfig as { turbopack?: { resolveAlias?: Record<string, string> } }).turbopack?.resolveAlias ?? {};
    expect(alias[SPECIFIER]).toBe("./lib/no-polyfills.js");
    expect(existsSync(join(ROOT, "lib/no-polyfills.js"))).toBe(true);
    expect(code("lib/no-polyfills.js").trim()).toBe("export {};");
  });

  it("is still imported by Next's client under that name, so the alias still bites", () => {
    // The import is relative, so the alias key is the specifier as written. If Next renames the
    // file or the import, the alias would match nothing and the polyfills would come back.
    for (const file of ["dist/esm/client/app-globals.js", "dist/client/app-globals.js"]) {
      expect(readFileSync(join(NEXT, file), "utf8"), file).toMatch(/polyfills\/polyfill-module['"]/);
      expect(readFileSync(join(NEXT, file), "utf8"), file).toContain(SPECIFIER);
    }
  });

  it("polyfills exactly the features this test knows, so a new one is looked at", () => {
    expect(polyfilled(polyfills)).toEqual(Object.keys(FEATURES).sort());
  });

  it("only stands in for features every browser Next targets has, URL.canParse aside", () => {
    const targets = require("next/dist/shared/lib/modern-browserslist-target") as string[];
    const target = (name: string) => Number(targets.find((t) => t.startsWith(`${name} `))?.split(" ")[1]);
    expect(targets).toEqual(["chrome 111", "edge 111", "firefox 111", "safari 16.4"]);
    for (const [feature, since] of Object.entries(FEATURES)) {
      if (feature === "URL.canParse") continue;
      expect(since.chrome, feature).toBeLessThanOrEqual(target("chrome"));
      expect(since.firefox, feature).toBeLessThanOrEqual(target("firefox"));
      expect(since.safari, feature).toBeLessThanOrEqual(target("safari"));
    }
  });

  /**
   * The names a file imports from `lucide-react`, or `null` for an import that does not name
   * them (`import * as`, a default import), which could reach any icon.
   */
  function lucideImports(source: string): string[] | null {
    const names: string[] = [];
    for (const [, clause] of source.matchAll(/\bimport\s+([^;]*?)\s+from\s*["']lucide-react["']/g)) {
      const braces = /^(?:type\s+)?\{([^}]*)\}$/.exec(clause.trim());
      if (!braces) return null;
      for (const part of braces[1].split(",")) {
        const name = part.trim().replace(/^type\s+/, "").split(/\s+as\s+/)[0];
        if (name) names.push(name);
      }
    }
    return names;
  }

  /**
   * The files under `dir` that mention `URL.canParse`, leaving out `skip` and `node_modules`.
   * `readdir` with file types, so a folder costs one call and not one `stat` per entry.
   */
  const callers = (dir: string, skip = ""): string[] =>
    readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) return entry.name === "node_modules" || path === skip ? [] : callers(path, skip);
      return /\.(?:m?js|cjs)$/.test(entry.name) && /canParse/.test(readFileSync(path, "utf8")) ? [path] : [];
    });

  it("is safe to drop URL.canParse, which Chrome 111, Firefox 111 and Safari 16.4 lack, because nothing in the browser calls it", () => {
    const since = FEATURES["URL.canParse"];
    expect(since.chrome).toBeGreaterThan(111);
    // The site's own code, and the lucide icons it imports.
    const own: string[] = [];
    const icons = new Set<string>();
    let everyIcon = false;
    const walk = (dir: string) => {
      for (const name of readdirSync(join(ROOT, dir))) {
        if (name === "node_modules" || name === ".next") continue;
        const rel = join(dir, name);
        if (statSync(join(ROOT, rel)).isDirectory()) walk(rel);
        else if (/\.(?:tsx?|mjs|js)$/.test(name)) {
          const source = read(rel);
          if (!/\.test\.ts$/.test(name) && /canParse/.test(source)) own.push(rel);
          const named = lucideImports(source);
          if (named === null) everyIcon = true;
          else for (const icon of named) icons.add(icon);
        }
      }
    };
    for (const dir of ["app", "components", "lib"]) walk(dir);
    expect(own, "a file of ours calls URL.canParse").toEqual([]);
    // What the page loads besides React and Next: nothing else in the client calls it either.
    const packages = [
      "gsap",
      "@gsap/react",
      "@vercel/analytics",
      "@vercel/speed-insights",
      "@radix-ui/react-accordion",
      "@radix-ui/react-slot",
      "lucide-react",
      "class-variance-authority",
      "clsx",
      "tailwind-merge",
    ];
    // lucide-react is 1,747 one-icon files, and a bundle holds only the icons the site imports
    // (`sideEffects: false`), so read those: the package minus its icons folder, then each icon
    // the site names, found through the package's own ESM entry. Reading all of them was the
    // bulk of this test's I/O, and it timed out on a cold disk (#687). An import that names no
    // icon, or an icon the entry does not list, reads the whole folder, so this never reads less
    // than it can show is safe.
    const lucide = join(ROOT, "node_modules/lucide-react");
    const iconsDir = join(lucide, "dist/esm/icons");
    const entry = readFileSync(join(lucide, "dist/esm/lucide-react.js"), "utf8");
    const iconFiles = (name: string) =>
      new RegExp(`default as ${name}(?=[,\\s}])[^;]*?from '\\./icons/([^']+)'`).exec(entry)?.[1];
    const files = [...icons].map((name) => [name, iconFiles(name)] as const);
    const unknown = everyIcon || files.some(([, file]) => file === undefined);
    expect(icons.size, "the site imports no lucide icons: has this scan lost them?").toBeGreaterThan(0);
    for (const pkg of packages) {
      const dir = join(ROOT, "node_modules", pkg);
      expect(callers(dir, pkg === "lucide-react" && !unknown ? iconsDir : ""), pkg).toEqual([]);
    }
    if (!unknown) {
      for (const [name, file] of files) {
        expect(/canParse/.test(readFileSync(join(iconsDir, file!), "utf8")), `lucide icon ${name}`).toBe(false);
      }
    }
  });
});

describe("the home page's motion is prepared when it is near, not all at hydration", () => {
  const gsapSource = read("lib/gsap.ts");

  /** Every file that calls the plain hook, with how many times. */
  function plainUses(): Record<string, number> {
    const out: Record<string, number> = {};
    const walk = (dir: string) => {
      for (const name of readdirSync(join(ROOT, dir))) {
        if (name === "node_modules" || name === ".next") continue;
        const rel = join(dir, name);
        if (statSync(join(ROOT, rel)).isDirectory()) walk(rel);
        else if (/\.tsx$/.test(name)) {
          const n = code(rel).match(/\buseGSAP\(/g)?.length ?? 0;
          if (n > 0) out[rel] = n;
        }
      }
    };
    for (const dir of ["app", "components"]) walk(dir);
    return out;
  }

  it("has a hook that creates a section's animations when it comes within a screen of the viewport", () => {
    expect(gsapSource).toContain("export function useGSAPWhenNear(");
    expect(gsapSource).toContain("new IntersectionObserver(");
    expect(gsapSource).toContain("rootMargin: NEAR");
    expect(gsapSource).toMatch(/export const NEAR = "100% 0px"/);
    // Created once, reverted with the component, and created at once where there is no observer.
    expect(gsapSource).toContain("observer.disconnect();");
    expect(gsapSource).toContain("context?.revert()");
    expect(gsapSource).toContain('typeof IntersectionObserver === "undefined"');
  });

  it("is used by every section that sits below the fold", () => {
    const near = [
      "components/motion/reveal.tsx",
      "components/motion/stagger.tsx",
      "components/motion/section-heading.tsx",
      "components/motion/cascade.tsx",
      "components/motion/line-draw.tsx",
      "components/motion/parallax.tsx",
      "components/motion/logo.tsx",
      "components/landing/agent-loop.tsx",
      "components/landing/comparison.tsx",
      "components/landing/cta.tsx",
      "components/landing/stats-strip.tsx",
      "components/landing/ui-showcase.tsx",
    ];
    for (const file of near) {
      expect(code(file), file).toContain("useGSAPWhenNear(");
      expect(code(file), file).not.toMatch(/\buseGSAP\(/);
    }
  });

  it("leaves plain useGSAP to what is on screen at load, or has to re-run when a value changes", () => {
    // A new component that calls `useGSAP` directly builds its animations in the hydration task,
    // where each one adds a forced reflow. Add it here only if it is in the first screen or takes
    // `dependencies`; otherwise use `useGSAPWhenNear`.
    expect(Object.keys(plainUses()).sort()).toEqual([
      "components/landing/site-header.tsx", // the header: on screen at load
      "components/landing/tool-catalogue.tsx", // re-runs when the filter changes
      "components/motion/counter.tsx", // re-runs when its value changes
      "components/motion/magnetic.tsx", // pointer listeners only
      "components/motion/micro-interactions.tsx", // loaded after paint by SiteMotion
      "components/motion/scroll-progress.tsx", // loaded after paint by SiteMotion
      "components/motion/split-heading.tsx", // the page's own heading
      "components/motion/tilt-card.tsx", // pointer listeners only
      "components/story/scroll-story.tsx", // the hero: on screen at load
      "components/ui/accordion.tsx", // opens on a press
    ]);
  });
});
