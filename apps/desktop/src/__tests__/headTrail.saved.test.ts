/**
 * The path is kept with the project, so it survives a restart (#453).
 *
 * `lib/headTrail.ts` decides what a saved path means once it has been
 * read back: it is the path to the head it ends at, or it is nothing.
 */

import { describe, expect, it } from "vitest";

import {
  headTrailReducer,
  initialHeadTrail,
  MAX_TRAIL,
  readSavedTrail,
  redoTarget,
  undoTarget,
  type HeadAction,
  type HeadTrail,
  type SavedTrail,
} from "../lib/headTrail";

const run = (actions: HeadAction[], from: HeadTrail = initialHeadTrail) =>
  actions.reduce(headTrailReducer, from);

/** What the app does for one Ctrl+Z, given the node's stored parent. */
function undo(s: HeadTrail, parentOf: Record<string, string | null>): HeadTrail {
  const from = s.head!;
  const to = undoTarget(s.trail, from, null) ?? parentOf[from] ?? null;
  return to === null ? s : headTrailReducer(s, { type: "stepBack", from, to });
}

function redo(s: HeadTrail): HeadTrail {
  const from = s.head!;
  const to = redoTarget(s.redo, from);
  return to === null ? s : headTrailReducer(s, { type: "stepForward", from, to });
}

describe("the path saved with the project (#453)", () => {
  /**
   * Load (L0), load a second file (L1), mute (M), unmute. Unmuting is the
   * node L1 again, and L1's stored parent is L0: the mute is not in it.
   */
  const saved: SavedTrail = { head: "L1", trail: ["L1", "M"], redo: [] };
  const parentOf: Record<string, string | null> = { L0: null, L1: "L0", M: "L1" };

  it("a project opened on the head the path ends at takes the path", () => {
    const s = run([{ type: "reset", head: "L1", saved }]);
    expect(s).toEqual({ head: "L1", trail: ["L1", "M"], redo: [] });
    // Undo goes to the mute, not to the load the stored parent names.
    expect(undoTarget(s.trail, s.head!, parentOf[s.head!])).toBe("M");
  });

  it("a path that ends at another head is not the way to this one", () => {
    const s = run([{ type: "reset", head: "M", saved }]);
    expect(s).toEqual({ head: "M", trail: [], redo: [] });
    expect(undoTarget(s.trail, s.head!, parentOf[s.head!])).toBe("L1");
  });

  it("a project with nothing saved, or with no head, has no path", () => {
    expect(run([{ type: "reset", head: "L1", saved: null }])).toEqual({
      head: "L1",
      trail: [],
      redo: [],
    });
    expect(run([{ type: "reset", head: "L1" }]).trail).toEqual([]);
    expect(run([{ type: "reset", head: null, saved }])).toEqual({
      head: null,
      trail: [],
      redo: [],
    });
  });

  it("the old project's path never carries over", () => {
    const s = run([
      { type: "fill", head: "A" },
      { type: "advance", head: "B" },
      { type: "reset", head: "X", saved: null },
    ]);
    expect(s).toEqual({ head: "X", trail: [], redo: [] });
  });

  it("the redo list is restored with the trail", () => {
    const withRedo: SavedTrail = { head: "L1", trail: [], redo: ["L1", "M"] };
    const s = run([{ type: "reset", head: "L1", saved: withRedo }]);
    expect(redoTarget(s.redo, s.head!)).toBe("M");
  });

  it("a restore, after the head was read, gives the path to that head", () => {
    const s = run([
      { type: "fill", head: "L1" },
      { type: "restore", saved },
    ]);
    expect(s).toEqual({ head: "L1", trail: ["L1", "M"], redo: [] });
  });

  it("a restore never replaces steps the user has already taken", () => {
    // An edit landed while the saved path was being read: that is newer.
    const edited = run([
      { type: "fill", head: "L1" },
      { type: "advance", head: "N" },
      { type: "advance", head: "L1" },
    ]);
    expect(run([{ type: "restore", saved }], edited)).toBe(edited);

    // An undo is a step too, even though it leaves the trail empty.
    const undone = run([
      { type: "fill", head: "L1" },
      { type: "stepBack", from: "L1", to: "L0" },
    ]);
    expect(run([{ type: "restore", saved: { head: "L0", trail: ["Z"], redo: [] } }], undone)).toBe(
      undone,
    );
  });

  it("a restore for another head, or with no head to apply to, changes nothing", () => {
    const at = run([{ type: "fill", head: "M" }]);
    expect(run([{ type: "restore", saved }], at)).toBe(at);
    expect(run([{ type: "restore", saved }])).toBe(initialHeadTrail);
  });

  it("a saved path longer than the bound is cut to the most recent steps", () => {
    const long = Array.from({ length: MAX_TRAIL + 25 }, (_, i) => `n${i}`);
    const s = run([{ type: "reset", head: "H", saved: { head: "H", trail: long, redo: long } }]);
    expect(s.trail).toHaveLength(MAX_TRAIL);
    expect(s.trail[s.trail.length - 1]).toBe(long[long.length - 1]);
    expect(s.trail[0]).toBe("n25");
    expect(s.redo).toHaveLength(MAX_TRAIL);
  });

  it("undo, redo and undo again retrace a restored path", () => {
    let s = run([{ type: "reset", head: "L1", saved }]);
    s = undo(s, parentOf); // to the mute
    expect(s).toEqual({ head: "M", trail: ["L1"], redo: ["L1"] });
    s = undo(s, parentOf); // before the mute
    expect(s.head).toBe("L1");
    s = undo(s, parentOf); // the trail is spent: the stored parent
    expect(s.head).toBe("L0");
    s = redo(s);
    expect(s.head).toBe("L1");
  });
});

describe("readSavedTrail", () => {
  it("reads what get_head_trail answers", () => {
    expect(readSavedTrail({ head: "H", trail: ["A", "B"], redo: ["C"] })).toEqual({
      head: "H",
      trail: ["A", "B"],
      redo: ["C"],
    });
  });

  it("reads a project with nothing saved as no path", () => {
    expect(readSavedTrail({ head: null, trail: [], redo: [] })).toBeNull();
  });

  it("reads anything that is not the shape as no path, and does not throw", () => {
    for (const raw of [undefined, null, 0, "H", [], true, { head: 7 }, { head: "" }]) {
      expect(readSavedTrail(raw)).toBeNull();
    }
  });

  it("keeps the ids it can use and drops the rest", () => {
    expect(readSavedTrail({ head: "H", trail: ["A", 1, null, "", "B"], redo: "nope" })).toEqual({
      head: "H",
      trail: ["A", "B"],
      redo: [],
    });
    expect(readSavedTrail({ head: "H" })).toEqual({ head: "H", trail: [], redo: [] });
  });

  it("cuts a list longer than the bound to its most recent steps", () => {
    const long = Array.from({ length: MAX_TRAIL + 3 }, (_, i) => `n${i}`);
    const read = readSavedTrail({ head: "H", trail: long, redo: [] });
    expect(read?.trail).toHaveLength(MAX_TRAIL);
    expect(read?.trail[0]).toBe("n3");
  });
});
