/**
 * A command that returns a new head must have that head adopted (#232).
 *
 * `agent://node-created` is emitted from the agent path only
 * (commands.rs:1403). Every other session-mutating command hands its
 * new head back as a return value instead, and for markers, batch
 * loads, templates and recordings that value was simply dropped.
 *
 * The result is data loss rather than a stale label. `persistView`
 * writes the frontend head into `view.json`; opening a project used to
 * feed it to `set_head_to` (it no longer does, #507), which rewinds the
 * store head *durably*. So labels typed into the lane survived as nodes
 * in the graph and disappeared from `list_markers` on the next open, and
 * Ctrl+Z reverts the edit before them instead of the label itself.
 *
 * ## Why this is still a source check
 *
 * The behaviour is tested for real in `e2e/labels.spec.ts` (#273): the
 * whole app in a browser, a label added through the lane, the head it
 * returns used by the next render and saved into `view.json`, and the
 * label still there when the project is opened again.
 *
 * This stays as the cheap layer under it. It covers every
 * head-returning command at once, including the ones no e2e test
 * drives yet, and it catches the shape the bug actually had: a call
 * whose result goes nowhere. It cannot prove the head is applied
 * *correctly*; it can prove the return value is not thrown away.
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
 *
 * A call passed as a thunk to `applyEdit` has its head adopted too:
 * `applyEdit` sends the command in the session's line and applies what it
 * returns inside the same turn (#498), which the last test below holds it to.
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

/**
 * Whether the head a call returns is adopted: either on the call's own
 * line, or — when the result is bound first because the caller also
 * needs the rest of it (`batchLoad` reports failures too) — by passing
 * a field of that variable to `applyNewHead(`. A bound result nobody
 * adopts still fails.
 */
function adopted(name: string, line: string): boolean {
  if (line.includes("applyNewHead") || line.includes("applyEdit(")) return true;
  const bound = line.match(new RegExp(`\\b(?:const|let)\\s+(\\w+)\\s*=\\s*await\\s+${name}\\(`));
  return bound !== null && new RegExp(`applyNewHead\\(\\s*${bound[1]}\\.`).test(app);
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

    const unadopted = sites.filter((line) => !adopted(name, line));
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

describe("applyEdit adopts a head in the session's line", () => {
  /**
   * `applyEdit` is what stands in for `applyNewHead(await command())` at
   * the call sites that pass a thunk. It has to adopt the head *inside*
   * the session's line: adopted after the turn, the next edit could be sent
   * and answered before the head from this one was applied, and the head
   * would go back (#498).
   */
  it("applies what the command returns inside the turn", () => {
    const body = app.match(/const applyEdit = useCallback\(([\s\S]*?)\n {2}\);/)?.[1];
    expect(body, "applyEdit is not defined as a useCallback").toBeTruthy();
    expect(body).toMatch(
      /inOrder\(\s*async \(\) => \{\s*applyNewHead\(await command\(\)\);\s*\}\)/,
    );
  });
});

describe("a head is adopted inside a turn, and nowhere else", () => {
  /**
   * A head adopted after its turn has ended is adopted out of line: the
   * next edit may already be on its way, and the adoption can put the head
   * back to a node that edit has left. It holds today only because an IPC
   * answer is a macrotask, which is not something to build on. So each
   * place that moves the frontend's head either sits in a handler that is
   * a turn (`inOrder(`, `applyEdit(`), or is one of the two that are not an
   * edit:
   *
   * - `applyNewHead`, which is what the turns call.
   * - `handleSelectGraphNode`, a click on a node in the graph, which
   *   previews it. The head it names was not moved on the backend.
   *
   * What it replaced: Record and Timer record adopted the head their take
   * returned a second time, after `loadInOrder` had adopted it in its
   * turn, and Accept B adopted its head in a callback that ran after the
   * command's turn.
   */
  const NOT_AN_EDIT = ["applyNewHead", "handleSelectGraphNode"];
  const IN_A_TURN = /\b(inOrder|applyEdit)\(/;
  const lines = app.split("\n");

  /** The `const x = useCallback(` a line is in, down to the line. */
  function handlerSoFar(at: number): { name: string; text: string } {
    let start = at;
    while (start > 0 && !/^\s*const \w+ = useCallback\(/.test(lines[start])) start--;
    const name = /^\s*const (\w+) = useCallback\(/.exec(lines[start])?.[1] ?? "(no handler)";
    return { name, text: lines.slice(start, at + 1).join("\n") };
  }

  const moves = lines.flatMap((line, i) => {
    const text = line.trim();
    const comment = text.startsWith("//") || text.startsWith("*") || text.startsWith("/*");
    return !comment && /\b(setHeadLocal|applyNewHead)\(/.test(line) ? [i] : [];
  });

  it("finds the places that move the head, so the check below is not vacuous", () => {
    expect(moves.length).toBeGreaterThan(5);
  });

  it("every one is in a turn", () => {
    const outside = moves
      .map((i) => ({ at: i, ...handlerSoFar(i) }))
      .filter(({ name, text }) => !NOT_AN_EDIT.includes(name) && !IN_A_TURN.test(text))
      .map(({ name, at }) => `${name}: ${lines[at].trim()}`);
    expect(
      outside,
      `these adopt a head outside the session's line. Move the adoption into ` +
        `the turn (inOrder() or applyEdit()), or drop it when the turn ` +
        `already adopts the head, as loadInOrder() does.`,
    ).toEqual([]);
  });
});
