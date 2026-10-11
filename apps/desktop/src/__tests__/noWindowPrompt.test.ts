/**
 * Nothing in the app asks for text with `window.prompt` (#546).
 *
 * The desktop shell has no prompt to show. A macOS webview answers
 * `window.prompt` with `null`: WKWebView only shows one when the host
 * implements the panel handler, and neither wry nor tauri-plugin-dialog
 * does (the plugin's init script replaces `alert` and `confirm` only).
 * The ruler's add-marker asked for the marker's name that way, so there a
 * click on the ruler did nothing at all, and nothing said why.
 *
 * A name is typed into a box in the page instead. This guard keeps the
 * call from coming back: it reads the authored source, not the tests,
 * and looks at code only, so a comment may still say why.
 */

import { readdirSync, readFileSync } from "node:fs";
import { extname, join, relative } from "node:path";
import { describe, expect, it } from "vitest";

const SRC = join(process.cwd(), "src");

/** Every authored `.ts` / `.tsx` source under `src`, tests left out. */
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

/**
 * The source with its comments taken out, line for line.
 *
 * A `//` is a comment only at the start of a line or after a space, so
 * the `//` in `https://` is left alone.
 */
function withoutComments(source: string): string[] {
  const noBlocks = source.replace(/\/\*[\s\S]*?\*\//g, (block) => block.replace(/[^\n]/g, " "));
  return noBlocks.split("\n").map((line) => line.replace(/(^|\s)\/\/.*$/, "$1"));
}

/** `window.prompt(` and its aliases, or a bare `prompt(` call. */
const PROMPT_CALL = /\b(?:window|globalThis|self)\s*\.\s*prompt\s*\(|(?<![.\w])prompt\s*\(/;

describe("no window.prompt in the app", () => {
  it("reads some source, so the scan below is not vacuous", () => {
    const files = sources();
    expect(files.length).toBeGreaterThan(20);
    expect(files.some((f) => f.endsWith("App.tsx"))).toBe(true);
  });

  it("is not called anywhere outside the tests", () => {
    const calls = sources().flatMap((file) =>
      withoutComments(readFileSync(file, "utf8")).flatMap((line, i) =>
        PROMPT_CALL.test(line) ? [`${relative(SRC, file)}:${i + 1}: ${line.trim()}`] : [],
      ),
    );
    expect(
      calls,
      `window.prompt returns null in a macOS webview, so whatever asks for ` +
        `text this way does nothing there. Take the text in a box in the page.`,
    ).toEqual([]);
  });

  it("sees a call, so a silent scan means there is none", () => {
    for (const call of [
      'const name = window.prompt("Marker name:") ?? "";',
      'const n = prompt("Name?");',
      'globalThis.prompt ("x")',
    ]) {
      expect(PROMPT_CALL.test(call), call).toBe(true);
    }
    for (const fine of [
      "const systemPrompt = build();",
      "const text = buildPrompt(args);",
      "// a prompt (see #546)",
      "agent.prompt(text)",
    ]) {
      expect(withoutComments(fine).some((l) => PROMPT_CALL.test(l)), fine).toBe(false);
    }
  });
});
