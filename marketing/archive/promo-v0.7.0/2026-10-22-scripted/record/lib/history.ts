/**
 * A session history with branches, for the Graph shots (S3, S8).
 *
 * The shape is what a real afternoon of editing leaves: a line of edits, and a
 * few tries that were abandoned (an echo that was too much, a distortion that
 * was not wanted). Node ids are hashes, tools are real tool names, and ages
 * are relative to the clock the page runs on, so the bubbles read "25m ago".
 */

import { Studio, hid } from "./studio";

export interface History {
  studio: Studio;
  /** The head, the newest node on the line. */
  head: string;
  /** The line of edits, oldest first. */
  chain: string[];
  /** Nodes on tries that were abandoned. */
  branches: string[];
}

/** [tool, minutes ago] for the line of edits, oldest first. */
const LINE: [string, number][] = [
  ["load", 25],
  ["load", 24],
  ["set_track_gain", 22],
  ["fade", 20],
  ["eq", 18],
  ["compressor", 16],
  ["add_effect", 15],
  ["set_pan", 14],
  ["normalize_loudness", 11],
  ["limiter", 10],
  ["set_track_gain", 7],
  ["fade", 3],
];

export function buildHistory(studio: Studio): History {
  const chain: string[] = [];
  const branches: string[] = [];
  const mk = (tool: string, parent: string | null, minAgo: number, label: string | null = null) =>
    studio.commit(tool, { parent, agoSec: minAgo * 60, label, seed: `graph:${tool}:${parent ?? "root"}:${minAgo}` });
  let parent: string | null = null;
  for (const [i, [tool, ago]] of LINE.entries()) {
    const id = mk(tool, parent, ago, i === 3 ? "intro fade" : null);
    chain.push(id);
    parent = id;
    // The tries: off the EQ, off the effect, off the first gain.
    if (i === 2) {
      const d1 = mk("time_stretch", id, 21.5);
      const d2 = mk("pitch_shift", d1, 21.2);
      branches.push(d1, d2);
    }
    if (i === 4) {
      const e1 = mk("reverb", id, 17.5);
      const e2 = mk("echo", e1, 17);
      branches.push(e1, e2);
    }
    if (i === 6) {
      branches.push(mk("distortion", id, 14.6));
    }
  }
  studio.head = chain[chain.length - 1];
  return { studio, head: studio.head, chain, branches };
}

/**
 * What `compact_session { keep_last, apply: true }` leaves (`plan_compaction`
 * in `crates/tools/src/reclaim.rs`): the head's last `keepLast` nodes on its
 * chain, the oldest of them made a root, and every node off the chain gone.
 * Returns the ids removed.
 */
export function compact(studio: Studio, chain: string[], keepLast: number): string[] {
  const keep = new Set(chain.slice(-keepLast));
  const removed = studio.nodes.filter((n) => !keep.has(n.id)).map((n) => n.id);
  studio.nodes = studio.nodes.filter((n) => keep.has(n.id));
  const root = studio.nodes.find((n) => n.id === chain[chain.length - keepLast]);
  if (root) root.parent = null;
  return removed;
}

export const ids = (seed: string) => hid(seed);
