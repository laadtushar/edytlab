/**
 * The app wears the site's brand, and keeps wearing it.
 *
 * The app and www.edytlab.com are one product with two front doors, and
 * they had drifted apart in the way a hand-copied palette does: the site
 * is violet on a zinc ground, the app was amber on a blue-grey one, and
 * the app's own icon (which is the site's logo) matched neither the UI
 * inside the window nor, in the corners that had been styled by hand, the
 * UI's own tokens. About a dozen call sites typed `#ffa05f` for a hover
 * because there was no token for one.
 *
 * Three things are pinned here, because each rots silently:
 *
 *   1. The tokens that mirror `website/app/globals.css` `.dark` still equal
 *      it. Change one, change the other, or this fails.
 *   2. The old palette's literals do not creep back into a component.
 *      Nothing about `#ffa05f` is a type error or a lint failure.
 *   3. Every text pair the tokens produce clears WCAG AA, and the focus
 *      ring and input edges clear 3:1 (1.4.11). `--text-faint` was 2.9:1 on
 *      the raised surface and used 111 times.
 *
 * Read from disk rather than imported: `css: false` stubs stylesheet
 * imports out, and `?raw` does not escape that (see motion.test.tsx).
 */

import { readdirSync, readFileSync } from "node:fs";
import { join, relative, sep } from "node:path";
import { describe, expect, it } from "vitest";

const srcDir = join(process.cwd(), "src");
const repoRoot = join(process.cwd(), "..", "..");

const css = readFileSync(join(srcDir, "styles.css"), "utf8");
const siteCss = readFileSync(join(repoRoot, "website", "app", "globals.css"), "utf8");

if (css.trim().length === 0 || siteCss.trim().length === 0) {
  throw new Error("a stylesheet read as empty — the guards below would be vacuous");
}

/** CSS with comments removed, so prose about a token is not read as the token. */
const strip = (s: string) => s.replace(/\/\*[\s\S]*?\*\//g, "");

/** The body of the first `selector { … }` block (no nested braces). */
function block(source: string, selector: string): string {
  const m = new RegExp(`${selector}\\s*\\{([^}]*)\\}`).exec(strip(source));
  if (!m) throw new Error(`no ${selector} block`);
  return m[1];
}

const root = block(css, ":root");

/** The raw value of an app token, e.g. `hsl(270 95% 70%)`. */
function token(name: string): string {
  const m = new RegExp(`${name}:\\s*([^;]+);`).exec(root);
  if (!m) throw new Error(`${name} is not defined in :root`);
  return m[1].trim();
}

// -- colour maths ---------------------------------------------------------

type Rgba = [number, number, number, number];

function hslToRgb(h: number, s: number, l: number): [number, number, number] {
  const sat = s / 100;
  const lig = l / 100;
  const k = (n: number) => (n + h / 30) % 12;
  const a = sat * Math.min(lig, 1 - lig);
  const f = (n: number) => lig - a * Math.max(-1, Math.min(k(n) - 3, Math.min(9 - k(n), 1)));
  return [Math.round(f(0) * 255), Math.round(f(8) * 255), Math.round(f(4) * 255)];
}

/** Parses `hsl(h s% l%)`, `hsl(h s% l% / a)` and `#rrggbb`. */
function parse(value: string): Rgba {
  const hsl = /^hsl\(\s*([\d.]+)\s+([\d.]+)%\s+([\d.]+)%\s*(?:\/\s*([\d.]+))?\s*\)$/.exec(value);
  if (hsl) {
    const [r, g, b] = hslToRgb(Number(hsl[1]), Number(hsl[2]), Number(hsl[3]));
    return [r, g, b, hsl[4] === undefined ? 1 : Number(hsl[4])];
  }
  const hex = /^#([0-9a-f]{6})$/i.exec(value);
  if (hex) {
    const n = parseInt(hex[1], 16);
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255, 1];
  }
  throw new Error(`cannot parse colour: ${value}`);
}

const color = (name: string) => parse(token(name));

/** `fg` laid over an opaque `bg`. */
function over(fg: Rgba, bg: Rgba): Rgba {
  const [r, g, b, a] = fg;
  return [r, g, b].map((v, i) => Math.round(v * a + bg[i] * (1 - a))).concat(1) as Rgba;
}

function luminance([r, g, b]: Rgba): number {
  const ch = (v: number) => {
    const c = v / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * ch(r) + 0.7152 * ch(g) + 0.0722 * ch(b);
}

function ratio(a: Rgba, b: Rgba): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

// -- 1. the mirror --------------------------------------------------------

describe("the tokens that mirror the website", () => {
  const dark = block(siteCss, "\\.dark");
  /** A site token's HSL triplet as the app writes it: `hsl(270 95% 70%)`. */
  const site = (name: string) => {
    const m = new RegExp(`${name}:\\s*([^;]+);`).exec(dark);
    if (!m) throw new Error(`the site has no ${name} in .dark`);
    return `hsl(${m[1].trim()})`;
  };

  it("--accent is the site's --primary", () => {
    expect(token("--accent")).toBe("hsl(270 95% 70%)");
    expect(token("--accent")).toBe(site("--primary"));
  });

  it.each([
    ["--bg", "--background"],
    ["--surface", "--card"],
    ["--surface-elev-2", "--secondary"],
    ["--border", "--border"],
    ["--text", "--foreground"],
    ["--text-dim", "--muted-foreground"],
    ["--accent-fg", "--primary-foreground"],
  ])("%s is the site's %s", (app, theirs) => {
    expect(token(app)).toBe(site(theirs));
  });

  it("the focus ring is the accent, with a gap in the page colour", () => {
    // The site's `ring-2 ring-offset-2`: 2px of background, then 2px of
    // violet. Written on the tokens so it follows an accent change.
    const ring = token("--ring-focus");
    expect(ring).toContain("var(--accent)");
    expect(ring).toContain("var(--bg)");
  });

  it("the interface is dark only", () => {
    expect(root).toMatch(/color-scheme:\s*dark/);
  });

  it("declares every token the components lean on", () => {
    for (const name of [
      "--accent-fg",
      "--accent-hover",
      "--accent-text",
      "--accent-soft",
      "--accent-glow",
      "--accent-border",
      "--accent-selection",
      "--accent-2",
      "--gradient-brand",
      "--border-input",
      "--warning-soft",
      "--warning-border",
      "--danger-soft",
      "--danger-border",
      "--shadow-pop",
      "--shadow-dialog",
      "--shadow-accent",
    ]) {
      expect(() => token(name), name).not.toThrow();
    }
  });
});

// -- 2. the old palette stays out ------------------------------------------

/** Every non-test source file under `src`. */
function sources(dir = srcDir): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return entry.name === "__tests__" ? [] : sources(path);
    return /\.(tsx?|css)$/.test(entry.name) && !/\.test\.tsx?$/.test(entry.name) ? [path] : [];
  });
}

describe("the old palette stays out of the app", () => {
  const forbidden: [string, RegExp][] = [
    ["the hand-typed amber hover", /#ffa05f/i],
    ["the amber accent, as rgb", /255, ?138, ?61/],
    ["the amber accent, as hex", /ff8a3d/i],
    ["the never-defined --onyx-0 token", /--onyx-0/],
    ["the never-defined --warn token", /var\(--warn,/],
    ["a Tailwind default neutral", /neutral-[0-9]/],
    ["a Tailwind default amber", /amber-[0-9]/],
    ["hand-typed slate / amber hex", /#0f172a|#1e293b|#334155|#f59e0b/i],
    ["the graph's old blue", /blue-500|3b82f6/],
    ["the serif face that left with the old wordmark", /font-serif|Instrument/],
  ];

  const files = sources();

  it("scans the source tree it thinks it does", () => {
    // A walker that finds nothing would make every case below pass.
    const names = files.map((f) => relative(srcDir, f).split(sep).join("/"));
    expect(names).toContain("styles.css");
    expect(names).toContain("components/AppHeader.tsx");
    expect(names).toContain("components/Timeline.tsx");
    expect(names.some((n) => n.includes("__tests__"))).toBe(false);
  });

  it.each(forbidden)("has no trace of %s", (_why, pattern) => {
    const hits = files
      .filter((f) => pattern.test(readFileSync(f, "utf8")))
      .map((f) => relative(srcDir, f).split(sep).join("/"));
    expect(hits).toEqual([]);
  });
});

// -- 3. contrast ------------------------------------------------------------

describe("contrast (WCAG 2.x)", () => {
  const surfaces = ["--bg", "--surface", "--surface-elev", "--surface-elev-2"];

  describe.each(surfaces)("on %s", (surface) => {
    const bg = () => color(surface);

    it.each(["--text", "--text-dim", "--text-faint", "--accent", "--accent-text"])(
      "%s is readable text",
      (fg) => {
        expect(ratio(color(fg), bg())).toBeGreaterThanOrEqual(4.5);
      },
    );

    it.each(["--success", "--warning", "--danger"])("%s status text is readable", (fg) => {
      expect(ratio(color(fg), bg())).toBeGreaterThanOrEqual(4.5);
    });

    it("text input edges are visible (1.4.11)", () => {
      expect(ratio(color("--border-input"), bg())).toBeGreaterThanOrEqual(3);
    });

    it("the focus ring is visible against it (1.4.11)", () => {
      expect(ratio(color("--accent"), bg())).toBeGreaterThanOrEqual(3);
    });
  });

  it("button text on the accent, resting and hovered", () => {
    expect(ratio(color("--accent-fg"), color("--accent"))).toBeGreaterThanOrEqual(4.5);
    expect(ratio(color("--accent-fg"), color("--accent-hover"))).toBeGreaterThanOrEqual(4.5);
  });

  it("accent text and dim text on an accent tint", () => {
    // Raw `--accent` on a tint over the raised inset is 4.2:1 and fails,
    // which is why `--accent-text` exists.
    for (const surface of surfaces) {
      const tint = over(color("--accent-soft"), color(surface));
      expect(ratio(color("--accent-text"), tint), `accent-text on tint over ${surface}`).toBeGreaterThanOrEqual(4.5);
      expect(ratio(color("--text-dim"), tint), `text-dim on tint over ${surface}`).toBeGreaterThanOrEqual(4.5);
    }
  });

  it("keeps --text-dim and --text-faint distinct", () => {
    // Raising the floor to AA must not flatten the hierarchy.
    const elev = color("--surface-elev");
    expect(ratio(color("--text-dim"), elev) - ratio(color("--text-faint"), elev)).toBeGreaterThan(1);
  });
});
