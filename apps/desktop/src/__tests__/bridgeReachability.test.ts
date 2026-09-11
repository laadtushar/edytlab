/**
 * Every bridge capability has a way to reach it (#225).
 *
 * The audit behind #225 checked three layers and found the seam.
 * Tauri commands → bridge calls was **86/86**, clean. Bridge exports →
 * referenced by a component or hook was **81/91**. So nothing goes
 * missing between Rust and TypeScript; everything that goes missing
 * goes missing in the last inch — the capability arrives in the
 * frontend and then no button, menu item or shortcut ever calls it.
 *
 * That is how the app shipped able to *store* an API key but not remove
 * one, and with a scheduled-recording feature whose entire purpose is
 * running unattended and no way to ask for it.
 *
 * The ticket says this guard matters more than any individual fix, and
 * that is right: each of those gaps was invisible until someone tried
 * to use the app, and the same audit is worth running after every
 * feature that adds a command. Without it the list simply regrows.
 *
 * ## How it is meant to be used
 *
 * `KNOWN_UNREACHABLE` is a ratchet, not a wishlist. A new export with
 * no caller fails immediately; the existing gaps are listed with the
 * reason they are still there. **Removing an entry is the fix** — the
 * list should only ever get shorter.
 *
 * ## Why text matching
 *
 * Deliberately crude — read the files and compare name sets — in the
 * same spirit as `apiReferenceCoverage.test.ts` and the Rust-side
 * `website_tool_docs.rs`. A parser clever enough to be elegant is a
 * parser that can silently stop matching, and a reachability check
 * that matches nothing passes perfectly.
 */

import { readdirSync, readFileSync, statSync } from "node:fs";
import { extname, join } from "node:path";
import { describe, expect, it } from "vitest";

const SRC = join(process.cwd(), "src");
const BRIDGE = join(SRC, "lib", "tauri-bridge.ts");

/**
 * Exports that legitimately have no UI caller, and why.
 *
 * Two kinds. The first is genuinely dead and staying: back-compat for
 * the legacy unsuffixed keychain slot, superseded by the per-provider
 * `…For` variants. The rest are the real gaps #225 catalogues — each
 * is a capability a user cannot reach, listed here so that *new* ones
 * fail the build while the known ones are tracked rather than
 * forgotten.
 */
const KNOWN_UNREACHABLE: Record<string, string> = {
  // Legitimately dead — no action.
  setApiKey:
    "superseded by setApiKeyFor; kept for the legacy unsuffixed keychain slot",
  testApiKey:
    "superseded by testApiKeyFor; kept for the legacy unsuffixed keychain slot",

  // Real gaps (#225). Deleting a line here is the fix.
  clearApiKeyFor:
    "#225 §1 — a key can be stored from Settings but only removed via Keychain Access",
  getActiveProvider:
    "#225 §3 — Settings persists the provider to localStorage instead of reading the backend",
  listProviders:
    "#225 §3 — Settings hardcodes a PROVIDERS array",
  timerRecord:
    "#225 §4 — scheduled recording has no UI, which is the one thing it is for",
  getProjectMeta:
    "#225 §5 — title/artist/album feed the export tag writers but are agent-only",
  setProjectMeta:
    "#225 §5 — project metadata can only be written by asking the agent in a sentence",
};

/** Every name the bridge exports as a callable. */
function bridgeExports(): Set<string> {
  const src = readFileSync(BRIDGE, "utf8");
  const names = new Set<string>();
  for (const m of src.matchAll(/^export (?:async )?function ([A-Za-z0-9_]+)/gm)) {
    names.add(m[1]);
  }
  for (const m of src.matchAll(/^export const ([A-Za-z0-9_]+)\s*=/gm)) {
    names.add(m[1]);
  }
  return names;
}

/**
 * Authored UI source: components, hooks, `App.tsx` — everything except
 * the bridge itself and the tests.
 *
 * Tests are excluded on purpose. A capability referenced only by its
 * own unit test is exactly as unreachable as one referenced nowhere,
 * and counting them would make this guard congratulate the app for
 * code no user can run.
 */
function uiSources(): string[] {
  const out: string[] = [];
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) {
        if (name === "__tests__" || name === "node_modules") continue;
        walk(path);
        continue;
      }
      if (![".ts", ".tsx"].includes(extname(path))) continue;
      if (path === BRIDGE) continue;
      if (/\.test\.tsx?$/.test(path)) continue;
      out.push(readFileSync(path, "utf8"));
    }
  };
  walk(SRC);
  return out;
}

const exportsSet = bridgeExports();
const sources = uiSources();

function isReferenced(name: string): boolean {
  const re = new RegExp(`\\b${name}\\b`);
  return sources.some((s) => re.test(s));
}

describe("the bridge's last inch", () => {
  /**
   * Asserted first and separately. A walk that reads nothing, or a
   * regex that stops matching, would make every check below pass while
   * measuring nothing — which is the failure mode that turns a guard
   * into decoration.
   */
  it("actually read the bridge and the UI", () => {
    expect(exportsSet.size, "found no bridge exports").toBeGreaterThan(80);
    expect(sources.length, "found no UI sources").toBeGreaterThan(30);
  });

  /**
   * The check. A capability that reaches the frontend and is called by
   * nothing is one a user cannot get to, however well it works.
   */
  it("has a caller for every bridge export", () => {
    const unreachable = [...exportsSet]
      .filter((name) => !isReferenced(name))
      .filter((name) => !(name in KNOWN_UNREACHABLE))
      .sort();

    expect(
      unreachable,
      `these bridge exports have no component or hook calling them, so the capability exists ` +
        `and no user can reach it. Wire one up, or add it to KNOWN_UNREACHABLE with the reason:\n  ` +
        unreachable.join("\n  "),
    ).toEqual([]);
  });

  /**
   * The ratchet's other direction, and the part that keeps the list
   * honest: an entry whose export *is* now reachable must be deleted.
   *
   * Without this the allowlist only ever grows, and a fix leaves
   * behind a line claiming the gap is still open — which is how a
   * tracking list stops describing anything.
   */
  it("has no stale allowlist entries", () => {
    const fixed = Object.keys(KNOWN_UNREACHABLE)
      .filter((name) => exportsSet.has(name))
      .filter((name) => isReferenced(name))
      .sort();

    expect(
      fixed,
      `these are listed in KNOWN_UNREACHABLE but now have a caller — delete their lines, that ` +
        `is what the fix looks like:\n  ` + fixed.join("\n  "),
    ).toEqual([]);
  });

  /**
   * And an entry naming an export that no longer exists is dead too.
   */
  it("has no allowlist entries for exports that are gone", () => {
    const gone = Object.keys(KNOWN_UNREACHABLE)
      .filter((name) => !exportsSet.has(name))
      .sort();

    expect(
      gone,
      `KNOWN_UNREACHABLE names exports the bridge no longer has:\n  ` + gone.join("\n  "),
    ).toEqual([]);
  });

  /**
   * Every entry has to say *why*. A bare name is indistinguishable
   * from someone silencing the guard, which is the one way this file
   * becomes worse than not existing.
   */
  it("gives a reason for every allowlisted export", () => {
    const unexplained = Object.entries(KNOWN_UNREACHABLE)
      .filter(([, reason]) => reason.trim().length < 20)
      .map(([name]) => name);

    expect(
      unexplained,
      `these allowlist entries have no real explanation, so there is no way to tell a tracked ` +
        `gap from a silenced failure:\n  ` + unexplained.join("\n  "),
    ).toEqual([]);
  });
});
