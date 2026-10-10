/**
 * A signed macOS build can record from the microphone (#464).
 *
 * `crates/recorder` captures through cpal's input device, and the bundle
 * is signed with the hardened runtime on (notarization requires it). Under
 * the hardened runtime macOS only lets an app open the microphone if it
 * carries `com.apple.security.device.audio-input`, and asks the user for
 * permission only if `Info.plist` has an `NSMicrophoneUsageDescription` to
 * show. The entitlement was `<false/>` and the string was nowhere, so a
 * signed build probably could not record at all.
 *
 * This cannot run a signed build, so it pins the two files that decide it.
 * Whether the system then prompts and captures audio is checked by hand on
 * a signed macOS build.
 */

import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

const tauriDir = resolve(__dirname, "../../src-tauri");

/** A plist without its comments, which may mention the keys they explain. */
const plist = (name: string): string =>
  readFileSync(resolve(tauriDir, name), "utf8").replace(/<!--[\s\S]*?-->/g, "");

const conf = JSON.parse(readFileSync(resolve(tauriDir, "tauri.conf.json"), "utf8")) as {
  bundle: { macOS: { entitlements?: string; hardenedRuntime?: boolean; infoPlist?: string } };
};

describe("the macOS bundle's microphone access", () => {
  it("is signed with the hardened runtime and the entitlements file", () => {
    // The two things that make the entitlement below necessary and effective.
    expect(conf.bundle.macOS.hardenedRuntime).toBe(true);
    expect(conf.bundle.macOS.entitlements).toBe("entitlements.plist");
  });

  it("holds the audio-input entitlement", () => {
    expect(plist("entitlements.plist")).toMatch(
      /<key>com\.apple\.security\.device\.audio-input<\/key>\s*<true\/>/,
    );
  });

  it("has an Info.plist next to tauri.conf.json, which Tauri merges into the bundle", () => {
    // Tauri looks for `Info.plist` in the directory of the config file; the
    // `bundle.macOS.infoPlist` option would name another, and then this
    // file would not be the one merged.
    expect(conf.bundle.macOS.infoPlist).toBeUndefined();
    expect(existsSync(resolve(tauriDir, "Info.plist"))).toBe(true);
  });

  it("tells the user why the app wants the microphone", () => {
    const info = plist("Info.plist");
    expect(info).toContain('<plist version="1.0">');
    const reason = info.match(
      /<key>NSMicrophoneUsageDescription<\/key>\s*<string>([^<]+)<\/string>/,
    )?.[1];
    expect(reason, "NSMicrophoneUsageDescription needs a string").toBeTruthy();
    // macOS shows this in the permission dialog, so it says what happens:
    // recording from the microphone into a track.
    expect(reason).toMatch(/microphone/i);
    expect(reason).toMatch(/record/i);
    expect(reason).toMatch(/track/i);
  });
});
