/**
 * The palette is a set of promises to the user.
 *
 * Picking a command injects its prompt into the chat, and the label and
 * description are what the user believes will happen. Two things can go
 * wrong that a rendering test would not catch: a command can promise
 * something the tool behind it does not do, and the keyboard's idea of
 * "the highlighted row" can drift from the rendered order.
 */

import { describe, expect, it } from "vitest";

import { COMMANDS, type Command } from "../components/CommandPalette";

/** The order the results list renders groups in. */
const CATEGORY_ORDER = [
  "Volume",
  "Fades",
  "Effects",
  "Editing",
  "Speed & Pitch",
  "Analysis",
  "Tracks",
  "Export & Session",
];

describe("COMMANDS", () => {
  it("offers beat alignment, now that something warps audio to a grid", () => {
    // This test used to assert the opposite: that no command steered the
    // agent at `align_to_beat`, because that tool recorded a grid the
    // render engine never read and the user was told it worked while
    // hearing no difference. #97 made it warp, so the entry is back.
    //
    // The underlying invariant — no tool may report a change it does not
    // make — did not go away, it moved somewhere it can be checked
    // properly: `no_tool_still_claims_it_is_unapplied` in
    // `crates/tools/tests/tools_integration.rs` reads every registered
    // tool's own description rather than pattern-matching palette prose.
    const aligners = COMMANDS.filter(
      (c) => /align.*\bbeat/i.test(c.label) || /warp/i.test(c.description),
    );
    expect(aligners.length).toBeGreaterThan(0);
    for (const c of aligners) {
      expect(
        c.description.toLowerCase(),
        `"${c.label}" should say what happens to pitch`,
      ).toMatch(/pitch/);
    }
  });

  it("says what happens to pitch in every speed command", () => {
    // Two different things live in this group now: time-stretch, which
    // holds pitch, and change_speed, which does not. A user picking one
    // has to be able to tell which they are getting.
    const speed = COMMANDS.filter((c) => c.category === "Speed & Pitch");
    expect(speed.length).toBeGreaterThan(0);
    for (const c of speed) {
      expect(
        c.description.toLowerCase(),
        `"${c.label}" should say what happens to the pitch`,
      ).toMatch(/pitch/);
    }
  });

  it("keeps categories contiguous and in the rendered order", () => {
    // Arrow-key navigation indexes the flat filtered list, which is
    // COMMANDS order, while the list renders grouped in CATEGORY_ORDER.
    // Those two agree only while COMMANDS is laid out in category blocks
    // in that same order. If they drift, the highlighted row and the row
    // Enter selects become different commands — silently.
    const seen: string[] = [];
    for (const c of COMMANDS) {
      if (seen[seen.length - 1] !== c.category) {
        expect(
          seen,
          `category "${c.category}" appears in more than one block`,
        ).not.toContain(c.category);
        seen.push(c.category);
      }
    }
    expect(seen).toEqual(CATEGORY_ORDER.filter((c) => seen.includes(c)));
  });

  it("gives every command the fields the list renders", () => {
    for (const c of COMMANDS as Command[]) {
      expect(c.label.trim()).not.toBe("");
      expect(c.prompt.trim()).not.toBe("");
      expect(c.description.trim()).not.toBe("");
      expect(CATEGORY_ORDER, `unknown category "${c.category}"`).toContain(
        c.category,
      );
    }
  });

  it("has no duplicate labels within a category", () => {
    const byCategory = new Map<string, string[]>();
    for (const c of COMMANDS) {
      const bucket = byCategory.get(c.category) ?? [];
      bucket.push(c.label);
      byCategory.set(c.category, bucket);
    }
    for (const [category, labels] of byCategory) {
      expect(new Set(labels).size, `duplicate label in ${category}`).toBe(
        labels.length,
      );
    }
  });
});
