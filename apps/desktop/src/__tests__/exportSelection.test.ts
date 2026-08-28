/**
 * Export Selection, tested against the path it actually takes (#262).
 *
 * This file used to declare its own `validate(start, end)` and
 * `isFullExport(start?, end?)` inside the test body and assert against
 * those. Neither exists in the app: nothing validates the range, and
 * there is no "full export" branch on this path at all — Export
 * Selection always sends a range. The suite reported coverage for a
 * feature it had invented.
 *
 * The real path is `App.tsx` → `renderRange` (`tauri-bridge.ts`) →
 * `render_range` (`commands.rs`). The seam worth pinning in TypeScript
 * is the bridge call: Tauri matches the object's **keys** to the Rust
 * command's parameter names, so a rename or a reorder there fails at
 * runtime with a bare "invalid args" and no type error anywhere.
 *
 * `App.tsx` mounts a Tauri surface a unit test cannot render (#273), so
 * the handler's own guard is asserted from the source.
 */

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  invoke,
}));

import { renderRange } from "../lib/tauri-bridge";

beforeEach(() => {
  invoke.mockReset();
  invoke.mockResolvedValue({ path: "/tmp/out.wav", frames_written: 1000 });
});

describe("renderRange", () => {
  /**
   * The argument names are the contract. `render_range` takes
   * `node_id`, `start_sec`, `end_sec`, `out_path`, and Tauri accepts
   * them camelCased — so these four keys are load-bearing, and nothing
   * else in the codebase would notice one changing.
   */
  it("sends the node, the range and the destination under the names the command expects", async () => {
    await renderRange("node-abc", 1.5, 4.25, "/tmp/out.wav");

    expect(invoke).toHaveBeenCalledTimes(1);
    const [command, args] = invoke.mock.calls[0];
    expect(command).toBe("render_range");
    expect(args).toEqual({
      nodeId: "node-abc",
      startSec: 1.5,
      endSec: 4.25,
      outPath: "/tmp/out.wav",
    });
  });

  /**
   * Seconds go across the bridge unrounded; `render_range` multiplies
   * by the session's sample rate on the Rust side, precisely so the
   * frontend never has to know it. Rounding here would shift the export
   * by up to half a sample-rate's worth of frames.
   */
  it("passes seconds through without rounding them", async () => {
    await renderRange("n", 0.0166667, 12.3456789, "/tmp/o.wav");

    const [, args] = invoke.mock.calls[0];
    expect(args.startSec).toBe(0.0166667);
    expect(args.endSec).toBe(12.3456789);
  });

  it("propagates a failed render rather than resolving", async () => {
    invoke.mockRejectedValueOnce(new Error("no such node"));
    await expect(renderRange("n", 0, 1, "/tmp/o.wav")).rejects.toThrow(
      /no such node/,
    );
  });
});

describe("the Export Selection handler", () => {
  const app = readFileSync(join(process.cwd(), "src", "App.tsx"), "utf8");
  const handler = app.slice(app.indexOf("const handleExportSelection"));

  it("was found in App.tsx", () => {
    expect(app.trim().length, "App.tsx read as empty").toBeGreaterThan(0);
    expect(handler.startsWith("const handleExportSelection")).toBe(true);
  });

  /**
   * Without a head there is nothing to render, and without a selection
   * there is no range — `renderRange(head, undefined, undefined, …)`
   * would reach Rust as `null` and fail deserialization. The guard is
   * what keeps a click on an empty session from doing that.
   */
  it("does nothing without a session head or a selection", () => {
    const body = handler.slice(0, handler.indexOf("setExporting(true)"));
    expect(body).toMatch(/!head/);
    expect(body).toMatch(/!selection/);
  });

  it("sends the selection's own bounds to renderRange", () => {
    expect(handler).toMatch(
      /renderRange\(\s*head,\s*selection\.start,\s*selection\.end,\s*outPath\s*\)/,
    );
  });

  /**
   * The dialog can be dismissed, and `save()` then resolves with null.
   * Rendering to `null` would be a hard failure, so the handler has to
   * return — and it has to clear `exporting` first, or the button stays
   * disabled for the rest of the session.
   */
  it("bails out when the save dialog is cancelled, without leaving the button disabled", () => {
    expect(handler).toMatch(/if\s*\(!outPath\)\s*\{\s*setExporting\(false\);\s*return;/);
  });

  /** A failed render has to reach the error banner, not a console. */
  it("reports a failure to the error banner", () => {
    const body = handler.slice(0, handler.indexOf("}, [head"));
    expect(body).toMatch(/catch\s*\(e\)\s*\{\s*setRenderError\(/);
    expect(body).not.toMatch(/console\.(error|warn)/);
  });
});
