import { describe, expect, it } from "vitest";
import { applyUndo, applyRedo } from "../lib/undoRedo";

describe("undo/redo logic", () => {
  it("undo pushes current head to redo stack and returns parent", () => {
    const result = applyUndo("node-b", "node-a", []);
    expect(result).toEqual({ head: "node-a", redoStack: ["node-b"] });
  });

  it("undo at root (no parent) returns null", () => {
    expect(applyUndo("node-a", null, [])).toBeNull();
  });

  it("redo pops from redo stack", () => {
    const result = applyRedo(["node-b"]);
    expect(result).toEqual({ head: "node-b", redoStack: [] });
  });

  it("redo on empty stack returns null", () => {
    expect(applyRedo([])).toBeNull();
  });

  it("undo then redo returns to original head", () => {
    const afterUndo = applyUndo("node-b", "node-a", [])!;
    const afterRedo = applyRedo(afterUndo.redoStack)!;
    expect(afterRedo.head).toBe("node-b");
  });
});
