/**
 * Only the user's click downloads a model (#383).
 *
 * A model is about 139 MB and the request goes to huggingface.co. The
 * rules are that no tool, agent path or launch code starts one: the one
 * call in the app is the Download (and Retry) button of the Settings row.
 * This reads the authored source and fails when another file calls
 * `downloadMlModel`, or when the row calls it from anywhere but a button's
 * `onClick`, such as an effect that runs when Settings opens.
 *
 * The backend half of the same rule is
 * `crates/ml-pipeline/tests/only_the_user_downloads.rs`.
 */

import { readdirSync, readFileSync } from "node:fs";
import { extname, join, relative } from "node:path";
import { describe, expect, it } from "vitest";

const SRC = join(process.cwd(), "src");

function sources(dir: string = SRC): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      return entry.name === "__tests__" || entry.name === "node_modules" ? [] : sources(path);
    }
    if (![".ts", ".tsx"].includes(extname(path))) return [];
    if (/\.test\.tsx?$/.test(path)) return [];
    return [path];
  });
}

/** The source with whole-line and trailing `//` and `/* *\/` comments cut. */
function code(text: string): string {
  return text
    .replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, " "))
    .replace(/(^|\s)\/\/.*$/gm, "$1");
}

const rel = (path: string) => relative(SRC, path).split("\\").join("/");

describe("downloading a model", () => {
  it("is called from the Settings row and defined in the bridge, nowhere else", () => {
    const callers = sources()
      .filter((p) => /\bdownloadMlModel\b/.test(code(readFileSync(p, "utf8"))))
      .map(rel)
      .sort();
    expect(callers).toEqual(["components/SpeechModelRow.tsx", "lib/tauri-bridge.ts"]);
  });

  it("is called once in the row, in the handler a button's onClick is given", () => {
    const text = code(readFileSync(join(SRC, "components/SpeechModelRow.tsx"), "utf8"));

    const calls = text.match(/\bdownloadMlModel\(/g) ?? [];
    expect(calls).toHaveLength(1);
    expect(text).toMatch(
      /const download = \(\) => void run\(\(\) => downloadMlModel\(SPEECH_MODEL_ID\)\);/,
    );

    // The handler is only ever handed to a button, never called.
    const uses = [...text.matchAll(/\bdownload\b(?!Ml)/g)].map((m) =>
      text.slice(Math.max(0, m.index - 10), m.index + m[0].length + 2),
    );
    const handedOut = uses.filter((u) => /onClick=\{download\}/.test(u));
    const defined = uses.filter((u) => /const download/.test(u));
    expect(handedOut).toHaveLength(2); // Download, and Retry
    expect(defined).toHaveLength(1);
    expect(text).not.toMatch(/\bdownload\(\)/);
  });
});
