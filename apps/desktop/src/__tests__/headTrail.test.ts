import { describe, expect, it } from "vitest";
import {
  headTrailReducer,
  initialHeadTrail,
  MAX_TRAIL,
  undoTarget,
  type HeadAction,
  type HeadTrail,
} from "../lib/headTrail";

const run = (actions: HeadAction[], from: HeadTrail = initialHeadTrail) =>
  actions.reduce(headTrailReducer, from);

describe("the path through history", () => {
  it("an edit pushes the head it leaves", () => {
    const s = run([
      { type: "fill", head: "A" },
      { type: "advance", head: "B" },
      { type: "advance", head: "C" },
    ]);
    expect(s).toEqual({ head: "C", trail: ["A", "B"] });
  });

  it("an edit to the head it is already on is not a step", () => {
    const s = run([
      { type: "fill", head: "A" },
      { type: "advance", head: "A" },
    ]);
    expect(s).toEqual({ head: "A", trail: [] });
  });

  it("the first head read at mount fills a gap and never overrides", () => {
    expect(run([{ type: "fill", head: "A" }])).toEqual({ head: "A", trail: [] });
    const later = run([
      { type: "advance", head: "B" },
      { type: "fill", head: "A" },
    ]);
    expect(later.head).toBe("B");
  });

  it("opening another project forgets the old path", () => {
    const s = run([
      { type: "fill", head: "A" },
      { type: "advance", head: "B" },
      { type: "reset", head: "X" },
    ]);
    expect(s).toEqual({ head: "X", trail: [] });
  });

  it("bounds a long history", () => {
    let s: HeadTrail = { head: "h0", trail: [] };
    for (let i = 1; i <= MAX_TRAIL + 50; i++) {
      s = headTrailReducer(s, { type: "advance", head: `h${i}` });
    }
    expect(s.trail).toHaveLength(MAX_TRAIL);
    expect(s.trail[s.trail.length - 1]).toBe(`h${MAX_TRAIL + 49}`);
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
    expect(s).toEqual({ head: "L1", trail: ["L1", "M"] });

    // The old behaviour: the parent of the head is L0, so the mute and
    // the second file's load are both skipped.
    expect(parentOf[s.head!]).toBe("L0");

    const first = undoTarget(s.trail, s.head!, parentOf[s.head!]);
    expect(first).toBe("M");
    s = headTrailReducer(s, { type: "stepBack", head: first! });
    expect(s).toEqual({ head: "M", trail: ["L1"] });

    const second = undoTarget(s.trail, s.head!, parentOf[s.head!]);
    expect(second).toBe("L1");
    s = headTrailReducer(s, { type: "stepBack", head: second! });
    expect(s).toEqual({ head: "L1", trail: [] });

    // The trail is spent: now the stored parent is the right answer.
    expect(undoTarget(s.trail, s.head!, parentOf[s.head!])).toBe("L0");
  });

  it("redo returns along the same path, and a new edit after undo starts a new one", () => {
    let s = run([
      { type: "fill", head: "A" },
      { type: "advance", head: "B" },
      { type: "advance", head: "C" },
    ]);
    s = headTrailReducer(s, { type: "stepBack", head: "B" });
    expect(s).toEqual({ head: "B", trail: ["A"] });
    s = headTrailReducer(s, { type: "stepForward", head: "C" });
    expect(s).toEqual({ head: "C", trail: ["A", "B"] });

    s = headTrailReducer(s, { type: "stepBack", head: "B" });
    s = headTrailReducer(s, { type: "advance", head: "D" });
    expect(s).toEqual({ head: "D", trail: ["A", "B"] });
    expect(undoTarget(s.trail, s.head!, null)).toBe("B");
  });

  it("falls back to the parent when the trail is empty, and leaves it empty", () => {
    const s = run([{ type: "fill", head: "H" }]);
    expect(undoTarget(s.trail, "H", "P")).toBe("P");
    expect(headTrailReducer(s, { type: "stepBack", head: "P" }).trail).toEqual([]);
  });

  it("is null at the root: nothing to undo", () => {
    expect(undoTarget([], "H", null)).toBeNull();
  });

  it("ignores a trail entry that names the current head", () => {
    expect(undoTarget(["A", "B", "B"], "B", "P")).toBe("A");
  });
});
