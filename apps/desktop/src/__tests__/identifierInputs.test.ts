/**
 * Identifier fields switch the system's text assistance off (#735).
 *
 * macOS autocorrect capitalised `test-delete` into `Test-delete.md`. Every
 * `<input>` and `<textarea>` in the editors and Settings must either spread
 * `IDENTIFIER_INPUT` (autoCapitalize, autoCorrect off, spellCheck false) or
 * be named here as prose or a non-text control, so a new field has to decide.
 */

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { IDENTIFIER_INPUT } from "../lib/identifierInput";

const FILES = [
  "SkillsEditor",
  "AgentProfilesEditor",
  "McpServersEditor",
  "Settings",
];

/** Free prose and non-text controls, which keep the defaults. */
const NOT_IDENTIFIERS = new Set([
  "skills-description",
  "skills-body",
  "skills-trigger",
  "skills-enabled",
  "profiles-description",
  "profiles-body",
  "profiles-active-toggle",
  "mcp-transport",
  "mcp-enabled",
]);

function fields(source: string): { tag: string; text: string }[] {
  const out: { tag: string; text: string }[] = [];
  for (const m of source.matchAll(/<(input|textarea)\b/g)) {
    // The tag ends at the first `>` that is not the arrow of `=>`.
    const rest = source.slice(m.index);
    const end = rest.search(/(?<!=)>/);
    out.push({ tag: m[1], text: rest.slice(0, end) });
  }
  return out;
}

describe("identifier inputs", () => {
  it("turn autocapitalize, autocorrect and spellcheck off", () => {
    expect(IDENTIFIER_INPUT).toEqual({
      autoCapitalize: "off",
      autoCorrect: "off",
      spellCheck: false,
    });
  });

  for (const name of FILES) {
    const source = readFileSync(join(process.cwd(), "src/components", `${name}.tsx`), "utf8");
    it(`${name}: every field spreads IDENTIFIER_INPUT or is listed as prose`, () => {
      const bad = fields(source).filter(({ text }) => {
        if (text.includes("{...IDENTIFIER_INPUT}")) return false;
        const id = /data-testid="([^"]+)"/.exec(text)?.[1];
        if (id && NOT_IDENTIFIERS.has(id)) return false;
        if (/type="(checkbox|radio)"/.test(text)) return false;
        return true;
      });
      expect(bad.map((b) => b.text.slice(0, 120))).toEqual([]);
    });
  }
});
