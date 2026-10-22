// The sound of the v0.7.0 promo's project, written from arithmetic: a
// 16-second A-minor loop at 120 BPM in six stems (drums, bass, pad, keys,
// lead, vocal). Nothing is sampled, recorded or downloaded, and nothing
// here is anyone's music. The app loads these files like any others.
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { voice, wav } from "../fixtures.mjs";

export const BPM = 120;
export const SECONDS = 16;
const BEAT = 60 / BPM;
const BAR = BEAT * 4;
const TAU = 2 * Math.PI;

// One chord a bar: Am, F, C, G (root and triad, in Hz).
const PROG = [
  { root: 55.0, tones: [220.0, 261.63, 329.63] },
  { root: 43.65, tones: [174.61, 220.0, 261.63] },
  { root: 65.41, tones: [196.0, 261.63, 329.63] },
  { root: 49.0, tones: [196.0, 246.94, 293.66] },
];
const chordAt = (t) => PROG[Math.floor(t / BAR) % PROG.length];
const noise = (t) => {
  const v = Math.sin(t * 12_989.8 + 78.233) * 43_758.5453;
  return (v - Math.floor(v)) * 2 - 1;
};

const drums = (t) => {
  const inBeat = t % BEAT;
  const beat = Math.floor(t / BEAT) % 4;
  const kick = Math.sin(TAU * (48 + 100 * Math.exp(-inBeat * 32)) * inBeat) * Math.exp(-inBeat * 8) * 0.9;
  const clap = beat % 2 === 1 ? noise(t * 1.7) * Math.exp(-inBeat * 25) * 0.35 : 0;
  const off = (t + BEAT / 2) % BEAT;
  const hat = noise(t) * Math.exp(-off * 70) * 0.18;
  return (kick + clap + hat) * 0.8;
};
const bass = (t) => {
  const eighth = t % (BEAT / 2);
  const f = chordAt(t).root;
  const saw = Math.sin(TAU * f * t) + 0.35 * Math.sin(TAU * 2 * f * t) + 0.12 * Math.sin(TAU * 3 * f * t);
  return saw * Math.exp(-eighth * 5) * 0.4;
};
const pad = (t) => {
  const breathe = 0.6 + 0.4 * Math.sin((TAU * t) / BAR);
  return chordAt(t).tones.reduce((a, f) => a + Math.sin(TAU * f * t) + 0.5 * Math.sin(TAU * f * 2.005 * t), 0) * 0.045 * breathe;
};
const keys = (t) => {
  const step = (t % (BEAT / 4)) / (BEAT / 4);
  const tones = chordAt(t).tones;
  const f = tones[Math.floor(t / (BEAT / 4)) % tones.length] * 2;
  return Math.sin(TAU * f * t) * Math.exp(-step * 5.5) * 0.17;
};
const RIFF = [0, 3, 7, 10, 7, 3, 0, -2];
const lead = (t) => {
  if (t < 2 * BAR) return 0;
  const eighth = t % (BEAT / 2);
  const f = 440 * 2 ** (RIFF[Math.floor(t / (BEAT / 2)) % RIFF.length] / 12);
  return (Math.sin(TAU * f * t) + 0.25 * Math.sin(TAU * 2 * f * t)) * Math.exp(-eighth * 8) * 0.16;
};
const v = voice();
const vocal = (t) => v(t % 5.6) * 1.9;

export const STEMS = { drums, bass, pad, keys, lead, vocal };
const mixOf = (names) => (t) => names.reduce((a, n) => a + STEMS[n](t), 0);

/** Write the stems named in `files` ({ "Backing.wav": ["drums", "bass", ...] }) into `dir` and return their paths in order. */
export function writeStems(dir, files, seconds = SECONDS, rate = 44100) {
  mkdirSync(dir, { recursive: true });
  return Object.entries(files).map(([name, parts]) => {
    const f = mixOf(Array.isArray(parts) ? parts : [parts]);
    const path = join(dir, name);
    const level = Array.isArray(parts) && parts.length > 1 ? 0.5 : 0.8;
    writeFileSync(path, wav({ seconds, rate, sample: (t) => f(t) * level }));
    return path;
  });
}
