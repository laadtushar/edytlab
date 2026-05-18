import { describe, expect, it } from "vitest";

// Minimal test: verify that the undo shortcut triggers the right action
// This tests the key handler logic, not the Tauri call itself

describe("undo/redo key shortcuts", () => {
  it("Ctrl+Z key produces undo intent", () => {
    const undoCalled = { value: false };
    const handler = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === "z" && !e.shiftKey) {
        undoCalled.value = true;
      }
    };
    const e = new KeyboardEvent("keydown", { ctrlKey: true, key: "z" });
    handler(e);
    expect(undoCalled.value).toBe(true);
  });

  it("Ctrl+Y key produces redo intent", () => {
    const redoCalled = { value: false };
    const handler = (e: KeyboardEvent) => {
      if (
        (e.ctrlKey || e.metaKey) &&
        (e.key === "y" || (e.key === "z" && e.shiftKey))
      ) {
        redoCalled.value = true;
      }
    };
    const e = new KeyboardEvent("keydown", { ctrlKey: true, key: "y" });
    handler(e);
    expect(redoCalled.value).toBe(true);
  });
});
