/**
 * Pure tests for `lib/graph.ts`. These don't require jsdom — but we
 * leave them in the default vitest environment because the rest of
 * the suite runs there too and toggling environments per file is more
 * trouble than it saves.
 */

import { describe, expect, it } from "vitest";

import {
  colourLineage,
  formatRelative,
  layoutDagre,
  nodeLabel,
} from "../graph";
import type { GraphNode } from "../tauri-bridge";

const node = (
  id: string,
  parent: string | null,
  extras: Partial<GraphNode> = {},
): GraphNode => ({
  id,
  parent,
  label: null,
  tool: null,
  created_at: new Date().toISOString(),
  ...extras,
});

describe("colourLineage", () => {
  it("assigns one hue per leaf and propagates it back to the root", () => {
    // a -> b -> c (leaf-1)
    //      \-> d (leaf-2)
    const nodes = [
      node("a", null),
      node("b", "a"),
      node("c", "b"),
      node("d", "b"),
    ];
    const colours = colourLineage(nodes);
    expect(colours.size).toBe(4);
    // Leaves get distinct colours; the shared trunk (a, b) gets one
    // of them.
    expect(colours.get("c")).not.toBe(colours.get("d"));
    expect([colours.get("c"), colours.get("d")]).toContain(colours.get("a"));
  });

  it("returns an empty map for an empty graph", () => {
    expect(colourLineage([]).size).toBe(0);
  });
});

describe("layoutDagre", () => {
  it("produces a position for every node and lays roots above leaves", () => {
    const nodes = [node("a", null), node("b", "a"), node("c", "b")];
    const { positions } = layoutDagre(nodes);
    expect(positions.size).toBe(3);
    const ya = positions.get("a")!.y;
    const yc = positions.get("c")!.y;
    expect(ya).toBeLessThan(yc);
  });
});

describe("formatRelative", () => {
  it("buckets timestamps into the expected windows", () => {
    const now = new Date("2026-01-01T12:00:00Z");
    expect(formatRelative("2026-01-01T11:59:58Z", now)).toBe("just now");
    expect(formatRelative("2026-01-01T11:59:50Z", now)).toBe("10s ago");
    expect(formatRelative("2026-01-01T11:58:00Z", now)).toBe("2m ago");
    expect(formatRelative("2026-01-01T10:00:00Z", now)).toBe("2h ago");
    expect(formatRelative("2025-12-30T12:00:00Z", now)).toBe("2d ago");
  });

  it("returns empty string for an unparseable timestamp", () => {
    expect(formatRelative("not a date")).toBe("");
  });
});

describe("nodeLabel", () => {
  it("falls through label -> tool words -> id prefix", () => {
    expect(nodeLabel(node("abcdef0", null, { label: "load /foo.wav" }))).toBe(
      "load /foo.wav",
    );
    expect(
      nodeLabel(node("abcdef0", null, { label: null, tool: "normalize" })),
    ).toBe("normalize");
    expect(nodeLabel(node("abcdef0", null))).toBe("abcdef0");
  });
});
