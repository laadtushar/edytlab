/**
 * A 200-track session for S4 (#618): an orchestral template with sections the
 * way a film-score project lays them out. The strings are twelve tracks
 * (indices 148 to 159), past where the session block in the system prompt stops
 * listing tracks. The block has 8,000 bytes (`SESSION_BLOCK_MAX_BYTES`) for
 * everything; a track line of these names is about 55 bytes, so even if the
 * fixed parts took no bytes at all only 146 of the 200 lines would fit, and the
 * agent has to page for the strings with `list_session`.
 */

import type { TrackSummary } from "../../../../apps/desktop/src/lib/tauri-bridge";

import { RATE, halveRate, stems, writeFixture } from "./music";
import { loadedTrack } from "./studio";

/** [section, how many, which stem its audio comes from]. */
const SECTIONS: [string, number, "backing" | "pad" | "keys" | "lead" | "vocal" | "drums" | "bass"][] = [
  ["Flute", 8, "keys"],
  ["Oboe", 6, "backing"],
  ["Clarinet", 8, "keys"],
  ["Bassoon", 6, "bass"],
  ["Horn", 10, "pad"],
  ["Trumpet", 8, "backing"],
  ["Trombone", 8, "bass"],
  ["Tuba", 4, "bass"],
  ["Timpani", 4, "drums"],
  ["Percussion", 16, "drums"],
  ["Harp", 6, "keys"],
  ["Piano", 6, "keys"],
  ["Celesta", 2, "keys"],
  ["Choir", 8, "vocal"],
  ["Synth Pad", 12, "pad"],
  ["Synth Lead", 8, "lead"],
  ["FX", 28, "backing"],
  // The strings: indices 148 to 159.
  ["Violin I", 3, "pad"],
  ["Violin II", 3, "pad"],
  ["Viola", 2, "pad"],
  ["Cello", 2, "pad"],
  ["Contrabass", 2, "pad"],
  ["Guitar", 8, "keys"],
  ["Bass Guitar", 4, "bass"],
  ["Drum Kit", 12, "drums"],
  ["Vox", 10, "vocal"],
  ["Foley", 6, "drums"],
];

const STRING_SECTIONS = new Set(["Violin I", "Violin II", "Viola", "Cello", "Contrabass"]);
const LETTERS = "abcdefgh";

export interface Orchestra {
  tracks: TrackSummary[];
  /** Indices of the string tracks. */
  strings: number[];
  seconds: number;
}

/** Eight seconds of each stem at half the rate: light enough for 200 lanes. */
export function orchestra(): Orchestra {
  const s = stems();
  const files = new Map<string, string>();
  const seconds = 8;
  const fileFor = (stem: keyof ReturnType<typeof stems>) => {
    let p = files.get(stem);
    if (!p) {
      const half = halveRate(s[stem]).subarray(0, Math.round(seconds * (RATE / 2)));
      p = writeFixture(`Music/score/${stem}.wav`, half, RATE / 2);
      files.set(stem, p);
    }
    return p;
  };

  const tracks: TrackSummary[] = [];
  const strings: number[] = [];
  for (const [section, count, stem] of SECTIONS) {
    for (let i = 0; i < count; i++) {
      // Sections of one or two are named by letter ("Violin I a"), the rest by number.
      const name = STRING_SECTIONS.has(section) || count <= 3 ? `${section} ${LETTERS[i]}` : `${section} ${i + 1}`;
      if (STRING_SECTIONS.has(section)) strings.push(tracks.length);
      tracks.push(loadedTrack(tracks.length + 1, name, fileFor(stem), seconds));
    }
  }
  return { tracks, strings, seconds };
}
