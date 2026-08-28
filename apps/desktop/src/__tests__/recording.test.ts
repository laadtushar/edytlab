/**
 * A failed recording has to reach the user (#248).
 *
 * `handleStartRecording` and `handleStopRecording` were the only two
 * handlers in `App.tsx` that caught to `console.error` instead of the
 * error banner — the app's single error surface. The `console.error`
 * sits inside a `try/catch`, so no global handler sees it either, and a
 * packaged desktop build has no console to read.
 *
 * So on a machine with no input device (or permission denied, or the
 * device busy) pressing Record did *nothing at all*: no state change,
 * no banner, nothing to distinguish it from a broken button.
 *
 * The stop path failed worse. It set `isRecording = false` before
 * returning, so the button flipped back to "Record" exactly as it does
 * after a good take — while the audio was gone.
 */

import { describe, expect, it, vi } from "vitest";

import { startTake, stopTake } from "../lib/recording";

describe("starting a take", () => {
  it("reports recording when the device opens", async () => {
    const outcome = await startTake(async () => "ok");
    expect(outcome).toEqual({ kind: "recording" });
  });

  it("returns a message instead of failing silently", async () => {
    const outcome = await startTake(async () => {
      throw new Error("no input device");
    });

    expect(outcome.kind).toBe("failed");
    // The device error has to survive into the message, or the banner
    // says "recording failed" and the user still cannot act.
    expect(outcome.kind === "failed" && outcome.message).toMatch(
      /no input device/,
    );
  });
});

describe("stopping a take", () => {
  it("reports the loaded node on success", async () => {
    const outcome = await stopTake(
      async () => ({ path: "/tmp/take.wav" }),
      async () => ({ last_node_id: "abc123" }),
    );

    expect(outcome).toEqual({
      kind: "loaded",
      path: "/tmp/take.wav",
      nodeId: "abc123",
    });
  });

  /**
   * The take is genuinely gone here, and the message says so. This is
   * the case that used to look identical to a successful stop.
   */
  it("says the take was lost when the write fails", async () => {
    const load = vi.fn();
    const outcome = await stopTake(async () => {
      throw new Error("disk full");
    }, load);

    expect(outcome.kind).toBe("saveFailed");
    expect(outcome.kind === "saveFailed" && outcome.message).toMatch(/lost/i);
    expect(outcome.kind === "saveFailed" && outcome.message).toMatch(
      /disk full/,
    );
    expect(load, "nothing to import when nothing was written").not.toHaveBeenCalled();
  });

  /**
   * The distinction worth having: the WAV exists, so telling the user
   * where it is beats telling them it was lost. Reporting both failures
   * identically would throw away a recoverable take.
   */
  it("names the file when it was saved but could not be imported", async () => {
    const outcome = await stopTake(
      async () => ({ path: "/tmp/take.wav" }),
      async () => {
        throw new Error("decode failed");
      },
    );

    expect(outcome.kind).toBe("loadFailed");
    expect(outcome.kind === "loadFailed" && outcome.path).toBe("/tmp/take.wav");
    expect(outcome.kind === "loadFailed" && outcome.message).toMatch(
      /\/tmp\/take\.wav/,
    );
    expect(
      outcome.kind === "loadFailed" && outcome.message,
      "the take is on disk, so it must not be described as lost",
    ).not.toMatch(/lost/i);
  });
});

describe("App delegates to these outcomes", () => {
  /**
   * The functions are only worth testing if the app runs them, and the
   * regression they guard is precisely a handler reporting to the
   * console instead. `App.tsx` mounts a Tauri surface a unit test
   * cannot render (#273), so this asserts the delegation.
   */
  it("uses startTake and stopTake, and no longer logs to the console", async () => {
    const { readFileSync } = await import("node:fs");
    const { join } = await import("node:path");
    const app = readFileSync(join(process.cwd(), "src", "App.tsx"), "utf8");

    expect(app.trim().length, "App.tsx read as empty").toBeGreaterThan(0);
    expect(/startTake\(/.test(app)).toBe(true);
    expect(/stopTake\(/.test(app)).toBe(true);
    expect(
      /console\.error\("start_recording/.test(app) ||
        /console\.error\("stop_recording/.test(app),
      "a recording failure is being reported to a console the user cannot see",
    ).toBe(false);
  });
});
