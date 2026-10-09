/**
 * `isDerivedAudioPath` tells audio the app wrote from audio the user
 * brought (#416).
 *
 * A destructive edit points its clip at a content-addressed file under
 * `<project>/.audiograph/derived/`, named by the hash of its samples.
 * The clip chip and the status bar named that file, so a person who
 * reversed `music.wav` saw it renamed to 64 hex digits. Both now ask
 * this function first.
 */

import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

import { isDerivedAudioPath } from "../lib/derivedAudio";

/** The name in #416's report: what a reversed file was called. */
const HASH = "703e2a7b07919f079abdfbd8c3556c274c8d519c40ef72fe75586ba5bada370e";

/** A `pub const NAME: &str = "…";` from a Rust source file. */
function rustConst(file: string, name: string): string {
  const source = readFileSync(resolve(__dirname, "../../../../", file), "utf8");
  const m = new RegExp(`pub const ${name}: &str = "([^"]+)";`).exec(source);
  expect(m, `${name} in ${file}`).not.toBeNull();
  return m![1];
}

describe("isDerivedAudioPath", () => {
  it("recognises an edit's output on a POSIX path", () => {
    expect(
      isDerivedAudioPath(`/home/me/Music/song/.audiograph/derived/${HASH}.wav`),
    ).toBe(true);
  });

  /** The app ships on Windows, where `PathBuf::join` writes `\`. */
  it("recognises an edit's output on a Windows path", () => {
    expect(
      isDerivedAudioPath(
        `C:\\Users\\tusha\\Music\\song\\.audiograph\\derived\\${HASH.toUpperCase()}.wav`,
      ),
    ).toBe(true);
  });

  it("recognises a Windows verbatim path and mixed separators", () => {
    expect(
      isDerivedAudioPath(`\\\\?\\C:\\Music\\song\\.audiograph\\derived\\${HASH}.wav`),
    ).toBe(true);
    expect(
      isDerivedAudioPath(`C:\\Music\\song/.audiograph/derived/${HASH}.wav`),
    ).toBe(true);
  });

  /**
   * A track cut into clips is flattened for its lane into the same
   * directory (`tools::lane_audio_path`), and the status bar names it.
   */
  it("recognises a flattened lane file", () => {
    expect(
      isDerivedAudioPath(`/home/me/Music/song/.audiograph/derived/track-${HASH}.wav`),
    ).toBe(true);
  });

  /**
   * Built the way `session::relocate::derived_dir` builds it, from the
   * Rust constants themselves, so renaming either directory there
   * cannot quietly bring the hash back on screen.
   */
  it("matches the directory the backend writes derived audio to", () => {
    const store = rustConst("crates/session/src/store.rs", "STORE_DIR");
    const derived = rustConst("crates/session/src/relocate.rs", "DERIVED_DIR");
    expect(isDerivedAudioPath(["/proj", store, derived, `${HASH}.wav`].join("/"))).toBe(
      true,
    );
    expect(
      isDerivedAudioPath(["C:\\proj", store, derived, `${HASH}.wav`].join("\\")),
    ).toBe(true);
  });

  it("leaves a file the user brought alone", () => {
    expect(isDerivedAudioPath("/home/me/Music/music.wav")).toBe(false);
    expect(isDerivedAudioPath("C:\\Users\\tusha\\Music\\music.wav")).toBe(false);
    expect(isDerivedAudioPath("music.wav")).toBe(false);
    expect(isDerivedAudioPath("")).toBe(false);
  });

  /** "derived" is an ordinary word; only the store's own directory counts. */
  it("is not fooled by the word derived elsewhere in the path", () => {
    for (const path of [
      "/home/me/derived/take.wav",
      "C:\\Users\\me\\derived\\take.wav",
      "/home/me/Music/derived.wav",
      "/home/me/derived-mixes/take.wav",
      // The two directories, but not one inside the other.
      "/home/me/derived/.audiograph/take.wav",
      "/home/me/.audiograph/stems/derived/take.wav",
      // Look-alikes of the store directory.
      "/home/me/audiograph/derived/take.wav",
      "/home/me/.audiograph-old/derived/take.wav",
      // A file named `derived`, not a file inside it.
      "/home/me/proj/.audiograph/derived.wav",
    ]) {
      expect(isDerivedAudioPath(path), path).toBe(false);
    }
  });

  it("does not call the directory itself a derived file", () => {
    expect(isDerivedAudioPath("/proj/.audiograph/derived")).toBe(false);
    expect(isDerivedAudioPath("/proj/.audiograph/derived/")).toBe(false);
  });
});
