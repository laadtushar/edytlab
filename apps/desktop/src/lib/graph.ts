/**
 * Pure helpers for the M25 graph view: lineage colouring, dagre
 * layout, and a tiny relative-time formatter.
 *
 * Kept separate from `GraphView.tsx` so the heavy logic stays
 * testable in isolation and so the React component file reads as
 * mostly view code.
 */

import dagre from "dagre";

import type { GraphNode } from "./tauri-bridge";

/**
 * HSL hue palette used for branch lineage colouring. Spaced roughly
 * evenly around the wheel; `s=70%, l=55%` is applied at render time so
 * we only carry the hue here.
 *
 * The palette length (10) is the soft cap on distinguishable branches
 * shown simultaneously; if a session genuinely has more branches the
 * colours wrap, which is acceptable for the 200-node budget.
 */
export const LINEAGE_HUES = [0, 30, 60, 90, 120, 180, 210, 240, 270, 300];

export const HUE_SAT = 70;
export const HUE_LIGHT = 55;

/**
 * Produce a stable `nodeId -> hex colour` map for `nodes`.
 *
 * Algorithm:
 *   1. Find the leaves (nodes that no other node names as parent).
 *   2. Sort leaves by id so the assignment is stable across calls.
 *   3. Assign each leaf a hue from `LINEAGE_HUES`, wrapping if needed.
 *   4. Walk from each leaf back to the root, painting every ancestor
 *      with that leaf's hue. Ancestors visited multiple times keep the
 *      first hue assigned — which biases shared trunks toward the
 *      lowest-id leaf, a deterministic but arbitrary choice.
 *
 * O(N) in node count.
 */
export function colourLineage(nodes: GraphNode[]): Map<string, string> {
  const byId = new Map<string, GraphNode>();
  const hasChild = new Set<string>();
  for (const n of nodes) {
    byId.set(n.id, n);
  }
  for (const n of nodes) {
    if (n.parent && byId.has(n.parent)) {
      hasChild.add(n.parent);
    }
  }

  const leaves = nodes
    .filter((n) => !hasChild.has(n.id))
    .map((n) => n.id)
    .sort();

  const colours = new Map<string, string>();
  leaves.forEach((leafId, idx) => {
    const hue = LINEAGE_HUES[idx % LINEAGE_HUES.length];
    const colour = `hsl(${hue}, ${HUE_SAT}%, ${HUE_LIGHT}%)`;
    let cursor: string | null = leafId;
    while (cursor && !colours.has(cursor)) {
      colours.set(cursor, colour);
      const node = byId.get(cursor);
      cursor = node?.parent ?? null;
    }
  });

  return colours;
}

export interface LayoutNode {
  id: string;
  x: number;
  y: number;
}

export interface LayoutResult {
  positions: Map<string, { x: number; y: number }>;
  width: number;
  height: number;
}

/**
 * Run dagre over `nodes` and return a `nodeId -> {x, y}` map plus the
 * overall bounding box. Direction is fixed to top→bottom so roots sit
 * at the top of the canvas and leaves at the bottom (M25 spec).
 *
 * `nodeWidth` / `nodeHeight` should match the rendered node size so
 * dagre's spacing accounts for the actual visual footprint.
 */
export function layoutDagre(
  nodes: GraphNode[],
  options: { nodeWidth?: number; nodeHeight?: number } = {},
): LayoutResult {
  const nodeWidth = options.nodeWidth ?? 180;
  const nodeHeight = options.nodeHeight ?? 64;

  const g = new dagre.graphlib.Graph();
  g.setGraph({ rankdir: "TB", nodesep: 40, ranksep: 60 });
  g.setDefaultEdgeLabel(() => ({}));

  for (const n of nodes) {
    g.setNode(n.id, { width: nodeWidth, height: nodeHeight });
  }
  const ids = new Set(nodes.map((n) => n.id));
  for (const n of nodes) {
    if (n.parent && ids.has(n.parent)) {
      g.setEdge(n.parent, n.id);
    }
  }

  dagre.layout(g);

  const positions = new Map<string, { x: number; y: number }>();
  let maxX = 0;
  let maxY = 0;
  for (const id of ids) {
    const n = g.node(id);
    if (!n) continue;
    // dagre returns center coordinates; convert to top-left so React
    // Flow's `position` (which is top-left) lines up.
    const x = n.x - nodeWidth / 2;
    const y = n.y - nodeHeight / 2;
    positions.set(id, { x, y });
    if (x + nodeWidth > maxX) maxX = x + nodeWidth;
    if (y + nodeHeight > maxY) maxY = y + nodeHeight;
  }
  return { positions, width: maxX, height: maxY };
}

/**
 * Format `iso` (an RFC 3339 timestamp) as a relative-to-`now` string
 * like `"2m ago"`, `"1h ago"`, `"just now"`. Deliberately tiny — we
 * don't want a date library for what's essentially a five-bucket
 * formatter.
 */
export function formatRelative(iso: string, now: Date = new Date()): string {
  const then = new Date(iso);
  const diffMs = now.getTime() - then.getTime();
  if (Number.isNaN(diffMs)) return "";
  const sec = Math.max(0, Math.floor(diffMs / 1000));
  if (sec < 5) return "just now";
  if (sec < 60) return `${sec}s ago`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h ago`;
  const day = Math.floor(hr / 24);
  return `${day}d ago`;
}

/**
 * Produce the short label shown in a graph node bubble. Falls back
 * through `label` -> first 3 words of `tool` -> short id prefix.
 */
export function nodeLabel(n: GraphNode): string {
  if (n.label && n.label.trim().length > 0) return n.label;
  if (n.tool && n.tool.trim().length > 0) {
    return n.tool.split(/\s+/).slice(0, 3).join(" ");
  }
  return n.id.slice(0, 7);
}
