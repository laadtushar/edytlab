/**
 * Dropping several files loads several files (#156).
 *
 * `listenToFileDrops` took `paths[0]` and discarded the rest. Dropping
 * five files loaded one and said nothing about the other four, which
 * reads as a broken drop rather than as a deliberate limit — and the
 * session model has held N tracks since Phase 1, so there was never a
 * reason for the limit.
 */

import { describe, expect, it, vi } from "vitest";

const { onDragDropEvent } = vi.hoisted(() => ({ onDragDropEvent: vi.fn() }));

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent }),
}));

vi.mock("./tauri-bridge", () => ({ sendMessage: vi.fn() }));

import { listenToFileDrops } from "../lib/file-open";

/** Hand the registered handler a drop payload. */
async function drop(payload: unknown) {
  const dropped: string[][] = [];
  onDragDropEvent.mockImplementation((cb: (e: unknown) => void) => {
    cb({ payload });
    return () => undefined;
  });
  await listenToFileDrops((paths) => dropped.push(paths));
  return dropped;
}

describe("listenToFileDrops", () => {
  it("passes every dropped path, not just the first", async () => {
    const dropped = await drop({
      type: "drop",
      paths: ["/a.wav", "/b.wav", "/c.wav"],
    });
    expect(dropped).toEqual([["/a.wav", "/b.wav", "/c.wav"]]);
  });

  it("still handles a single file", async () => {
    const dropped = await drop({ type: "drop", paths: ["/only.wav"] });
    expect(dropped).toEqual([["/only.wav"]]);
  });

  it("ignores hovers and cancels, and an empty drop", async () => {
    expect(await drop({ type: "over", paths: ["/a.wav"] })).toEqual([]);
    expect(await drop({ type: "drop", paths: [] })).toEqual([]);
    expect(await drop({ type: "drop" })).toEqual([]);
  });

  /**
   * Outside a Tauri runtime `getCurrentWebview` throws. The helper has
   * to survive that so the test environment — and a browser preview —
   * do not need to special-case it.
   */
  it("is a no-op when there is no webview", async () => {
    vi.resetModules();
    vi.doMock("@tauri-apps/api/webview", () => ({
      getCurrentWebview: () => {
        throw new Error("not a tauri runtime");
      },
    }));
    const { listenToFileDrops: isolated } = await import("../lib/file-open");
    const unlisten = await isolated(() => {
      throw new Error("must not be called");
    });
    expect(typeof unlisten).toBe("function");
    unlisten();
    vi.doUnmock("@tauri-apps/api/webview");
  });
});
