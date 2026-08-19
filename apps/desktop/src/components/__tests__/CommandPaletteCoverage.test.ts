/**
 * What the command palette can find.
 *
 * The palette is the app's discovery surface — its own header calls it
 * "a searchable launcher for the agent's tools" — and it had drifted to
 * covering about half of them. The gap was not random: it was the
 * features that shipped *after* the palette was written. Ducking,
 * described selection, punch-in, silence truncation and filler removal
 * were all reachable by typing a sentence into the chat and by no other
 * means, which for a user who has not read the changelog is
 * indistinguishable from not existing.
 *
 * These tests are the anti-drift guard. They assert by search term
 * rather than by tool name, because the palette deliberately holds
 * natural-language prompts rather than tool names — so what is being
 * checked is the thing that actually matters: *someone looking for this
 * can find it*.
 */

import { describe, expect, it } from "vitest";

import { COMMANDS } from "../CommandPalette";

/** What the palette's own filter does, so the tests search as a user does. */
function search(q: string) {
  const needle = q.toLowerCase().trim();
  return COMMANDS.filter(
    (c) =>
      c.label.toLowerCase().includes(needle) ||
      c.description.toLowerCase().includes(needle) ||
      c.prompt.toLowerCase().includes(needle) ||
      c.category.toLowerCase().includes(needle) ||
      (c.tags ?? []).some((t) => t.includes(needle)),
  );
}

/**
 * Terms a user would plausibly type, paired with the capability they
 * are looking for. Each one was unfindable before this change.
 */
const MUST_FIND: Array<[string, string]> = [
  // The reason to use this over a sidechain compressor (#168 §1).
  ["duck", "ducking music under speech"],
  ["sidechain", "ducking, by the name people know it by"],
  // Every range-taking tool becomes reachable by description (#168 §3).
  ["select", "resolving a described region into a selection"],
  // Podcast staples.
  ["silence", "silence handling"],
  ["filler", "filler-word removal"],
  ["um", "filler-word removal, by what you actually say"],
  // Interviews (#168 §2).
  ["speaker", "splitting a track by speaker"],
  ["diarize", "splitting by speaker, by its technical name"],
  // Recording (#203 §2).
  ["punch", "punch-in recording"],
  ["retake", "punch-in, by what it is for"],
  // Disk (#98) — the report and the thing that acts on it.
  ["disk", "disk usage and reclamation"],
  ["reclaim", "reclaiming disk space"],
  // Repair, searched by symptom rather than by tool name.
  ["sibilance", "de-essing"],
  ["harsh", "de-essing, by symptom"],
  ["click", "click removal"],
  // Routing.
  ["bus", "bus sends"],
  // Repeatability.
  ["recipe", "saving and applying a recipe"],
  ["batch", "running a chain over many files"],
];

describe("the palette can find what shipped", () => {
  it.each(MUST_FIND)("finds %j — %s", (term) => {
    expect(search(term).length).toBeGreaterThan(0);
  });
});

describe("the palette stays usable", () => {
  it("puts every command in a known category", () => {
    // A typo'd category silently drops the command out of the rendered
    // list — it is filtered by CATEGORY_ORDER, not appended to it.
    const known = new Set([
      "Volume",
      "Fades",
      "Effects",
      "Editing",
      "Speed & Pitch",
      "Analysis",
      "Tracks",
      "Export & Session",
    ]);
    const strays = COMMANDS.filter((c) => !known.has(c.category));
    expect(strays.map((c) => `${c.label} (${c.category})`)).toEqual([]);
  });

  it("has no two commands with the same label", () => {
    // Two rows reading the same thing is a coin flip for the user, and
    // the likeliest way to get one is adding an entry for a capability
    // that was already there under a different phrasing.
    const seen = new Map<string, number>();
    for (const c of COMMANDS) {
      seen.set(c.label, (seen.get(c.label) ?? 0) + 1);
    }
    const dupes = [...seen.entries()].filter(([, n]) => n > 1);
    expect(dupes).toEqual([]);
  });

  it("gives every command a prompt and a description", () => {
    const empty = COMMANDS.filter(
      (c) => !c.prompt.trim() || !c.description.trim(),
    );
    expect(empty.map((c) => c.label)).toEqual([]);
  });
});
