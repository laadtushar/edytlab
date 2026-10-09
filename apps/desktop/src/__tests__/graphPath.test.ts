import { describe, expect, it } from "vitest";

import { pathSteps } from "../lib/graph";
import type { GraphNode } from "../lib/tauri-bridge";

const node = (id: string, parent: string | null): GraphNode => ({
  id,
  parent,
  label: null,
  tool: null,
  created_at: "2026-10-09T00:00:00Z",
});

/** A load (L0), a second load (L1), a mute (M) off L1. */
const nodes = [node("L0", null), node("L1", "L0"), node("M", "L1")];

describe("pathSteps: the path taken, over a content-addressed graph (#398)", () => {
  it("a step down an existing edge is on the tree", () => {
    expect(pathSteps(nodes, ["L1", "M"])).toEqual([
      { id: "path:L1->M", source: "L1", target: "M", onTree: true },
    ]);
  });

  it("a step back to a state reached before is not: mute, then unmute", () => {
    // M's parent is L1, so M -> L1 is no edge of the graph.
    expect(pathSteps(nodes, ["L1", "M", "L1"])).toEqual([
      { id: "path:L1->M", source: "L1", target: "M", onTree: true },
      { id: "path:M->L1", source: "M", target: "L1", onTree: false },
    ]);
  });

  it("a step taken twice is drawn once", () => {
    const steps = pathSteps(nodes, ["L1", "M", "L1", "M", "L1"]);
    expect(steps.map((s) => s.id)).toEqual(["path:L1->M", "path:M->L1"]);
  });

  it("skips a step with an id the graph does not have", () => {
    expect(pathSteps(nodes, ["gone", "L1", "other", "M"])).toEqual([]);
    expect(pathSteps(nodes, ["L1", "gone"])).toEqual([]);
  });

  it("skips a step from a node to itself", () => {
    expect(pathSteps(nodes, ["L1", "L1", "M"]).map((s) => s.id)).toEqual(["path:L1->M"]);
  });

  it("has no steps for a path of fewer than two heads", () => {
    expect(pathSteps(nodes, [])).toEqual([]);
    expect(pathSteps(nodes, ["L1"])).toEqual([]);
  });

  it("a jump to an ancestor is off the tree: the edge points the other way", () => {
    expect(pathSteps(nodes, ["M", "L0"])).toEqual([
      { id: "path:M->L0", source: "M", target: "L0", onTree: false },
    ]);
  });
});
