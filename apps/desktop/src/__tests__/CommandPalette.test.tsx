/**
 * The palette is a set of promises to the user.
 *
 * Picking a command injects its prompt into the chat, and the label and
 * description are what the user believes will happen. Two things can go
 * wrong that a rendering test would not catch: a command can promise
 * something the tool behind it does not do, and the keyboard's idea of
 * "the highlighted row" can drift from the rendered order.
 */

import { readFileSync } from "node:fs";
import { join } from "node:path";
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

/**
 * Effect kinds a chain (a track's, a bus's or the master's) can run today.
 *
 * Source of truth: the `STREAMABLE` table in
 * `crates/audio-engine/src/effect_chain.rs`. `add_effect` asks that table
 * and refuses every other kind (`NOT_YET_STREAMING` there), so a palette
 * entry that has the agent put one in a chain invites an action the tool
 * will refuse. The last test in this block reads the Rust table and fails
 * when this list and it disagree.
 */
const STREAMABLE_CHAIN_KINDS = [
  "gain",
  "limiter",
  "low_pass_filter",
  "high_pass_filter",
  "notch_filter",
];

/**
 * How a prompt names each effect kind: the streamable ones above, and the
 * eleven the renderer cannot run in a chain yet (`NOT_YET_STREAMING` in
 * the same Rust file). A kind missing from this table is a kind the guard
 * cannot see, so a new one goes here when it is added to either list.
 */
const KIND_WORDS: Record<string, RegExp> = {
  gain: /\bgain\b/i,
  limiter: /\blimit(?:er|ing)?\b/i,
  low_pass_filter: /\blow[- ]?pass\b/i,
  high_pass_filter: /\bhigh[- ]?pass\b/i,
  notch_filter: /\bnotch\b/i,
  tremolo: /\btremolo\b/i,
  phaser: /\bphaser\b/i,
  echo: /\becho\b|\bdelay\b/i,
  reverb: /\breverb\b/i,
  noise_gate: /\b(?:noise )?gate\b/i,
  leveler: /\blevell?er\b/i,
  de_esser: /\bde-?ess/i,
  distortion: /\bdistort|\bsaturat/i,
  stereo_widener: /\bwiden/i,
  eq: /\beq\b|\bequali[sz]/i,
  compressor: /\bcompress/i,
};

/**
 * Whether a prompt sends the agent to a chain: a bus (whose only effects
 * are chain effects), a track's or the master's effect chain, or one of
 * its entries. The destructive tools (`reverb`, `eq`, `compressor`...)
 * take no chain and stay free to name any effect.
 */
function usesAChain(c: Command): boolean {
  return /\bbus(?:es)?\b|\beffects?\b|\bchain\b|\bmaster\b/i.test(c.prompt);
}

/** The effect kinds a command's label, prompt and description name. */
function namedKinds(c: Command): string[] {
  const text = `${c.label} ${c.prompt} ${c.description}`;
  return Object.entries(KIND_WORDS)
    .filter(([, words]) => words.test(text))
    .map(([kind]) => kind);
}

describe("COMMANDS that use an effect chain", () => {
  it("name only effects a chain can run", () => {
    // `add_effect` refuses reverb, echo, EQ, compressor and the rest on
    // any chain, so "create a reverb bus" had the agent make a bus, then
    // fail to put the reverb on it. A suggestion the first tool call of
    // which is refused is a promise the app cannot keep.
    const refused = COMMANDS.filter(usesAChain).flatMap((c) =>
      namedKinds(c)
        .filter((kind) => !STREAMABLE_CHAIN_KINDS.includes(kind))
        .map((kind) => `"${c.label}" names ${kind}: ${c.prompt}`),
    );
    expect(refused).toEqual([]);
  });

  it("includes the bus suggestion, and gives it an effect a bus can run", () => {
    // Guards the guard: if the bus entry were dropped or reworded out of
    // `usesAChain`, the test above would pass without checking it.
    const bus = COMMANDS.find((c) => c.label === "Send tracks to a bus");
    expect(bus, "the bus suggestion is gone").toBeDefined();
    expect(usesAChain(bus as Command)).toBe(true);
    const kinds = namedKinds(bus as Command);
    expect(kinds.length, "name an effect so the bus is not empty").toBeGreaterThan(0);
    for (const kind of kinds) {
      expect(STREAMABLE_CHAIN_KINDS).toContain(kind);
    }
  });

  it("recognises the suggestion this guard exists for", () => {
    const old: Command = {
      category: "Tracks",
      label: "Send tracks to a bus",
      prompt: "create a reverb bus and send track 1 to it at -6 dB",
      description: "Shared processing for several tracks",
    };
    expect(usesAChain(old)).toBe(true);
    expect(namedKinds(old)).toEqual(["reverb"]);
  });

  it("uses the kinds the renderer streams", () => {
    // Keeps STREAMABLE_CHAIN_KINDS honest: read the Rust table rather than
    // trust a copy. The slice runs from the table's opening to
    // its closing `];`, and each entry there is `kind: "<name>"`.
    const source = readFileSync(
      join(process.cwd(), "..", "..", "crates", "audio-engine", "src", "effect_chain.rs"),
      "utf8",
    );
    const start = source.indexOf("const STREAMABLE: &[Streamable] = &[");
    expect(start, "STREAMABLE moved: update this test").toBeGreaterThan(-1);
    const table = source.slice(start, source.indexOf("\n];", start));
    const kinds = [...table.matchAll(/kind: "([a-z_]+)"/g)].map((m) => m[1]);
    expect([...kinds].sort()).toEqual([...STREAMABLE_CHAIN_KINDS].sort());
    for (const kind of kinds) {
      expect(KIND_WORDS, `no words for ${kind}`).toHaveProperty(kind);
    }
  });
});
