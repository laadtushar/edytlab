/**
 * One version, everywhere it is written.
 *
 * `tauri.conf.json` is canonical: it names the installers and the tag a
 * release is cut from. The package manifests mirror it, the status bar
 * prints it, and until this test they could drift apart silently — the
 * status bar said `v0.1.0` as a literal of its own.
 */

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const root = join(process.cwd(), "..", "..");
const read = (...parts: string[]) => readFileSync(join(root, ...parts), "utf8");
const json = (...parts: string[]) => JSON.parse(read(...parts)) as { version: string };

const canonical = json("apps", "desktop", "src-tauri", "tauri.conf.json").version;

describe("the app's version", () => {
  it("is a plain semantic version", () => {
    expect(canonical).toMatch(/^\d+\.\d+\.\d+$/);
  });

  it.each([
    ["package.json"],
    ["apps/desktop/package.json"],
    ["website/package.json"],
  ])("is mirrored by %s", (path) => {
    expect(json(...path.split("/")).version).toBe(canonical);
  });

  it("is the Cargo workspace's version", () => {
    const m = /\[workspace\.package\][^[]*?\nversion = "([^"]+)"/.exec(read("Cargo.toml"));
    expect(m?.[1]).toBe(canonical);
  });

  it("is what the status bar prints", () => {
    // Set by `vite.config.ts` from the same file; vitest runs that config.
    expect(__APP_VERSION__).toBe(canonical);
  });
});
