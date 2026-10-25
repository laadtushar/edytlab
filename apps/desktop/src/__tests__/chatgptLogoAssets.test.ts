/**
 * The ChatGPT logo files are OpenAI's approved Sign in with ChatGPT assets,
 * unmodified (#474). Nobody edits, recolours, redraws or re-exports them: this
 * test fails if either file's bytes differ from the official ones.
 *
 * Source: https://developers.openai.com/assets/siwc/sign-in-buttons/
 * `chatgpt-logo-white.svg` and `chatgpt-logo-black.svg` (downloaded
 * 2026-10-25). To take a newer official version, replace both files and
 * change the hashes here and in `assets/chatgpt/README.md`.
 */

import { createHash } from "node:crypto";
import { readFileSync, readdirSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

const dir = resolve(__dirname, "../assets/chatgpt");

const OFFICIAL_SHA256 = {
  "chatgpt-logo-white.svg": "568ce2e8701934856aaafc0ed1c3d0300f6c2ff384d0ae743dd1dea08570e12c",
  "chatgpt-logo-black.svg": "b8b9aba7ba591f54bf1805f422bc23f32c44ee993643cc378cc77f766061f7a1",
} as const;

describe("the bundled ChatGPT logo", () => {
  for (const [file, hash] of Object.entries(OFFICIAL_SHA256)) {
    it(`${file} is byte-identical to OpenAI's official file`, () => {
      const bytes = readFileSync(resolve(dir, file));
      expect(createHash("sha256").update(bytes).digest("hex")).toBe(hash);
    });
  }

  it("holds those two files and a README, and nothing else", () => {
    expect(readdirSync(dir).sort()).toEqual([
      "README.md",
      "chatgpt-logo-black.svg",
      "chatgpt-logo-white.svg",
    ]);
  });

  it("the README names the source, the date, the hashes and the rules", () => {
    const readme = readFileSync(resolve(dir, "README.md"), "utf8");
    expect(readme).toContain("https://developers.openai.com/assets/siwc/sign-in-buttons/chatgpt-logo-white.svg");
    expect(readme).toContain("https://developers.openai.com/assets/siwc/sign-in-buttons/chatgpt-logo-black.svg");
    expect(readme).toContain("2026-10-25");
    for (const hash of Object.values(OFFICIAL_SHA256)) expect(readme).toContain(hash);
    expect(readme).toMatch(/Do not recolour, redraw/);
  });
});
