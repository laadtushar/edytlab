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
  /** Length of the tone itself, after any lead-in. */
  seconds: number;
  hz: number;
  /** Peak amplitude as a fraction of full scale. */
  amplitude: number;
  /** Seconds of digital silence before the tone starts. */
  leadIn?: number;
}

/** A mono 16-bit PCM WAV holding a single sine tone. */
export function toneWav({ seconds, hz, amplitude, leadIn = 0 }: ToneSpec): Buffer {
  const silent = Math.round(leadIn * SAMPLE_RATE);
  const frames = silent + Math.round(seconds * SAMPLE_RATE);
  const data = Buffer.alloc(frames * 2);
  for (let i = silent; i < frames; i++) {
    const v = Math.sin((2 * Math.PI * hz * (i - silent)) / SAMPLE_RATE) * amplitude;
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
  /** Three seconds at another pitch: the B side of an A/B comparison. */
  tone3sB: { file: "tone-3s-b.wav", spec: { seconds: 3, hz: 220, amplitude: 0.5 } },
  /** One second: a track that ends before the session does. */
  tone1s: { file: "tone-1s.wav", spec: { seconds: 1, hz: 330, amplitude: 0.5 } },
  /**
   * What `list_tracks` hands the lane for `tone1s` placed at 1 s: the
   * file `flattened_track_wav` writes for that clip, silent until the
   * clip starts (#348).
   */
  tone1sAt1s: {
    file: "tone-1s-at-1s.wav",
    spec: { seconds: 1, hz: 330, amplitude: 0.5, leadIn: 1 },
  },
} as const;

export type FixtureName = keyof typeof FIXTURES;

/** Absolute path of a fixture, as the backend would report it. */
export function fixturePath(name: FixtureName): string {
  return join(FIXTURE_DIR, FIXTURES[name].file);
}

/** Length of a fixture in seconds, lead-in included, from its spec. */
export function fixtureSeconds(name: FixtureName): number {
  const spec: ToneSpec = FIXTURES[name].spec;
  return (spec.leadIn ?? 0) + spec.seconds;
}

export function writeFixtures(): void {
  mkdirSync(FIXTURE_DIR, { recursive: true });
  for (const { file, spec } of Object.values(FIXTURES)) {
    writeFileSync(join(FIXTURE_DIR, file), toneWav(spec));
  }
}
