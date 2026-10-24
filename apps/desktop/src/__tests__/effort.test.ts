/** The levels the ChatGPT plan's picker offers follow the model's list (#474 PR 6). */
import { describe, expect, it } from "vitest";

import { asEffort, levelsOffered } from "../lib/effort";

describe("levelsOffered", () => {
  it("keeps the listed levels the backend can save, in the API's order", () => {
    expect(levelsOffered(["high", "low", "medium"])).toEqual(["low", "medium", "high"]);
  });

  it("is empty for a model that lists none, so the picker is hidden", () => {
    expect(levelsOffered([])).toEqual([]);
  });

  it("drops a level the backend would refuse", () => {
    expect(levelsOffered(["minimal", "none", "low"])).toEqual(["low"]);
    expect(levelsOffered(["minimal"])).toEqual([]);
  });
});

describe("asEffort", () => {
  it("reads a saved level, and anything else as the model's default", () => {
    expect(asEffort("xhigh")).toBe("xhigh");
    expect(asEffort("minimal")).toBe("");
    expect(asEffort(null)).toBe("");
  });
});
