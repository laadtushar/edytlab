/**
 * Where the ChatGPT logo is drawn (#474): the component that picks OpenAI's
 * white or black file, and the sources that may import them. The logo is an
 * `<img>` of a bundled file, never inlined, never fetched from the network.
 */

import { readFileSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";

import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { ChatGptLogo } from "../components/ChatGptLogo";

describe("ChatGptLogo", () => {
  it("on a dark surface it is the white file, decorative, at the size asked", () => {
    render(<ChatGptLogo tone="on-dark" size={16} />);
    const img = screen.getByTestId("chatgpt-logo");
    expect(img.tagName).toBe("IMG");
    expect(img).toHaveAttribute("data-logo", "white");
    expect(img.getAttribute("src")).toMatch(/chatgpt-logo-white\.svg/);
    expect(img).toHaveAttribute("alt", "");
    expect(img).toHaveAttribute("aria-hidden", "true");
    // Square, as the file is (viewBox 0 0 21 21): the ratio is kept.
    expect(img).toHaveAttribute("width", "16");
    expect(img).toHaveAttribute("height", "16");
  });

  it("on a light surface it is the black file", () => {
    render(<ChatGptLogo tone="on-light" />);
    const img = screen.getByTestId("chatgpt-logo");
    expect(img).toHaveAttribute("data-logo", "black");
    expect(img.getAttribute("src")).toMatch(/chatgpt-logo-black\.svg/);
    expect(img).toHaveAttribute("width", "18");
  });
});

describe("how the logo is brought in", () => {
  const src = resolve(__dirname, "..");
  const sources = (dir: string): string[] =>
    readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
      e.isDirectory()
        ? e.name === "__tests__" || e.name === "assets"
          ? []
          : sources(join(dir, e.name))
        : /\.(ts|tsx)$/.test(e.name)
          ? [join(dir, e.name)]
          : [],
    );

  it("only ChatGptLogo imports the files, as image URLs", () => {
    const importers = sources(src).filter((f) => /assets\/chatgpt\//.test(readFileSync(f, "utf8")));
    expect(importers.map((f) => f.slice(src.length + 1))).toEqual([join("components", "ChatGptLogo.tsx")]);
    const text = readFileSync(resolve(src, "components/ChatGptLogo.tsx"), "utf8");
    expect(text).toMatch(/import blackLogoUrl from "\.\.\/assets\/chatgpt\/chatgpt-logo-black\.svg"/);
    expect(text).toMatch(/import whiteLogoUrl from "\.\.\/assets\/chatgpt\/chatgpt-logo-white\.svg"/);
    // Not `?raw` or `?inline`: the file is never read as text or embedded edited.
    expect(text).not.toMatch(/\.svg\?/);
  });

  it("no source loads the logo, or any OpenAI asset, from the network", () => {
    for (const f of sources(src)) {
      const text = readFileSync(f, "utf8");
      expect(text, f).not.toMatch(/https?:\/\/[^"'`\s]*siwc/);
      expect(text, f).not.toMatch(/developers\.openai\.com\/assets/);
    }
  });
});
