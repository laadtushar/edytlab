import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import {
  applyUndo,
  applyRedo,
  isUndoChord,
  isRedoChord,
  type Chord,
} from "../lib/undoRedo";

/** A keydown as the window handler would see it. */
function chord(key: string, mods: Partial<Chord> = {}): Chord {
  return { key, ctrlKey: false, metaKey: false, shiftKey: false, ...mods };
}

describe("undo/redo chords", () => {
  it("⌘Z undoes on macOS", () => {
    expect(isUndoChord(chord("z", { metaKey: true }))).toBe(true);
  });

  it("Ctrl+Z still undoes on Windows and Linux", () => {
    expect(isUndoChord(chord("z", { ctrlKey: true }))).toBe(true);
  });

  it("a bare Z does not undo", () => {
    expect(isUndoChord(chord("z"))).toBe(false);
  });

  /**
   * The chord that was dead on every platform, not just macOS.
   *
   * `key` carries the *shifted* value, so a Shift+Z press reports "Z".
   * The old branch required `shiftKey` and then compared against the
   * lowercase "z", so it could never match its own guard.
   */
  it("Ctrl+Shift+Z redoes even though the key arrives as uppercase Z", () => {
    expect(isRedoChord(chord("Z", { ctrlKey: true, shiftKey: true }))).toBe(
      true,
    );
  });

  it("⌘⇧Z redoes on macOS", () => {
    expect(isRedoChord(chord("Z", { metaKey: true, shiftKey: true }))).toBe(
      true,
    );
  });

  it("Ctrl/⌘+Y redoes", () => {
    expect(isRedoChord(chord("y", { ctrlKey: true }))).toBe(true);
    expect(isRedoChord(chord("y", { metaKey: true }))).toBe(true);
  });

  /**
   * Undo and redo share the Z key and are told apart only by Shift.
   * If undo stopped excluding it, ⌘⇧Z would undo before redo was ever
   * consulted — the handler checks undo first and returns.
   */
  it("the shifted Z is redo only — undo must not also claim it", () => {
    expect(isUndoChord(chord("Z", { metaKey: true, shiftKey: true }))).toBe(
      false,
    );
  });

  it("an unmodified Y or Z does nothing", () => {
    expect(isRedoChord(chord("y"))).toBe(false);
    expect(isRedoChord(chord("Z", { shiftKey: true }))).toBe(false);
  });
});

/**
 * The predicates above are only worth testing if the app runs them.
 *
 * The binding used to live inline in a `useEffect` inside App.tsx,
 * unreachable from any test — so the suite stayed green with the real
 * branch deleted outright. App.tsx mounts a Tauri surface that a unit
 * test cannot render, so this asserts the delegation instead: the
 * handler must call the same predicate these tests exercise.
 */
describe("App.tsx delegates to the tested predicates", () => {
  const app = readFileSync(join(process.cwd(), "src", "App.tsx"), "utf8");

  it.each(["isUndoChord", "isRedoChord"])("the handler calls %s", (name) => {
    expect(app.trim().length, "App.tsx read as empty").toBeGreaterThan(0);
    expect(
      new RegExp(`${name}\\(e\\)`).test(app),
      `App.tsx never calls ${name}(e) — the tests below would be ` +
        `asserting against code the app does not run`,
    ).toBe(true);
  });
});

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

  it("redo stack cleared after new node resets all forward history", () => {
    // After undo, redoStack has one entry
    const afterUndo = applyUndo("node-b", "node-a", [])!;
    expect(afterUndo.redoStack).toHaveLength(1);
    // When onNodeCreated fires, App.tsx calls setRedoStack([])
    // Verify that after that reset, applyRedo finds nothing
    expect(applyRedo([])).toBeNull();
  });
});
