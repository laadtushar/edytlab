/**
 * Every command that changes the session goes through the session's line
 * (#498).
 *
 * A command sent outside it can overtake the edit before it, which is the
 * bug the line exists for, and nothing about the call looks wrong: it is
 * an ordinary `await`. `App.orderedEdits.test.tsx` drives the real app
 * through the kinds of edit that matter most; this is the cheap layer
 * under it, over every call site, for the same reason `headIsApplied`
 * is: a command added next month will not be driven by anyone's test yet.
 *
 * Two checks, both on source text:
 *
 * - In `App.tsx`, each call to one of these commands is inside a handler
 *   that sends it through the line: `inOrder(`, `applyEdit(`,
 *   `commitTrackChange(` or `commitSlider(` (which wraps the last) appears
 *   between the `useCallback` the handler starts with and the call.
 * - Nothing else calls them. Components are handed the line's version as
 *   a prop, and `useSession`, which owns the line, calls none of them: a
 *   hook method that queued one would deadlock when called from inside a
 *   turn.
 *
 * What it cannot see: a command called through another name, or a handler
 * that is not a `const x = useCallback(`.
 */

import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const src = join(process.cwd(), "src");
const read = (path: string) => readFileSync(join(src, path), "utf8");

/**
 * The bridge's commands that append a node, move the head, switch the
 * project or write to the store, as App.tsx names them. Reads, renders,
 * the agent's turn, and settings that do not touch the session are not
 * here, on purpose: see the PR for #498 for the list and the reasons.
 */
const SESSION_CHANGING = [
  "setTrackGain",
  "setTrackPan",
  "setTrackMuted",
  "setTrackSoloed",
  "setClipEnvelope",
  "moveClip",
  "removeClip",
  "renameTrack",
  "duplicateTrack",
  "removeTrack",
  "setSyncLock",
  "addMarker",
  "removeMarker",
  "updateMarker",
  "cutTranscriptWords",
  "setHeadTo",
  "batchLoad",
  "applyTemplate",
  "openProject",
  "saveProjectAs",
  "renameNode",
  "bridgeAcceptB",
];

/**
 * Calls that are not in a handler of their own with the line in it,
 * because the handler that calls them is itself a turn.
 */
const INSIDE_ANOTHER_TURN: Record<string, string[]> = {
  // `restoreView` runs inside `handleOpenRecent`'s turn.
  setHeadTo: ["await setHeadTo(view.head);"],
};

const IN_LINE = /\b(inOrder|applyEdit|commitTrackChange|commitSlider)\(/;

function callsTo(lines: string[], name: string): number[] {
  const call = new RegExp(`\\b${name}\\(`);
  return lines.flatMap((line, i) => {
    const text = line.trim();
    const comment = text.startsWith("//") || text.startsWith("*") || text.startsWith("/*");
    return call.test(line) && !comment ? [i] : [];
  });
}

/** From the `const x = useCallback(` the line is in, down to the line. */
function handlerSoFar(lines: string[], at: number): string {
  let start = at;
  while (start > 0 && !/^\s*const \w+ = useCallback\(/.test(lines[start])) start--;
  return lines.slice(start, at + 1).join("\n");
}

describe("App.tsx sends every session-changing command through the line", () => {
  const app = read("App.tsx");
  const lines = app.split("\n");
  if (lines.length < 100) throw new Error("App.tsx read as empty; this guard would be vacuous");

  it.each(SESSION_CHANGING)("%s", (name) => {
    const calls = callsTo(lines, name);
    expect(
      calls.length,
      `no call to ${name}() in App.tsx: renamed or removed, and this guard checks nothing`,
    ).toBeGreaterThan(0);

    const allowed = INSIDE_ANOTHER_TURN[name] ?? [];
    const outside = calls
      .filter((i) => !IN_LINE.test(handlerSoFar(lines, i)))
      .map((i) => lines[i].trim())
      .filter((text) => !allowed.includes(text));
    expect(
      outside,
      `${name}() is called outside the session's line. Send it through ` +
        `inOrder(), applyEdit() or commitTrackChange(), or two edits made ` +
        `in quick succession can reach the backend, and be answered, out of order.`,
    ).toEqual([]);
  });
});

describe("nothing else calls them", () => {
  /** Every non-test source file under `dir`, relative to `src`. */
  function sources(dir: string): string[] {
    return readdirSync(join(src, dir), { withFileTypes: true }).flatMap((entry) => {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) return entry.name === "__tests__" ? [] : sources(path);
      return /\.(ts|tsx)$/.test(entry.name) ? [path] : [];
    });
  }

  it.each([...sources("components"), ...sources("hooks")])("%s", (path) => {
    const lines = read(path).split("\n");
    const direct = SESSION_CHANGING.flatMap((name) =>
      callsTo(lines, name).map((i) => `${name}: ${lines[i].trim()}`),
    );
    expect(
      direct,
      `${path} calls a session-changing command itself. Take it as a prop and ` +
        `let App send it through the line, as GraphView and ABCompareBar do.`,
    ).toEqual([]);
  });
});
