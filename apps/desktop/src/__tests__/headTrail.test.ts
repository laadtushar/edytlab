import { describe, expect, it } from "vitest";
import {
  headTrailReducer,
  initialHeadTrail,
  MAX_TRAIL,
  redoTarget,
  undoTarget,
  type HeadAction,
  type HeadTrail,
} from "../lib/headTrail";

const run = (actions: HeadAction[], from: HeadTrail = initialHeadTrail) =>
  actions.reduce(headTrailReducer, from);

/** What the app does for one Ctrl+Z, given the node's stored parent. */
function undo(s: HeadTrail, parentOf: Record<string, string | null> = {}): HeadTrail {
  const from = s.head!;
  const to = undoTarget(s.trail, from, null) ?? parentOf[from] ?? null;
  return to === null ? s : headTrailReducer(s, { type: "stepBack", from, to });
}

/** What the app does for one Ctrl+Y. */
function redo(s: HeadTrail): HeadTrail {
  const from = s.head!;
  const to = redoTarget(s.redo, from);
  return to === null ? s : headTrailReducer(s, { type: "stepForward", from, to });
}

describe("the path through history", () => {
  it("an edit pushes the head it leaves", () => {
    const s = run([
      { type: "fill", head: "A" },
      { type: "advance", head: "B" },
      { type: "advance", head: "C" },
    ]);
    expect(s).toEqual({ head: "C", trail: ["A", "B"], redo: [] });
  });

  it("an edit to the head it is already on is not a step", () => {
    const s = run([
      { type: "fill", head: "A" },
      { type: "advance", head: "A" },
    ]);
    expect(s).toEqual({ head: "A", trail: [], redo: [] });
  });

  it("the first head read at mount fills a gap and never overrides", () => {
    expect(run([{ type: "fill", head: "A" }])).toEqual({ head: "A", trail: [], redo: [] });
    const later = run([
      { type: "advance", head: "B" },
      { type: "fill", head: "A" },
    ]);
    expect(later.head).toBe("B");
  });

  it("a head read as null at mount fills nothing", () => {
    expect(run([{ type: "fill", head: null }])).toBe(initialHeadTrail);
  });

  it("the first edit of a project with no head has nothing to push", () => {
    expect(run([{ type: "advance", head: "A" }])).toEqual({ head: "A", trail: [], redo: [] });
  });

  it("opening another project forgets the old path and the redo list", () => {
    const s = run([
      { type: "fill", head: "A" },
      { type: "advance", head: "B" },
      { type: "advance", head: "C" },
      { type: "stepBack", from: "C", to: "B" },
      { type: "reset", head: "X" },
    ]);
    expect(s).toEqual({ head: "X", trail: [], redo: [] });
  });

  it("opening a project with nothing in it leaves no head, and no path", () => {
    const s = run([
      { type: "fill", head: "A" },
      { type: "advance", head: "B" },
      { type: "reset", head: null },
    ]);
    expect(s).toEqual({ head: null, trail: [], redo: [] });
    // The first edit there has nothing of the old project to push.
    expect(headTrailReducer(s, { type: "advance", head: "N1" })).toEqual({
      head: "N1",
      trail: [],
      redo: [],
    });
  });

  it("bounds a long history", () => {
    let s: HeadTrail = { head: "h0", trail: [], redo: [] };
    for (let i = 1; i <= MAX_TRAIL + 50; i++) {
      s = headTrailReducer(s, { type: "advance", head: `h${i}` });
    }
    expect(s.trail).toHaveLength(MAX_TRAIL);
    expect(s.trail[s.trail.length - 1]).toBe(`h${MAX_TRAIL + 49}`);
  });

  it("bounds the redo list the same way", () => {
    let s: HeadTrail = { head: "h0", trail: [], redo: [] };
    for (let i = 1; i <= MAX_TRAIL + 50; i++) {
      s = headTrailReducer(s, { type: "stepBack", from: `h${i}`, to: "h0" });
    }
    expect(s.redo).toHaveLength(MAX_TRAIL);
    expect(s.redo[s.redo.length - 1]).toBe(`h${MAX_TRAIL + 50}`);
  });
});

describe("the redo list", () => {
  it("undo puts the head it leaves on the redo list", () => {
    const s = run([
      { type: "fill", head: "A" },
      { type: "advance", head: "B" },
      { type: "stepBack", from: "B", to: "A" },
    ]);
    expect(s).toEqual({ head: "A", trail: [], redo: ["B"] });
  });

  it("redo takes it off again, and puts the head it leaves back on the trail", () => {
    const s = run([
      { type: "fill", head: "A" },
      { type: "advance", head: "B" },
      { type: "stepBack", from: "B", to: "A" },
      { type: "stepForward", from: "A", to: "B" },
    ]);
    expect(s).toEqual({ head: "B", trail: ["A"], redo: [] });
  });

  it("any new head clears it: a new edit starts a new branch", () => {
    const s = run([
      { type: "fill", head: "A" },
      { type: "advance", head: "B" },
      { type: "stepBack", from: "B", to: "A" },
      { type: "advance", head: "C" },
    ]);
    expect(s).toEqual({ head: "C", trail: ["A"], redo: [] });
    expect(redoTarget(s.redo, s.head!)).toBeNull();
  });

  it("an edit that lands on the head it is on keeps it", () => {
    const s = run([
      { type: "fill", head: "A" },
      { type: "advance", head: "B" },
      { type: "stepBack", from: "B", to: "A" },
      { type: "advance", head: "A" },
    ]);
    expect(s.redo).toEqual(["B"]);
  });

  it("redoTarget is the last place undo came from, and null when there is none", () => {
    expect(redoTarget(["B", "C"], "A")).toBe("C");
    expect(redoTarget([], "A")).toBeNull();
    // An entry naming the head is not a move.
    expect(redoTarget(["B", "A"], "A")).toBe("B");
    expect(redoTarget(["A"], "A")).toBeNull();
  });

  it("opening a project clears it", () => {
    const s = run([
      { type: "fill", head: "A" },
      { type: "advance", head: "B" },
      { type: "stepBack", from: "B", to: "A" },
      { type: "reset", head: "X" },
    ]);
    expect(s.redo).toEqual([]);
  });
});

describe("undo follows the path taken, not the node's first parent (#398)", () => {
  /**
   * The graph as the store holds it after: load (L0), load second file
   * (L1), mute (M), unmute. Unmuting reaches L1's state again, which is
   * the *same node*, whose stored parent is still L0.
   */
  const parentOf: Record<string, string | null> = { L0: null, L1: "L0", M: "L1" };

  it("mute, unmute, undo brings the mute back, then the unmuted state, then the load", () => {
    let s = run([
      { type: "fill", head: "L1" },
      { type: "advance", head: "M" }, // mute
      { type: "advance", head: "L1" }, // unmute: the same node as before
    ]);
    expect(s).toEqual({ head: "L1", trail: ["L1", "M"], redo: [] });

    // The old behaviour: the parent of the head is L0, so the mute and
    // the second file's load are both skipped.
    expect(parentOf[s.head!]).toBe("L0");

    const first = undoTarget(s.trail, s.head!, parentOf[s.head!]);
    expect(first).toBe("M");
    s = headTrailReducer(s, { type: "stepBack", from: "L1", to: first! });
    expect(s).toEqual({ head: "M", trail: ["L1"], redo: ["L1"] });

    const second = undoTarget(s.trail, s.head!, parentOf[s.head!]);
    expect(second).toBe("L1");
    s = headTrailReducer(s, { type: "stepBack", from: "M", to: second! });
    expect(s).toEqual({ head: "L1", trail: [], redo: ["L1", "M"] });

    // The trail is spent: now the stored parent is the right answer.
    expect(undoTarget(s.trail, s.head!, parentOf[s.head!])).toBe("L0");
  });

  it("three undos, then three redos, retrace the path exactly", () => {
    let s = run([
      { type: "fill", head: "L1" },
      { type: "advance", head: "M" },
      { type: "advance", head: "L1" },
    ]);
    const visited = [s.head];
    for (let i = 0; i < 3; i++) {
      s = undo(s, parentOf);
      visited.push(s.head);
    }
    // unmuted -> muted -> unmuted (before the mute) -> before the load
    expect(visited).toEqual(["L1", "M", "L1", "L0"]);

    const back = [s.head];
    for (let i = 0; i < 3; i++) {
      s = redo(s);
      back.push(s.head);
    }
    expect(back).toEqual(["L0", "L1", "M", "L1"]);
    expect(s.redo).toEqual([]);

    // And the way back out is still the path taken.
    expect(undo(s, parentOf).head).toBe("M");
  });

  it("duplicate, then remove the copy: undo brings the copy back", () => {
    // Remove returns to the state before the duplicate: the same node.
    const parents: Record<string, string | null> = { S: "P", D: "S" };
    let s = run([
      { type: "fill", head: "S" },
      { type: "advance", head: "D" }, // duplicate
      { type: "advance", head: "S" }, // remove the copy
    ]);
    expect(parents[s.head!]).toBe("P"); // the old answer: the copy cannot come back
    s = undo(s, parents);
    expect(s.head).toBe("D");
    s = redo(s);
    expect(s.head).toBe("S");
  });

  it("redo returns along the same path, and a new edit after undo starts a new one", () => {
    let s = run([
      { type: "fill", head: "A" },
      { type: "advance", head: "B" },
      { type: "advance", head: "C" },
    ]);
    s = headTrailReducer(s, { type: "stepBack", from: "C", to: "B" });
    expect(s).toEqual({ head: "B", trail: ["A"], redo: ["C"] });
    s = headTrailReducer(s, { type: "stepForward", from: "B", to: "C" });
    expect(s).toEqual({ head: "C", trail: ["A", "B"], redo: [] });

    s = headTrailReducer(s, { type: "stepBack", from: "C", to: "B" });
    s = headTrailReducer(s, { type: "advance", head: "D" });
    expect(s).toEqual({ head: "D", trail: ["A", "B"], redo: [] });
    expect(undoTarget(s.trail, s.head!, null)).toBe("B");
  });

  it("falls back to the parent when the trail is empty, and leaves it empty", () => {
    const s = run([{ type: "fill", head: "H" }]);
    expect(undoTarget(s.trail, "H", "P")).toBe("P");
    const after = headTrailReducer(s, { type: "stepBack", from: "H", to: "P" });
    expect(after.trail).toEqual([]);
    // And redo can come back from there.
    expect(redoTarget(after.redo, after.head!)).toBe("H");
  });

  it("is null at the root: nothing to undo", () => {
    expect(undoTarget([], "H", null)).toBeNull();
  });

  it("ignores a trail entry that names the current head", () => {
    expect(undoTarget(["A", "B", "B"], "B", "P")).toBe("A");
  });

  it("mirrors the backend when a move starts from a head the state did not know", () => {
    // `set_head_to` already succeeded: the frontend follows it, whatever
    // it believed the head was.
    const s = run([
      { type: "fill", head: "A" },
      { type: "advance", head: "B" },
      { type: "stepBack", from: "Z", to: "A" },
    ]);
    expect(s.head).toBe("A");
    expect(s.redo).toEqual(["Z"]);
    const fwd = headTrailReducer(s, { type: "stepForward", from: "Y", to: "B" });
    expect(fwd.head).toBe("B");
    expect(fwd.trail).toContain("Y");
  });
});
