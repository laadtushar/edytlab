/**
 * The palette is a set of promises to the user.
 *
 * Picking a command injects its prompt into the chat, and the label and
 * description are what the user believes will happen. Two things can go
 * wrong that a rendering test would not catch: a command can name a tool
 * that records state without touching the audio, and the keyboard's idea
 * of "the highlighted row" can drift from the rendered order.
 */

import { describe, expect, it } from "vitest";

import { COMMANDS, type Command } from "../components/CommandPalette";

/**
 * Tools whose value the render engine does not read. They carry
 * `applied_at_render: false` in their results; until that flips, a
 * command that steers the agent towards one of them reports success and
 * changes nothing.
 */
const INERT_TOOLS = ["align_to_beat"];

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
  it("does not promise anything the render engine ignores", () => {
    // The palette can't name tools directly — it sends prose — so this
    // looks for the phrasing that steers the agent to one of them.
    // Deliberately narrow. `analyze_track` genuinely reports a beat grid
    // and its command says so — reporting one is fine, warping audio onto
    // one is what nothing does. Match the action, not the noun.
    const inertPhrases = [/align.*\bbeat/i, /quantize/i];

    const offenders = COMMANDS.filter((c) =>
      inertPhrases.some(
        (re) => re.test(c.prompt) || re.test(c.description) || re.test(c.label),
      ),
    ).map((c) => `${c.label}: "${c.prompt}" / "${c.description}"`);

    expect(
      offenders,
      `these commands steer the agent at ${INERT_TOOLS.join(", ")}, which ` +
        `record a value the render engine never reads — the user is told it ` +
        `worked and hears no difference`,
    ).toEqual([]);
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
