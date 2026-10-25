import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

// OpenAI's approved ChatGPT logos for the sign-in button, used unaltered
// (developers.openai.com/siwc/website). A recoloured or redrawn copy fails.
const PINNED = {
  "chatgpt-logo-white.svg":
    "568ce2e8701934856aaafc0ed1c3d0300f6c2ff384d0ae743dd1dea08570e12c",
  "chatgpt-logo-black.svg":
    "b8b9aba7ba591f54bf1805f422bc23f32c44ee993643cc378cc77f766061f7a1",
};

describe("OpenAI brand assets", () => {
  for (const [file, sha] of Object.entries(PINNED)) {
    it(`${file} is byte-identical to OpenAI's file`, () => {
      const bytes = readFileSync(join(__dirname, "../public/brand/openai", file));
      expect(createHash("sha256").update(bytes).digest("hex")).toBe(sha);
    });
  }
});
