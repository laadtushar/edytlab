/**
 * The site's typeface, from the loader to the rendered class.
 *
 * `next/font` only declares a CSS variable (`--font-inter`) on `<html>`;
 * nothing uses it until the Tailwind theme points `font-sans` at it
 * (`@theme` in `app/globals.css`). Without that mapping the build is
 * green and the page renders in Tailwind's default `ui-sans-serif,
 * system-ui` stack, with Inter downloaded and never applied. No type
 * check sees that, so this pins both ends of the link: the variable the
 * layout sets, and the `font-family` Tailwind generates for the class
 * `<body>` carries, compiled from the site's real stylesheet.
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import tailwindcss from "@tailwindcss/postcss";
import postcss from "postcss";
import { describe, expect, it } from "vitest";

const LAYOUT = readFileSync(
  fileURLToPath(new URL("../app/layout.tsx", import.meta.url)),
  "utf8",
);
const GLOBALS = fileURLToPath(new URL("../app/globals.css", import.meta.url));

/** The CSS variable the layout's `Inter(...)` loader declares. */
function loaderVariable(): string {
  const call = /\bInter\(\s*\{([^}]*)\}\s*\)/.exec(LAYOUT);
  if (!call) throw new Error("app/layout.tsx no longer calls Inter({ ... })");
  const variable = /\bvariable:\s*"([^"]+)"/.exec(call[1]);
  if (!variable) throw new Error("the Inter loader sets no `variable`");
  return variable[1];
}

let compiled: Promise<postcss.Root> | undefined;

/**
 * `app/globals.css` through Tailwind, with the two font classes asked for
 * by name (`@source inline`), so the answer does not hang on which files
 * happen to use them.
 */
function compiledCss(): Promise<postcss.Root> {
  compiled ??= postcss([tailwindcss({ optimize: false })])
    .process(`${readFileSync(GLOBALS, "utf8")}\n@source inline("font-sans font-mono");\n`, {
      from: GLOBALS,
    })
    .then((result) => result.root);
  return compiled;
}

/**
 * The `font-family` Tailwind generates for one utility, with the theme's
 * own variables (`--font-mono`, say) written out. A variable the theme
 * does not define, such as the loader's, is left as `var(...)`.
 */
async function fontFamilyOf(utility: string): Promise<string> {
  const root = await compiledCss();
  const theme = new Map<string, string>();
  root.walkRules(/^:root, :host$/, (rule) => {
    rule.walkDecls(/^--/, (decl) => {
      theme.set(decl.prop, decl.value);
    });
  });
  let family: string | undefined;
  root.walkRules(`.${utility}`, (rule) => {
    rule.walkDecls("font-family", (decl) => {
      family = decl.value;
    });
  });
  if (family === undefined) throw new Error(`Tailwind emitted no .${utility} font-family`);
  for (let i = 0; i < 10; i++) {
    family = family.replace(/var\((--[\w-]+)\)/g, (whole, name: string) => theme.get(name) ?? whole);
  }
  return family;
}

describe("site typeface", () => {
  it("loads Inter into a CSS variable and puts that variable on <html>", () => {
    expect(loaderVariable()).toBe("--font-inter");
    expect(LAYOUT).toMatch(/<html[^>]*className=\{[^}]*\$\{inter\.variable\}/);
  });

  it("applies <body>'s font-sans through the variable the loader sets", async () => {
    expect(LAYOUT).toMatch(/<body[^>]*className="[^"]*\bfont-sans\b/);
    const family = await fontFamilyOf("font-sans");
    expect(family.startsWith(`var(${loaderVariable()})`)).toBe(true);
  });

  it("keeps a system stack behind Inter for the swap period", async () => {
    const family = await fontFamilyOf("font-sans");
    expect(family).toMatch(/^var\(--font-inter\),\s*ui-sans-serif,\s*system-ui/);
    expect(family).toMatch(/sans-serif/);
  });

  it("leaves font-mono on the system monospace stack, which nothing overrides", async () => {
    // The site loads no mono face, so there is no variable to map.
    expect(LAYOUT).not.toMatch(/--font-mono/);
    expect(await fontFamilyOf("font-mono")).toMatch(/^ui-monospace/);
  });
});
