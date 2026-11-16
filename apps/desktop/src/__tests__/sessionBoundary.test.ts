/**
 * The window changes the session through `useSession` and nothing else
 * (ADR 0001, #713).
 *
 * This is the one import test the ADR asks for (the app has no ESLint). It
 * replaces two scans of `App.tsx`'s source text, `headIsApplied` ("a command
 * that returns a head has it adopted", #232) and `sessionQueueCoverage`
 * ("every session-changing command goes through the line", #498), and the
 * rule "every head move goes through `setHeadLocal` / `resetHead`". Each is
 * now a fact about what can reach what, so a handler that forgets one of
 * them does not compile, or cannot be written:
 *
 * - The commands live in `lib/sessionCommands.ts`, and only `useSession`
 *   imports it. A component has nothing to call but `useSession`'s `edit`
 *   and `move`, which put every command in the line (one `scope` of
 *   mutations), adopt the head each entry says it returns (the type has no
 *   default) and read again what each entry says it changes.
 * - `useSessionHead` owns the head and the path (`setHeadLocal`,
 *   `resetHead`, `stepBack`, …), and only `useSession` imports it. The head
 *   moves from `edit`, `move` and `agent://node-created` alone.
 * - `tauri-bridge.ts` no longer exports the commands, and no other file
 *   names their IPC commands, so the module cannot be gone round with a
 *   bridge import or a raw `invoke`.
 *
 * What it checks is import specifiers and string literals, which is what
 * the rule is about; the commands' behaviour is `useSession.session.test.tsx`.
 * `findViolations` is itself tested on files that break each rule, so the
 * test cannot pass for lack of looking.
 */

import { readdirSync, readFileSync } from "node:fs";
import { join, posix } from "node:path";
import { describe, expect, it } from "vitest";

import * as bridge from "../lib/tauri-bridge";

const src = join(process.cwd(), "src");

/** Every module that may be imported by only one other, and by whom. */
const PRIVATE: Record<string, readonly string[]> = {
  "lib/sessionCommands": ["hooks/useSession.ts"],
  "hooks/useSessionHead": ["hooks/useSession.ts"],
};

/** The module that names the session's commands. */
const COMMANDS_MODULE = "lib/sessionCommands.ts";

/** Source files, by path relative to `src`. */
type Sources = Record<string, string>;

/** Non-test sources, by path relative to `src`. */
function readSources(dir = ""): Sources {
  const out: Sources = {};
  for (const entry of readdirSync(join(src, dir), { withFileTypes: true })) {
    const path = posix.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === "__tests__" || entry.name === "node_modules") continue;
      Object.assign(out, readSources(path));
    } else if (/\.(ts|tsx)$/.test(entry.name) && !/\.(test|spec)\./.test(entry.name)) {
      out[path] = readFileSync(join(src, path), "utf8");
    }
  }
  return out;
}

/** The module specifiers a file imports, re-exports or loads. */
function specifiers(text: string): string[] {
  const found: string[] = [];
  const patterns = [
    /\bfrom\s+["']([^"']+)["']/g,
    /\bimport\s*\(\s*["']([^"']+)["']\s*\)/g,
    /\brequire\s*\(\s*["']([^"']+)["']\s*\)/g,
    /\bimport\s+["']([^"']+)["']/g,
  ];
  for (const pattern of patterns) {
    for (const match of text.matchAll(pattern)) found.push(match[1]);
  }
  return found;
}

/** `specifier` as written in `from`, resolved to a path relative to `src` with no extension. */
function resolve(from: string, specifier: string): string | null {
  if (!specifier.startsWith(".")) return null;
  return posix.normalize(posix.join(posix.dirname(from), specifier)).replace(/\.(ts|tsx)$/, "");
}

/** `text` without its comments: a name in a doc comment is not a call. */
function withoutComments(text: string): string {
  return text.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[^:"'`\\])\/\/.*$/gm, "$1");
}

/** The IPC command names a commands module sends: `invoke("name"` and `invoke<T>("name"`. */
function ipcCommands(text: string): string[] {
  return [...text.matchAll(/\binvoke\s*(?:<[^>]*>)?\s*\(\s*["']([a-z_]+)["']/g)].map((m) => m[1]);
}

/** Everything in `files` that breaks the boundary, as `file: what`. */
function findViolations(files: Sources, commands: readonly string[]): string[] {
  const violations: string[] = [];
  for (const [file, text] of Object.entries(files)) {
    for (const specifier of specifiers(text)) {
      const target = resolve(file, specifier);
      if (target === null) continue;
      const allowed = PRIVATE[target];
      if (allowed && !allowed.includes(file)) {
        violations.push(`${file}: imports ${target}, which only ${allowed.join(", ")} may`);
      }
    }
    if (file !== COMMANDS_MODULE) {
      const code = withoutComments(text);
      for (const command of commands) {
        if (new RegExp(`["'\`]${command}["'\`]`).test(code)) {
          violations.push(`${file}: names the session command "${command}" outside ${COMMANDS_MODULE}`);
        }
      }
    }
  }
  return violations;
}

describe("only useSession reaches the session's commands and head", () => {
  const files = readSources();
  const commands = [...new Set(ipcCommands(files[COMMANDS_MODULE] ?? ""))];

  it("reads the sources it is meant to check", () => {
    // A guard that read nothing would pass for lack of looking.
    expect(Object.keys(files).length).toBeGreaterThan(50);
    expect(files["hooks/useSession.ts"]).toBeTruthy();
    expect(commands.length).toBeGreaterThanOrEqual(22);
    expect(commands).toContain("set_head_to");
    expect(commands).toContain("batch_load");
  });

  it("no source imports the commands or the head hook but useSession, or names a command", () => {
    expect(findViolations(files, commands)).toEqual([]);
  });

  it("useSession does import both, so the table is not dead", () => {
    const imported = specifiers(files["hooks/useSession.ts"]).map((s) =>
      resolve("hooks/useSession.ts", s),
    );
    expect(imported).toContain("lib/sessionCommands");
    expect(imported).toContain("hooks/useSessionHead");
  });

  it("the bridge exports none of the session-changing commands", () => {
    const camel = (command: string) => command.replace(/_([a-z])/g, (_, c: string) => c.toUpperCase());
    const exported = Object.keys(bridge);
    const leaked = commands.map(camel).filter((name) => exported.includes(name));
    expect(leaked).toEqual([]);
    for (const name of ["acceptB", "setHeadTo", "openProject", "saveProjectAs", "batchLoad", "renameNode"]) {
      expect(exported, `${name} is a session command`).not.toContain(name);
    }
  });
});

describe("findViolations notices each way the boundary can be broken", () => {
  const commands = ["set_track_gain", "set_head_to"];
  const clean: Sources = {
    "hooks/useSession.ts":
      'import { edits } from "../lib/sessionCommands";\nimport { useSessionHead } from "./useSessionHead";',
    "App.tsx": 'import { useSession } from "./hooks/useSession";\n// set_head_to, said in a comment',
    "lib/sessionCommands.ts": 'invoke<string>("set_track_gain", {}); invoke("set_head_to");',
  };

  it("passes the arrangement the app has", () => {
    expect(findViolations(clean, commands)).toEqual([]);
  });

  it.each([
    ["a component imports the commands", { "components/Mixer.tsx": 'import { edits } from "../lib/sessionCommands";' }],
    ["a component re-exports them, with an extension", { "components/Mixer.tsx": 'export { edits } from "../lib/sessionCommands.ts";' }],
    ["a component loads them late", { "components/Mixer.tsx": 'const m = await import("../lib/sessionCommands");' }],
    ["App moves the head itself", { "App.tsx": 'import { useSessionHead } from "./hooks/useSessionHead";' }],
    ["a handler sends the raw command", { "App.tsx": 'await invoke("set_track_gain", { track: 0, gainDb: -3 });' }],
    ["a handler names the command in a template", { "App.tsx": "await send(`set_head_to`);" }],
  ])("%s", (_, extra) => {
    expect(findViolations({ ...clean, ...extra }, commands)).not.toEqual([]);
  });
});
