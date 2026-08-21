/**
 * Every shortcut the overlay advertises must actually be bound.
 *
 * The overlay listed `Ctrl+K — Command palette (all tools)` for weeks
 * while nothing in the app ever opened the palette: `paletteOpen` was
 * initialised to `false` and the only other reference set it back to
 * `false`. Eighty-seven commands sat behind a door with no handle, and
 * the help screen told people the handle was there.
 *
 * That is worse than an unbound key. An undiscoverable feature is
 * merely hidden; an advertised one that does nothing makes the user
 * doubt their keyboard.
 *
 * The guard is deliberately crude — read `App.tsx` and check the key
 * appears in its handler. It cannot prove the binding *works*, but it
 * catches the failure that actually happened: a row in the help table
 * with no corresponding branch in the handler at all.
 */

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

import { SHORTCUTS } from "../components/ShortcutsOverlay";

const app = readFileSync(join(process.cwd(), "src", "App.tsx"), "utf8");

if (app.trim().length === 0) {
  throw new Error("App.tsx read as empty — this guard would be vacuous");
}

/**
 * The key a row is really about, as it would appear in a `e.key === …`
 * comparison. `null` means the row documents something that is not a
 * window-level key binding, so there is nothing in App.tsx to find.
 */
function keyToken(keys: string): string | null {
  const table: Record<string, string | null> = {
    Space: " ",
    Home: "Home",
    End: "End",
    "← →": "ArrowLeft",
    "Shift+← →": "ArrowLeft",
    Escape: "Escape",
    "Ctrl+K": "k",
    "Ctrl+Z": "z",
    "Ctrl+Y / Ctrl+Shift+Z": "y",
    "+ / =": "+",
    "-": "-",
    "0": "0",
    "Ctrl/Cmd + E": "e",
    "Ctrl/Cmd + F": "f",
    L: "l",
    "?": "?",
    // Toolbar buttons, not keys — the overlay says so in the label.
    "↕+ / ↕−": null,
  };
  if (!(keys in table)) {
    throw new Error(
      `ShortcutsOverlay advertises "${keys}", which this test does not know ` +
        `how to look for. Add it to the table above (or map it to null if it ` +
        `is not a key binding) — an unmapped row is an unchecked promise.`,
    );
  }
  return table[keys];
}

describe("the shortcuts overlay does not promise keys that do nothing", () => {
  it.each(SHORTCUTS.map((s) => [s.keys, s.description] as const))(
    "%s — %s",
    (keys) => {
      const token = keyToken(keys);
      if (token === null) return;
      // Matches `e.key === "k"`, `e.key === "K"`, and the
      // `(e.key === "l" || e.key === "L")` form.
      //
      // The token is escaped because several of these keys are regex
      // metacharacters — `?`, `+` and `-` all appear in the overlay,
      // and interpolating them raw builds an invalid pattern rather
      // than a failing one.
      const esc = (s: string) => s.replace(/[.*+?^${}()|[\]\\-]/g, "\\$&");
      const pattern = new RegExp(
        `e\\.key\\s*===\\s*"(${esc(token)}|${esc(token.toUpperCase())})"`,
      );
      expect(
        pattern.test(app),
        `the overlay advertises "${keys}" but App.tsx never compares e.key ` +
          `against "${token}" — the row is a promise with no handler`,
      ).toBe(true);
    },
  );
});

describe("the command palette can actually be opened", () => {
  it("has something that sets paletteOpen true", () => {
    // The precise shape of the bug: the only writes were `false`.
    const opens =
      /setPaletteOpen\(\s*true\s*\)/.test(app) ||
      /setPaletteOpen\(\s*\([a-zA-Z]+\)\s*=>/.test(app);
    expect(
      opens,
      "nothing in App.tsx ever opens the command palette — `paletteOpen` " +
        "starts false and every other write sets it false",
    ).toBe(true);
  });
});
