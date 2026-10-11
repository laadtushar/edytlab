/**
 * The website's design tokens, read from the website's own files.
 *
 * Nothing here is typed in by eye. The colour, radius and shadow values
 * are parsed out of `website/app/globals.css` each time the visuals are
 * rendered, so a change to the site's palette reaches the next render and
 * a change that breaks the parse fails the render instead of drifting.
 *
 *   website/app/globals.css          the `:root` and `.dark` token blocks
 *                                    (colours, `--radius`), and the rules
 *                                    `.gradient-text`, `.glow`, `.aurora`,
 *                                    `.surface`
 *   website/components/motion/logo.tsx   the logo's viewBox
 *   website/public/logo.svg          the logo's shapes and fills
 *   fonts/inter-latin-variable.woff2 the file the live site serves for
 *                                    Inter (`next/font`, `app/layout.tsx`)
 *
 * What the site keeps in Tailwind classes rather than in `globals.css`
 * (type sizes, tracking, the `rounded-xl` card) is in `base.css`, each
 * with a comment naming the component it was read from.
 */

import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = join(HERE, "../../..");
const SITE = join(REPO, "website");

const read = (path) => readFileSync(path, "utf8");

/** The body of the first `selector { ... }` rule, braces matched. */
function ruleBody(css, selector) {
  // The selector must start its line and not continue a selector list, so
  // `.aurora::after` does not match the tail of the shared
  // `.aurora::before,\n .aurora::after` rule.
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&").replace(/\n\s*/g, "\\s*");
  const re = new RegExp(`(?:^|\\n)[ \\t]*${escaped}\\s*\\{`, "g");
  let found;
  while ((found = re.exec(css))) {
    if (!css.slice(0, found.index).trimEnd().endsWith(",")) break;
  }
  if (!found) throw new Error(`website/app/globals.css has no \`${selector}\` rule any more`);
  const open = found.index + found[0].length - 1;
  let depth = 0;
  for (let i = open; i < css.length; i++) {
    if (css[i] === "{") depth++;
    if (css[i] === "}" && --depth === 0) return css.slice(open + 1, i);
  }
  throw new Error(`unterminated rule \`${selector}\``);
}

/** `--name: value;` declarations of a rule body, in order. */
function declarations(body) {
  return [...body.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)].map((m) => [m[1], m[2].trim()]);
}

/** One property's value out of a rule body. */
function property(body, name, what) {
  const m = new RegExp(`(?:^|[\\s;{])${name}\\s*:\\s*([^;]+);`).exec(body);
  if (!m) throw new Error(`could not read \`${name}\` of ${what} in website/app/globals.css`);
  return m[1].replace(/\s+/g, " ").trim();
}

/** Mono face: the app's own, so every render is set in the same one. */
function geistMonoUrl() {
  const desktop = createRequire(join(REPO, "apps/desktop/package.json"));
  const dir = dirname(desktop.resolve("@fontsource-variable/geist-mono/package.json"));
  return pathToFileURL(join(dir, "files/geist-mono-latin-wght-normal.woff2")).href;
}

/**
 * The stylesheet every template starts from: fonts, then the site's
 * `:root` / `.dark` token blocks verbatim, then the values the site only
 * writes inside rules, published as custom properties.
 */
export function tokensCss() {
  const css = read(join(SITE, "app/globals.css"));

  const root = declarations(ruleBody(css, ":root"));
  const dark = declarations(ruleBody(css, ".dark"));
  const blocks = (sel, decls) => `${sel} {\n${decls.map(([k, v]) => `  ${k}: ${v};`).join("\n")}\n}`;

  const gradientText = property(ruleBody(css, ".gradient-text"), "background", ".gradient-text");
  const glow = property(ruleBody(css, ".glow"), "box-shadow", ".glow");
  const surface = property(ruleBody(css, ".surface"), "background", ".surface");

  // `.aurora::before` / `::after`: two blurred discs. The shared size,
  // blur and opacity are in the combined rule; the colours and corners in
  // the two that follow.
  const auroraShared = ruleBody(css, ".aurora::before,\n  .aurora::after");
  const auroraA = ruleBody(css, ".aurora::before");
  const auroraB = ruleBody(css, ".aurora::after");
  const aurora = {
    size: property(auroraShared, "width", ".aurora"),
    opacity: property(auroraShared, "opacity", ".aurora"),
    blur: /blur\(([^)]+)\)/.exec(property(auroraShared, "filter", ".aurora"))[1],
    a: { color: property(auroraA, "background", ".aurora::before"), top: property(auroraA, "top", ".aurora::before"), left: property(auroraA, "left", ".aurora::before") },
    b: { color: property(auroraB, "background", ".aurora::after"), right: property(auroraB, "right", ".aurora::after"), bottom: property(auroraB, "bottom", ".aurora::after") },
  };

  const fonts = `
/* The Inter the site serves: fonts/inter-latin-variable.woff2 (OFL, see fonts/Inter-OFL.txt). */
@font-face {
  font-family: "Inter";
  font-style: normal;
  font-weight: 100 900;
  font-display: block;
  src: url("${pathToFileURL(join(HERE, "fonts/inter-latin-variable.woff2")).href}") format("woff2");
}
/* The app's mono, from @fontsource-variable/geist-mono in apps/desktop. */
@font-face {
  font-family: "Geist Mono Variable";
  font-style: normal;
  font-weight: 100 900;
  font-display: block;
  src: url("${geistMonoUrl()}") format("woff2");
}`;

  const published = `
:root {
  /* Values the site writes inside rules, not as variables. */
  --gradient-text: ${gradientText};
  --glow: ${glow};
  --surface-lift: ${surface.split(", hsl(var(--card))")[0].replace(/^linear-gradient\(/, "linear-gradient(")};
  --aurora-size: ${aurora.size};
  --aurora-opacity: ${aurora.opacity};
  --aurora-blur: ${aurora.blur};
  --aurora-a: ${aurora.a.color};
  --aurora-a-top: ${aurora.a.top};
  --aurora-a-left: ${aurora.a.left};
  --aurora-b: ${aurora.b.color};
  --aurora-b-right: ${aurora.b.right};
  --aurora-b-bottom: ${aurora.b.bottom};
}`;

  return [fonts, blocks(":root", root), blocks(".dark", dark), published].join("\n\n");
}

/** The logo as inline SVG, from the site's own files. */
export function logoSvg(className = "logo") {
  const component = read(join(SITE, "components/motion/logo.tsx"));
  const viewBox = /viewBox="([^"]+)"/.exec(component)?.[1];
  const file = read(join(SITE, "public/logo.svg"));
  const shapes = /<g transform="[^"]*">([\s\S]*)<\/g>\s*<\/svg>/.exec(file)?.[1];
  if (!viewBox || !shapes) throw new Error("could not read the logo from website/");
  return `<svg class="${className}" viewBox="${viewBox}" aria-hidden="true" focusable="false">${shapes}</svg>`;
}

/** The released version, from the one place it is set. */
export function appVersion() {
  const conf = JSON.parse(read(join(REPO, "apps/desktop/src-tauri/tauri.conf.json")));
  return conf.version;
}

/**
 * The tool count, from the registry's own generated doc. The site's
 * stats strip carries the same number, and `website_tool_docs.rs`
 * keeps the two in step.
 */
export function toolCount() {
  const m = /Every one of the (\d+) tools/.exec(read(join(REPO, "docs/tools-reference.md")));
  if (!m) throw new Error("docs/tools-reference.md no longer states the tool count");
  return Number(m[1]);
}
