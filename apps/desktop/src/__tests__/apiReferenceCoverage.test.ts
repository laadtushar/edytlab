/**
 * `docs/api-reference.md` has to cover the whole bridge (#260).
 *
 * Its header says "Complete reference for all Tauri IPC commands and
 * the TypeScript bridge". It documented 62 of 94 exports. The 32
 * missing were not fringe: recording, templates, plugin install,
 * plan-first approval, clip manipulation, per-provider base URLs and
 * tool-progress streaming — whole subsystems that shipped after the
 * file was written, with no section in the table of contents at all.
 *
 * A contributor or a coding agent consulting it to find out whether an
 * IPC surface exists got a wrong answer for a third of the bridge, and
 * nothing anywhere checked: `grep -rn 'api-reference' .github/ crates/
 * apps/` returned nothing.
 *
 * This is the check. It is deliberately crude — read both files as
 * text and compare name sets — in the same spirit as the Rust-side
 * `website_tool_docs.rs`, because a parser clever enough to be elegant
 * is a parser that can silently stop matching.
 */

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const bridge = readFileSync(join(process.cwd(), "src", "lib", "tauri-bridge.ts"), "utf8");
const doc = readFileSync(join(process.cwd(), "..", "..", "docs", "api-reference.md"), "utf8");

/**
 * Every name the bridge exports as a callable.
 *
 * Both shapes are in use: `export const foo = (…) => invoke(…)` for
 * most of it, `export async function foo(…)` for a handful.
 */
function bridgeExports(): Set<string> {
  const names = new Set<string>();
  for (const m of bridge.matchAll(/^export (?:async )?function ([A-Za-z0-9_]+)/gm)) {
    names.add(m[1]);
  }
  for (const m of bridge.matchAll(/^export const ([A-Za-z0-9_]+)\s*=/gm)) {
    names.add(m[1]);
  }
  return names;
}

/**
 * Every name the reference documents.
 *
 * An entry is a `### \`name(…)\`` heading. Some headings cover a pair
 * — `` ### `getViewState() → …` · `saveViewState(…) → …` `` — so the
 * second name is picked up from after the separator.
 */
function documented(): Set<string> {
  const names = new Set<string>();
  for (const m of doc.matchAll(/^### `([A-Za-z0-9_]+)\(/gm)) names.add(m[1]);
  for (const m of doc.matchAll(/·\s*`([A-Za-z0-9_]+)\(/g)) names.add(m[1]);
  return names;
}

describe("docs/api-reference.md", () => {
  it("is being read by this test at all", () => {
    expect(bridge.length, "tauri-bridge.ts read as empty").toBeGreaterThan(1000);
    expect(doc.length, "api-reference.md read as empty").toBeGreaterThan(1000);
    // A regex that stopped matching would make every assertion below
    // vacuously true.
    expect(bridgeExports().size).toBeGreaterThan(80);
    expect(documented().size).toBeGreaterThan(80);
  });

  it("documents every function the bridge exports", () => {
    const missing = [...bridgeExports()].filter((n) => !documented().has(n)).sort();
    expect(
      missing,
      `these bridge exports have no entry in docs/api-reference.md, which ` +
        `calls itself a complete reference — add them or change the claim`,
    ).toEqual([]);
  });

  /**
   * The drift has only ever run one way, and it is worth keeping it
   * that way: an entry for a function nobody exports promises an IPC
   * surface that is not there, which is the worse of the two errors.
   */
  it("documents nothing the bridge does not export", () => {
    const invented = [...documented()].filter((n) => !bridgeExports().has(n)).sort();
    expect(
      invented,
      `docs/api-reference.md documents functions tauri-bridge.ts does not ` +
        `export — they were renamed or removed`,
    ).toEqual([]);
  });
});
