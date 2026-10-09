import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import {
  isUndoChord,
  isRedoChord,
  isTextEntry,
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
 *
 * Undo and redo are also driven for real through the keys, in
 * `e2e/undo-path.spec.ts` (the path taken, #398) and
 * `e2e/labels.spec.ts`, against a `get_node` answer taken from the Rust
 * snapshot of a serialised `SessionNode`. Where they go is
 * `lib/headTrail.ts`, tested in `headTrail.test.ts`. This stays as the
 * cheap layer that names the predicates the tests above exercise.
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

describe("isTextEntry", () => {
  it("is text entry inside a textarea or a contenteditable element", () => {
    expect(isTextEntry({ tagName: "TEXTAREA" })).toBe(true);
    expect(isTextEntry({ tagName: "DIV", isContentEditable: true })).toBe(true);
  });

  it.each(["text", "search", "url", "tel", "email", "password", "number", "date"])(
    "is text entry in an <input type=%s>",
    (type) => {
      expect(isTextEntry({ tagName: "INPUT", type })).toBe(true);
    },
  );

  it("treats an input with no type, or one it does not know, as text", () => {
    expect(isTextEntry({ tagName: "INPUT" })).toBe(true);
    expect(isTextEntry({ tagName: "INPUT", type: "some-future-type" })).toBe(true);
  });

  it.each(["range", "checkbox", "radio", "button", "submit", "reset", "image", "color", "file"])(
    "is not text entry in an <input type=%s>, so session undo still runs",
    (type) => {
      expect(isTextEntry({ tagName: "INPUT", type })).toBe(false);
    },
  );

  it("matches the type case-insensitively", () => {
    expect(isTextEntry({ tagName: "INPUT", type: "RANGE" })).toBe(false);
  });

  it("is not text entry on a button, the body, or nothing", () => {
    expect(isTextEntry({ tagName: "BUTTON" })).toBe(false);
    expect(isTextEntry({ tagName: "BODY" })).toBe(false);
    expect(isTextEntry(null)).toBe(false);
    expect(isTextEntry(undefined)).toBe(false);
  });
});
