/**
 * The audio the promo shots load, written from arithmetic in this file.
 *
 * The e2e suite's own fixtures are single sine tones, which draw as a flat
 * bar. These are a short piece of music built from simple synthesis in code
 * (an A-minor loop at 120 BPM: drums, bass, pad, keys, lead and a
 * formant-synthesised "vocal"), so the waveforms and the sound the shots play
 * look and sound like a mix. Nothing is sampled, recorded or downloaded, and
 * the shots say it is synthetic audio.
 *
 * Files are written under the e2e fixture directory (`FIXTURE_DIR`), which
 * the preview server serves and which git ignores, and are written again on
 * every run, into a folder named for the synthesis version (`VERSION`).
 */

import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

import { FIXTURE_DIR } from "../../../../apps/desktop/e2e/audio-fixtures";

/** Bump when the synthesis changes, so cached files are rewritten. */
const VERSION = "v1";

export const RATE = 44_100;
export const BPM = 120;
export const BEAT = 60 / BPM;
export const BAR = 4 * BEAT;
/** Eight bars. */
export const LENGTH_SEC = 8 * BAR;

/** Where the promo audio goes: a folder of its own under the e2e fixtures. */
export const PROMO_DIR = join(FIXTURE_DIR, "promo", VERSION);

type Samples = Float32Array;

function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const hz = (midi: number) => 440 * 2 ** ((midi - 69) / 12);
const blank = (seconds: number, rate = RATE): Samples => new Float32Array(Math.round(seconds * rate));

function peakOf(s: Samples): number {
  let p = 0;
  for (let i = 0; i < s.length; i++) p = Math.max(p, Math.abs(s[i]));
  return p;
}

/** Scale so the loudest sample is `peak`. */
export function normalise(s: Samples, peak: number): Samples {
  const p = peakOf(s);
  if (p === 0) return s;
  const k = peak / p;
  const out = new Float32Array(s.length);
  for (let i = 0; i < s.length; i++) out[i] = s[i] * k;
  return out;
}

/** Add `src` into `dst` at `at` seconds, scaled by `gain`. */
function addAt(dst: Samples, src: Samples, at: number, gain = 1, rate = RATE): void {
  const start = Math.round(at * rate);
  for (let i = 0; i < src.length && start + i < dst.length; i++) {
    if (start + i >= 0) dst[start + i] += src[i] * gain;
  }
}

// -- the chords: A minor, F, C, G, one bar each, over eight bars ------------

const CHORDS: { root: number; notes: number[] }[] = [
  { root: 45, notes: [57, 60, 64] }, // Am
  { root: 41, notes: [53, 57, 60] }, // F
  { root: 48, notes: [60, 64, 67] }, // C
  { root: 43, notes: [55, 59, 62] }, // G
  { root: 45, notes: [57, 60, 64] }, // Am
  { root: 41, notes: [53, 57, 60] }, // F
  { root: 43, notes: [55, 59, 62] }, // G
  { root: 45, notes: [57, 60, 64] }, // Am
];

// -- instruments ------------------------------------------------------------

function kick(): Samples {
  const s = blank(0.45);
  let phase = 0;
  for (let i = 0; i < s.length; i++) {
    const t = i / RATE;
    const f = 45 + 110 * Math.exp(-t * 28);
    phase += (2 * Math.PI * f) / RATE;
    s[i] = Math.sin(phase) * Math.exp(-t * 7.5) * 0.95;
  }
  return s;
}

function snare(rand: () => number): Samples {
  const s = blank(0.3);
  let last = 0;
  for (let i = 0; i < s.length; i++) {
    const t = i / RATE;
    const n = rand() * 2 - 1;
    const hp = n - last; // crude high-pass: bright noise
    last = n * 0.4;
    s[i] = (hp * 0.5 * Math.exp(-t * 16) + Math.sin(2 * Math.PI * 190 * t) * 0.45 * Math.exp(-t * 22)) * 0.9;
  }
  return s;
}

function hat(rand: () => number, open = false): Samples {
  const s = blank(open ? 0.22 : 0.07);
  let last = 0;
  for (let i = 0; i < s.length; i++) {
    const t = i / RATE;
    const n = rand() * 2 - 1;
    const hp = n - last;
    last = n;
    s[i] = hp * 0.3 * Math.exp(-t * (open ? 14 : 55));
  }
  return s;
}

/** Drums: kick on 1 and 3, snare on 2 and 4, eighth-note hats, from `fromBar`. */
export function drums(fromBar = 2): Samples {
  const out = blank(LENGTH_SEC);
  const rand = mulberry32(7);
  const k = kick();
  const sn = snare(rand);
  const h = hat(rand);
  const ho = hat(rand, true);
  for (let bar = fromBar; bar < 8; bar++) {
    const t0 = bar * BAR;
    for (let beat = 0; beat < 4; beat++) {
      const t = t0 + beat * BEAT;
      if (beat % 2 === 0) addAt(out, k, t, 1);
      else addAt(out, sn, t, 0.85);
      addAt(out, h, t, 0.7);
      addAt(out, beat === 3 ? ho : h, t + BEAT / 2, 0.5);
    }
    // a kick pickup on the "and" of 3
    addAt(out, k, t0 + 2.5 * BEAT, 0.6);
  }
  return out;
}

/** Bass: saw through a one-pole low-pass, eighth notes on the chord root. */
export function bass(fromBar = 0): Samples {
  const out = blank(LENGTH_SEC);
  for (let bar = fromBar; bar < 8; bar++) {
    const root = CHORDS[bar].root;
    for (let step = 0; step < 8; step++) {
      const t0 = bar * BAR + step * (BEAT / 2);
      const note = step === 3 || step === 7 ? root + 12 : root;
      const f = hz(note);
      const len = Math.round((BEAT / 2) * 0.92 * RATE);
      const seg = new Float32Array(len);
      let lp = 0;
      for (let i = 0; i < len; i++) {
        const t = i / RATE;
        const saw = 2 * ((f * t) % 1) - 1;
        lp += (saw - lp) * 0.07;
        const env = Math.min(1, t / 0.006) * Math.exp(-t * 3.2);
        seg[i] = (lp * 0.8 + Math.sin(2 * Math.PI * f * t) * 0.5) * env;
      }
      addAt(out, seg, t0, 0.8);
    }
  }
  return out;
}

/** Pad: detuned soft saws on the chord, a slow attack and a long release. */
export function pad(): Samples {
  const out = blank(LENGTH_SEC);
  for (let bar = 0; bar < 8; bar++) {
    const len = Math.round((BAR + 0.5) * RATE);
    const seg = new Float32Array(len);
    for (const note of CHORDS[bar].notes) {
      for (const detune of [-0.07, 0.07]) {
        const f = hz(note) * 2 ** (detune / 12);
        let lp = 0;
        for (let i = 0; i < len; i++) {
          const t = i / RATE;
          const saw = 2 * ((f * t) % 1) - 1;
          lp += (saw - lp) * 0.05;
          seg[i] += lp * 0.11;
        }
      }
    }
    for (let i = 0; i < len; i++) {
      const t = i / RATE;
      seg[i] *= Math.min(1, t / 0.5) * Math.min(1, (len / RATE - t) / 0.5);
    }
    addAt(out, seg, bar * BAR, 1);
  }
  return out;
}

/** Keys: a plucked sixteenth-note arpeggio of the chord, an octave up. */
export function keys(): Samples {
  const out = blank(LENGTH_SEC);
  const order = [0, 1, 2, 1, 0, 1, 2, 1, 2, 1, 0, 1, 2, 1, 0, 1];
  for (let bar = 0; bar < 8; bar++) {
    for (let step = 0; step < 16; step++) {
      const note = CHORDS[bar].notes[order[step]] + 12;
      const f = hz(note);
      const len = Math.round(0.5 * RATE);
      const seg = new Float32Array(len);
      for (let i = 0; i < len; i++) {
        const t = i / RATE;
        seg[i] =
          (Math.sin(2 * Math.PI * f * t) + 0.35 * Math.sin(2 * Math.PI * 2 * f * t) + 0.12 * Math.sin(2 * Math.PI * 3 * f * t)) *
          Math.min(1, t / 0.003) *
          Math.exp(-t * 9);
      }
      addAt(out, seg, bar * BAR + step * (BEAT / 4), 0.3);
    }
  }
  return out;
}

/** Lead: a triangle-ish melody from bar 4, a few long notes. */
export function lead(): Samples {
  const out = blank(LENGTH_SEC);
  const notes: [number, number, number][] = [
    [6.0, 0.9, 81], [7.0, 0.9, 79], [8.0, 1.4, 76], [10.0, 0.9, 77], [11.0, 0.9, 79], [12.0, 1.9, 83], [14.0, 1.9, 81],
  ];
  for (const [t0, dur, midi] of notes) {
    const f = hz(midi);
    const len = Math.round((dur + 0.15) * RATE);
    const seg = new Float32Array(len);
    for (let i = 0; i < len; i++) {
      const t = i / RATE;
      const vib = 1 + 0.004 * Math.sin(2 * Math.PI * 5.5 * t) * Math.min(1, t / 0.3);
      const ph = (f * vib * t) % 1;
      const tri = 4 * Math.abs(ph - 0.5) - 1;
      seg[i] = tri * Math.min(1, t / 0.02) * Math.min(1, (len / RATE - t) / 0.15) * (0.6 + 0.4 * Math.exp(-t * 2));
    }
    addAt(out, seg, t0, 0.5);
  }
  return out;
}

/**
 * A "vocal": a harmonic source through two moving formant peaks, with
 * vibrato and a syllable envelope. It sings from bar 3. Synthesised, so it
 * has no words; it is only there to look and sound like a voice line.
 */
export function vocal(): Samples {
  const out = blank(LENGTH_SEC);
  // [start, duration, midi, vowel]
  const phrase: [number, number, number, "ah" | "oh" | "ee"][] = [
    [4.0, 0.9, 76, "ah"], [5.0, 0.4, 74, "ee"], [5.5, 0.45, 72, "oh"], [6.0, 1.4, 74, "ah"],
    [8.0, 0.9, 76, "ah"], [9.0, 0.4, 79, "ee"], [9.5, 0.45, 76, "oh"], [10.0, 1.0, 72, "ah"],
    [11.0, 0.9, 69, "oh"], [12.0, 0.6, 71, "ee"], [12.75, 0.6, 74, "ah"], [13.5, 0.45, 71, "oh"],
    [14.0, 1.9, 69, "ah"],
  ];
  const formants = { ah: [800, 1150], oh: [500, 900], ee: [320, 2300] } as const;
  const rand = mulberry32(11);
  for (const [t0, dur, midi, vowel] of phrase) {
    const f0 = hz(midi);
    const len = Math.round((dur + 0.12) * RATE);
    const seg = new Float32Array(len);
    const [f1, f2] = formants[vowel];
    const level = 0.82 + 0.18 * rand();
    const harmonics = Math.min(28, Math.floor(7000 / f0));
    const weights = new Float32Array(harmonics + 1);
    for (let h = 1; h <= harmonics; h++) {
      const f = h * f0;
      weights[h] =
        (1 / h) * (Math.exp(-(((f - f1) / (f1 * 0.35)) ** 2)) * 1.0 + Math.exp(-(((f - f2) / (f2 * 0.3)) ** 2)) * 0.55 + 0.04);
    }
    let phase = 0;
    for (let i = 0; i < len; i++) {
      const t = i / RATE;
      const vib = 1 + 0.009 * Math.sin(2 * Math.PI * 5.6 * t) * Math.min(1, t / 0.25);
      phase += (2 * Math.PI * f0 * vib) / RATE;
      let v = 0;
      for (let h = 1; h <= harmonics; h++) v += weights[h] * Math.sin(h * phase);
      const breath = (rand() * 2 - 1) * 0.015;
      // a syllable swells a little and settles, as a sung note does
      const env = Math.min(1, t / 0.05) * Math.min(1, (len / RATE - t) / 0.1) * (0.72 + 0.28 * Math.exp(-t * 2.2)) * level;
      seg[i] = (v + breath) * env;
    }
    addAt(out, seg, t0, 1);
  }
  return out;
}

// -- mixing and files -------------------------------------------------------

/** Sum stems (each with its gain in dB) and keep the peak under `ceiling`. */
export function mix(stems: { samples: Samples; gainDb?: number }[], ceiling = 0.92): Samples {
  const length = Math.max(...stems.map((s) => s.samples.length));
  const out = new Float32Array(length);
  for (const { samples, gainDb = 0 } of stems) {
    const k = 10 ** (gainDb / 20);
    for (let i = 0; i < samples.length; i++) out[i] += samples[i] * k;
  }
  // A soft knee only above 0.8, so a quiet mix is untouched.
  for (let i = 0; i < out.length; i++) {
    const v = out[i];
    const a = Math.abs(v);
    if (a > 0.8) out[i] = Math.sign(v) * (0.8 + (ceiling - 0.8) * Math.tanh((a - 0.8) / (ceiling - 0.8)));
  }
  return out;
}

/** `track * gain`: the fade-in the agent's `fade` tool applies (a linear ramp). */
export function fadeIn(s: Samples, fromSec: number, toSec: number, rate = RATE): Samples {
  const out = new Float32Array(s.length);
  const a = Math.round(fromSec * rate);
  const b = Math.round(toSec * rate);
  for (let i = 0; i < s.length; i++) {
    out[i] = i < a ? 0 : i >= b ? s[i] : (s[i] * (i - a)) / (b - a);
  }
  return out;
}

/** A mono 16-bit PCM WAV. */
export function wavBytes(samples: Samples, rate = RATE): Buffer {
  const data = Buffer.alloc(samples.length * 2);
  for (let i = 0; i < samples.length; i++) {
    data.writeInt16LE(Math.round(Math.max(-1, Math.min(1, samples[i])) * 32767), i * 2);
  }
  const header = Buffer.alloc(44);
  header.write("RIFF", 0, "ascii");
  header.writeUInt32LE(36 + data.length, 4);
  header.write("WAVE", 8, "ascii");
  header.write("fmt ", 12, "ascii");
  header.writeUInt32LE(16, 16);
  header.writeUInt16LE(1, 20);
  header.writeUInt16LE(1, 22);
  header.writeUInt32LE(rate, 24);
  header.writeUInt32LE(rate * 2, 28);
  header.writeUInt16LE(2, 32);
  header.writeUInt16LE(16, 34);
  header.write("data", 36, "ascii");
  header.writeUInt32LE(data.length, 40);
  return Buffer.concat([header, data]);
}

/**
 * Write `samples` under the promo fixture folder, at `relPath` (so a file the
 * user "loaded" reads `backing.wav` in the clip strip), and return its
 * absolute path. Rewritten every call: the files are small.
 */
export function writeFixture(relPath: string, samples: Samples, rate = RATE): string {
  const path = join(PROMO_DIR, relPath);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, wavBytes(samples, rate));
  return path;
}

/** Half the sample rate by averaging pairs: the short stems S4 loads 200 times. */
export function halveRate(s: Samples): Samples {
  const out = new Float32Array(Math.floor(s.length / 2));
  for (let i = 0; i < out.length; i++) out[i] = (s[2 * i] + s[2 * i + 1]) / 2;
  return out;
}

export interface Stems {
  backing: Samples;
  vocal: Samples;
  drums: Samples;
  bass: Samples;
  pad: Samples;
  keys: Samples;
  lead: Samples;
}

let cached: Stems | null = null;

/** Every stem, each peak-normalised for display. Built once per process. */
export function stems(): Stems {
  if (cached) return cached;
  const d = normalise(drums(2), 0.85);
  const b = normalise(bass(0), 0.75);
  const p = normalise(pad(), 0.4);
  const k = normalise(keys(), 0.55);
  const l = normalise(lead(), 0.6);
  const v = normalise(vocal(), 0.8);
  // The backing is what an engineer would call the bed: drums, bass and pad.
  const backing = normalise(mix([{ samples: d, gainDb: -2 }, { samples: b, gainDb: -2 }, { samples: p, gainDb: -1 }]), 0.85);
  cached = { backing, vocal: v, drums: d, bass: b, pad: p, keys: k, lead: l };
  return cached;
}
