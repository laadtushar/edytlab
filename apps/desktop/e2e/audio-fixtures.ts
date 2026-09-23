/**
 * Audio the e2e tests load, written from arithmetic rather than
 * committed as binaries.
 *
 * Generated means every property a test asserts on is known exactly —
 * a 3-second file is 3 seconds because it was written with 132 300
 * frames, not because someone once measured a recording — and nothing
 * opaque ends up in the repository.
 */

import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

/** Where fixtures are written. Ignored by git. */
export const FIXTURE_DIR = join(
  dirname(fileURLToPath(import.meta.url)),
  ".generated",
);

export const SAMPLE_RATE = 44_100;

export interface ToneSpec {
  seconds: number;
  hz: number;
  /** Peak amplitude as a fraction of full scale. */
  amplitude: number;
}

/** A mono 16-bit PCM WAV holding a single sine tone. */
export function toneWav({ seconds, hz, amplitude }: ToneSpec): Buffer {
  const frames = Math.round(seconds * SAMPLE_RATE);
  const data = Buffer.alloc(frames * 2);
  for (let i = 0; i < frames; i++) {
    const v = Math.sin((2 * Math.PI * hz * i) / SAMPLE_RATE) * amplitude;
    data.writeInt16LE(Math.round(v * 32_767), i * 2);
  }

  const header = Buffer.alloc(44);
  header.write("RIFF", 0, "ascii");
  header.writeUInt32LE(36 + data.length, 4);
  header.write("WAVE", 8, "ascii");
  header.write("fmt ", 12, "ascii");
  header.writeUInt32LE(16, 16); // fmt chunk size
  header.writeUInt16LE(1, 20); // PCM
  header.writeUInt16LE(1, 22); // mono
  header.writeUInt32LE(SAMPLE_RATE, 24);
  header.writeUInt32LE(SAMPLE_RATE * 2, 28); // byte rate
  header.writeUInt16LE(2, 32); // block align
  header.writeUInt16LE(16, 34); // bits per sample
  header.write("data", 36, "ascii");
  header.writeUInt32LE(data.length, 40);
  return Buffer.concat([header, data]);
}

/** Every fixture a test can ask for, by name. */
export const FIXTURES = {
  /** Three seconds: wider than the pane once zoomed past ~260 px/s. */
  tone3s: { file: "tone-3s.wav", spec: { seconds: 3, hz: 440, amplitude: 0.5 } },
  /** Text with an audio extension, for the undecodable-file path (#322). */
  notAudio: { file: "not-audio.wav", text: "this is not audio at all" },
} as const;

export type FixtureName = keyof typeof FIXTURES;

/** Absolute path of a fixture, as the backend would report it. */
export function fixturePath(name: FixtureName): string {
  return join(FIXTURE_DIR, FIXTURES[name].file);
}

export function writeFixtures(): void {
  mkdirSync(FIXTURE_DIR, { recursive: true });
  for (const fixture of Object.values(FIXTURES)) {
    const body = "spec" in fixture ? toneWav(fixture.spec) : fixture.text;
    writeFileSync(join(FIXTURE_DIR, fixture.file), body);
  }
}
