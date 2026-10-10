/**
 * The site's typeface, from the loader to the rendered class.
 *
 * `next/font` only declares a CSS variable (`--font-sans`) on `<html>`;
 * nothing uses it until the Tailwind theme points `font-sans` at it.
 * Without that mapping the build is green and the page renders in
 * Tailwind's default `ui-sans-serif, system-ui` stack, with Inter
 * downloaded and never applied. No type check sees that, so this pins
 * both ends of the link: the variable the layout sets, and the
 * `font-family` Tailwind generates for the class `<body>` carries.
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import postcss from "postcss";
import tailwindcss from "tailwindcss";
import { describe, expect, it } from "vitest";

import tailwindConfig from "../tailwind.config";

const LAYOUT = readFileSync(
  fileURLToPath(new URL("../app/layout.tsx", import.meta.url)),
  "utf8",
);

/** The CSS variable the layout's `Inter(...)` loader declares. */
function loaderVariable(): string {
  const call = /\bInter\(\s*\{([^}]*)\}\s*\)/.exec(LAYOUT);
  if (!call) throw new Error("app/layout.tsx no longer calls Inter({ ... })");
  const variable = /\bvariable:\s*"([^"]+)"/.exec(call[1]);
  if (!variable) throw new Error("the Inter loader sets no `variable`");
  return variable[1];
}

/**
 * The `font-family` declaration Tailwind generates for one utility,
 * built from the site's real config and nothing else.
 */
async function fontFamilyOf(utility: string): Promise<string> {
  const result = await postcss([
    tailwindcss({
      ...tailwindConfig,
      content: [{ raw: `<p class="${utility}"></p>` }],
    }),
  ]).process("@tailwind utilities;", { from: undefined });
  let family: string | undefined;
  result.root.walkRules(`.${utility}`, (rule) => {
    rule.walkDecls("font-family", (decl) => {
      family = decl.value;
    });
  });
  if (family === undefined) throw new Error(`Tailwind emitted no .${utility} font-family`);
  return family;
}

describe("site typeface", () => {
  it("loads Inter into a CSS variable and puts that variable on <html>", () => {
    expect(loaderVariable()).toBe("--font-sans");
    expect(LAYOUT).toMatch(/<html[^>]*className=\{[^}]*\$\{inter\.variable\}/);
  });

  it("applies <body>'s font-sans through the variable the loader sets", async () => {
    expect(LAYOUT).toMatch(/<body[^>]*className="[^"]*\bfont-sans\b/);
    const family = await fontFamilyOf("font-sans");
    expect(family.startsWith(`var(${loaderVariable()})`)).toBe(true);
  });

  it("keeps a system stack behind Inter for the swap period", async () => {
    const family = await fontFamilyOf("font-sans");
    expect(family).toMatch(/^var\(--font-sans\),\s*ui-sans-serif,\s*system-ui/);
    expect(family).toMatch(/sans-serif/);
  });

  it("leaves font-mono on the system monospace stack, which nothing overrides", async () => {
    // The site loads no mono face, so there is no variable to map.
    expect(LAYOUT).not.toMatch(/--font-mono/);
    expect(await fontFamilyOf("font-mono")).toMatch(/^ui-monospace/);
  });
});
