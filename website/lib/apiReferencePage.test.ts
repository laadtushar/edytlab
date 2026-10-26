/**
 * The API reference against the commands the app registers.
 *
 * The reference says it lists "every Tauri command the frontend can call".
 * It lost track of 24 of them (track gain, pan, mute and solo, memory,
 * sync-lock, clip moves, transcript cuts, timer recording and more) until a
 * manual sweep (#560) added a row for each of the 92. Nothing checked it, so
 * the next command would have started the drift again.
 *
 * This is the check. The source of truth is the `generate_handler![...]`
 * list in `apps/desktop/src-tauri/src/lib.rs`: a command that is not listed
 * there cannot be invoked, and one that is listed can. That file is read as
 * text, crudely on purpose (a parser clever enough to be elegant is one that
 * can silently stop matching). The rows are the `apiAreas` data the pages
 * are rendered from (`lib/docs/api-data.ts`), imported rather than parsed,
 * so what is checked is exactly what is on the site, whichever area's page
 * a row is on. The rows name each command the way the TypeScript bridge
 * does, in camelCase (`set_track_gain` is `setTrackGain(...)`).
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

import { apiAreas } from "./docs/api-data";
import { bareName } from "./docs/reference";

const REPO_ROOT = new URL("../../", import.meta.url);
const read = (path: string) => readFileSync(fileURLToPath(new URL(path, REPO_ROOT)), "utf8");

const LIB_RS = "apps/desktop/src-tauri/src/lib.rs";
const DATA = "website/lib/docs/api-data.ts";

const libRs = read(LIB_RS);

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
 * Every command the reference has a row for, in whichever area it is. The
 * events the backend emits (`onTextDelta(cb)` and friends) are rows of a
 * different kind (`kind: "events"`) and are not commands.
 */
function documentedCommands(): string[] {
  return apiAreas.flatMap((area) =>
    area.sections.flatMap((section) => (section.kind === "commands" ? section.items.map((i) => bareName(i.name)) : [])),
  );
}

describe("the API reference", () => {
  it("is being read by this test at all", () => {
    // A reader that stopped matching would leave the "no stale row" check
    // below with nothing to look at, and pass. There are 94 commands today;
    // a reference with fewer than half of them has other problems, which the
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
      `${LIB_RS} registers these commands but ${DATA} has no row for them. ` +
        `Add a row to the matching area's commands, named in camelCase.`,
    ).toEqual([]);
  });

  it("has no row for a command lib.rs no longer registers", () => {
    const commands = new Set(registeredCommands().map(camelCase));
    const stale = documentedCommands()
      .filter((row) => !commands.has(row))
      .map((row) => `${row}(...): not in generate_handler!`);
    expect(
      stale,
      `${DATA} has rows for commands that ${LIB_RS} does not register, so they ` +
        `were renamed or removed. Rename or delete the row (events belong in \`events\`).`,
    ).toEqual([]);
  });

  it("has exactly one row for each command", () => {
    // The reference used to be one table per group on one page, where a
    // repeated row was easy to see. Now a command is on one of several
    // pages, and a second row for it on another would answer one question
    // twice (and could answer it differently).
    const rows = documentedCommands();
    const repeated = [...new Set(rows.filter((row, i) => rows.indexOf(row) !== i))];
    expect(repeated, `${DATA} documents these commands more than once`).toEqual([]);
  });
});
