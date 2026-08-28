/**
 * A command that returns a new head must have that head adopted (#232).
 *
 * `agent://node-created` is emitted from the agent path only
 * (commands.rs:1403). Every other session-mutating command hands its
 * new head back as a return value instead, and for markers, batch
 * loads, templates and recordings that value was simply dropped.
 *
 * The result is data loss rather than a stale label. `persistView`
 * writes the frontend head into `view.json`; `restoreView` feeds it to
 * `set_head_to`, which rewinds the store head *durably*. So labels
 * typed into the lane survive as nodes in the graph and disappear from
 * `list_markers` on the next open, and Ctrl+Z reverts the edit before
 * them instead of the label itself.
 *
 * ## Why this is a source check
 *
 * App.tsx mounts a Tauri surface a unit test cannot render — there is
 * no seam to drive `handleAddMarker` through and observe `head`. What
 * can be checked is the property that actually regressed: a call to a
 * head-returning command whose result goes nowhere. That is exactly
 * the shape of the bug, and it is what a future handler is most likely
 * to reintroduce.
 *
 * It cannot prove the head is applied *correctly*; it can prove the
 * return value is not thrown away.
 */

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const app = readFileSync(join(process.cwd(), "src", "App.tsx"), "utf8");

if (app.trim().length === 0) {
  throw new Error("App.tsx read as empty — this guard would be vacuous");
}

/**
 * Bridge commands that append a session node and return its id.
 *
 * Mixer commands (`setTrackGain`, `moveClip`, …) are deliberately
 * absent: they are passed as thunks to `commitTrackChange`, which
 * applies the head for them. `cutTranscriptWords` is absent because it
 * already calls `setHeadLocal` directly.
 */
const HEAD_RETURNING = [
  "addMarker",
  "removeMarker",
  "updateMarker",
  "applyTemplate",
  "batchLoad",
];

/**
 * Lines that call `name(`. The import list names these commands too,
 * but without a paren, so bare identifiers do not count as call sites.
 */
function callSites(name: string): string[] {
  const call = new RegExp(`\\b${name}\\(`);
  return app.split("\n").filter((line) => call.test(line));
}

describe("every head-returning command has its head adopted", () => {
  it.each(HEAD_RETURNING)("%s", (name) => {
    const sites = callSites(name);

    // A command that has stopped being called at all would make the
    // assertion below vacuously true.
    expect(
      sites.length,
      `no call to ${name}() found in App.tsx — either it was renamed ` +
        `or removed, and this guard is no longer checking anything`,
    ).toBeGreaterThan(0);

    const unadopted = sites.filter((line) => !line.includes("applyNewHead"));
    expect(
      unadopted,
      `${name}() returns the new session head, and these call sites ` +
        `discard it. A dropped head is written to view.json and replayed ` +
        `through set_head_to on the next open, which rewinds the store ` +
        `past every node created since — wrap the call in applyNewHead().`,
    ).toEqual([]);
  });
});

describe("applyNewHead does the two things a new head requires", () => {
  /**
   * Adopting a head without clearing the mix is the failure
   * `commitTrackChange` was written for: `render_preview` names its
   * output after the node id, so re-rendering a stale head returns the
   * same path string, React's useState bails out, and the load effect
   * never fires — no reload, and no error either.
   */
  it("sets the head and invalidates the rendered mix", () => {
    const body = app.match(
      /const applyNewHead = useCallback\(([\s\S]*?)\n {4}\[/,
    )?.[1];
    expect(body, "applyNewHead is not defined as a useCallback").toBeTruthy();
    expect(body).toContain("setHeadLocal(");
    expect(body).toContain("setMixPath(null)");
    expect(body).toContain("setMixNodeId(null)");
  });

  it("ignores a null or absent head rather than clearing state", () => {
    const body = app.match(
      /const applyNewHead = useCallback\(([\s\S]*?)\n {4}\[/,
    )?.[1];
    // `batch_load` returns `last_node_id: string | null` — a load that
    // appended nothing must leave the head where it is.
    expect(body).toMatch(/if\s*\(!newHead\)\s*return/);
  });
});
