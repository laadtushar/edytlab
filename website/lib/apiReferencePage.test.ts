/**
 * The API reference page against the commands the app registers.
 *
 * `app/docs/api-reference/page.tsx` says it lists "every Tauri command the
 * frontend can call". It lost track of 24 of them (track gain, pan, mute and
 * solo, memory, sync-lock, clip moves, transcript cuts, timer recording and
 * more) until a manual sweep (#560) added a row for each of the 92. Nothing
 * checked it, so the next command would have started the drift again.
 *
 * This is the check. Like the other reference guards in this repo it is
 * crude on purpose: read both files as text and compare name sets, because a
 * parser clever enough to be elegant is one that can silently stop matching.
 * The source of truth is the `generate_handler![...]` list in
 * `apps/desktop/src-tauri/src/lib.rs`: a command that is not listed there
 * cannot be invoked, and one that is listed can. The page names each command
 * the way the TypeScript bridge does, in camelCase (`set_track_gain` is
 * `setTrackGain(...)`).
 *
 * `docs/api-reference.md` is not checked here. Its headings pair some
 * commands on one line and name the bridge's exports, and
 * `apps/desktop/src/__tests__/apiReferenceCoverage.test.ts` already checks
 * it, in both directions, against `tauri-bridge.ts`.
 *
 * Unlike the rest of `lib/*.test.ts` this reads files outside `website/`. CI
 * checks out the whole repository for the `website (test)` job, so the path
 * exists there; Vercel only builds the site and never runs the tests.
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

const REPO_ROOT = new URL("../../", import.meta.url);
const read = (path: string) => readFileSync(fileURLToPath(new URL(path, REPO_ROOT)), "utf8");

const LIB_RS = "apps/desktop/src-tauri/src/lib.rs";
const PAGE = "website/app/docs/api-reference/page.tsx";

const libRs = read(LIB_RS);
const page = read(PAGE);

/** `set_track_gain` -> `setTrackGain`, the name the bridge and the page use. */
function camelCase(command: string): string {
  return command.replace(/_([a-z0-9])/g, (_, c: string) => c.toUpperCase());
}

/**
 * Every command `generate_handler![...]` registers, as written in Rust.
 *
 * Comments are dropped first, and a path (`commands::open_project`) is
 * reduced to its last segment, which is the name the frontend invokes.
 */
function registeredCommands(): string[] {
  const start = libRs.indexOf("generate_handler![");
  if (start < 0) throw new Error(`${LIB_RS} no longer has generate_handler![...]: update this test`);
  const end = libRs.indexOf("]", start);
  const list = libRs
    .slice(start + "generate_handler![".length, end)
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/\/\/.*$/gm, "");
  return list
    .split(",")
    .map((entry) => entry.trim().split("::").pop() ?? "")
    .filter((name) => name !== "");
}

/**
 * Every command the page has a row for.
 *
 * The page keeps commands in `const commands = [...]` and the events the
 * backend emits (`onTextDelta(cb)` and friends) in `const events = [...]`
 * after it. Only the first is a list of commands, so only it is read. A row
 * is `name: "setTrackGain(track, db)"` or, when the arguments contain a
 * double quote, `name: 'readMemory("global" | "project")'`.
 */
function documentedCommands(): string[] {
  const start = page.indexOf("const commands = [");
  const end = page.indexOf("const events = [");
  if (start < 0 || end < start) {
    throw new Error(`${PAGE} no longer has "const commands" followed by "const events": update this test`);
  }
  return [...page.slice(start, end).matchAll(/^\s*name:\s*(["'])([A-Za-z0-9_]+)\(/gm)].map((m) => m[2]);
}

describe("the API reference page", () => {
  it("is being read by this test at all", () => {
    // A regex that stopped matching would leave the "no stale row" check
    // below with nothing to look at, and pass. There are 94 commands today;
    // a page with fewer than half of them has other problems, which the
    // next test names.
    expect(registeredCommands().length).toBeGreaterThan(80);
    expect(documentedCommands().length).toBeGreaterThan(registeredCommands().length / 2);
  });

  it("has a row for every command lib.rs registers", () => {
    const rows = new Set(documentedCommands());
    const missing = registeredCommands()
      .filter((command) => !rows.has(camelCase(command)))
      .map((command) => `${command}: add a row named ${camelCase(command)}(...)`);
    expect(
      missing,
      `${LIB_RS} registers these commands but ${PAGE} has no row for them. ` +
        `Add a row to the matching group of \`commands\`, named in camelCase.`,
    ).toEqual([]);
  });

  it("has no row for a command lib.rs no longer registers", () => {
    const commands = new Set(registeredCommands().map(camelCase));
    const stale = documentedCommands()
      .filter((row) => !commands.has(row))
      .map((row) => `${row}(...): not in generate_handler!`);
    expect(
      stale,
      `${PAGE} has rows for commands that ${LIB_RS} does not register, so they ` +
        `were renamed or removed. Rename or delete the row (events belong in \`events\`).`,
    ).toEqual([]);
  });
});
