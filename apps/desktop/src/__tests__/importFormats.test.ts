/**
 * Which files the picker offers (#764). M4A/AAC is opened by the operating
 * system's decoder, so it is offered on macOS and Windows and nowhere else.
 * `crates/audio-decoder/tests/supported_extensions.rs` holds these lists equal
 * to the decoder's own.
 */

import { afterEach, describe, expect, it, vi } from "vitest";

import {
  BASE_EXTENSIONS,
  importExtensions,
  importFormatNames,
  osDecodesM4a,
} from "../lib/importFormats";

afterEach(() => {
  vi.unstubAllGlobals();
});

const LINUX = { platform: "Linux x86_64", userAgent: "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15" };
const MAC = { platform: "MacIntel", userAgent: "Mozilla/5.0 (Macintosh; Intel Mac OS X) AppleWebKit/605.1.15" };
const WINDOWS = { platform: "Win32", userAgent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/140" };

describe("importExtensions", () => {
  it("offers the base list on Linux, where there is no system AAC decoder", () => {
    vi.stubGlobal("navigator", LINUX);
    expect(osDecodesM4a()).toBe(false);
    expect(importExtensions()).toEqual(BASE_EXTENSIONS);
    expect(importFormatNames()).not.toContain("m4a");
  });

  it.each([
    ["macOS", MAC],
    ["Windows", WINDOWS],
  ])("adds m4a on %s", (_name, nav) => {
    vi.stubGlobal("navigator", nav);
    expect(osDecodesM4a()).toBe(true);
    expect(importExtensions()).toEqual([...BASE_EXTENSIONS, "m4a"]);
    expect(importFormatNames()).toContain("m4a");
  });

  it("never offers a bare .aac file: only M4A is routed to the system decoder", () => {
    for (const nav of [LINUX, MAC, WINDOWS]) {
      vi.stubGlobal("navigator", nav);
      expect(importExtensions()).not.toContain("aac");
    }
  });
});
