/**
 * The audio the screenshots load: short synthetic loops, written from
 * arithmetic (the way `apps/desktop/e2e/audio-fixtures.ts` writes its
 * tones), so nothing opaque is committed and nothing has a licence.
 *
 * Two tracks at 120 BPM in F major, 32 seconds (16 bars) each, mono
 * 16-bit PCM at 44.1 kHz, built from a kick, clap and hats, a bass, a pad
 * and an arpeggio. They exist so the timeline lanes in the screenshots
 * draw waveforms that look like music (a groove that thins out, a build)
 * instead of the flat bars a sine tone gives.
 */

import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

export const SAMPLE_RATE = 44_100;
export const BPM = 120;
const BEAT = 60 / BPM;

/** A small deterministic generator, so a render today equals one next year. */
function noise(seed: number): () => number {
  let s = seed >>> 0;
  return () => {
    s = (Math.imul(s, 1664525) + 1013904223) >>> 0;
    return (s / 0xffffffff) * 2 - 1;
  };
}

function wav(samples: Float32Array): Buffer {
  const data = Buffer.alloc(samples.length * 2);
  for (let i = 0; i < samples.length; i++) {
    const v = Math.max(-1, Math.min(1, samples[i]));
    data.writeInt16LE(Math.round(v * 32_767), i * 2);
  }
  const header = Buffer.alloc(44);
  header.write("RIFF", 0, "ascii");
  header.writeUInt32LE(36 + data.length, 4);
  header.write("WAVE", 8, "ascii");
  header.write("fmt ", 12, "ascii");
  header.writeUInt32LE(16, 16);
  header.writeUInt16LE(1, 20); // PCM
  header.writeUInt16LE(1, 22); // mono
  header.writeUInt32LE(SAMPLE_RATE, 24);
  header.writeUInt32LE(SAMPLE_RATE * 2, 28);
  header.writeUInt16LE(2, 32);
  header.writeUInt16LE(16, 34);
  header.write("data", 36, "ascii");
  header.writeUInt32LE(data.length, 40);
  return Buffer.concat([header, data]);
}

function add(into: Float32Array, at: number, voice: Float32Array, gain = 1): void {
  const start = Math.round(at * SAMPLE_RATE);
  for (let i = 0; i < voice.length && start + i < into.length; i++) into[start + i] += voice[i] * gain;
}

function kick(): Float32Array {
  const n = Math.round(0.32 * SAMPLE_RATE);
  const out = new Float32Array(n);
  let phase = 0;
  for (let i = 0; i < n; i++) {
    const t = i / SAMPLE_RATE;
    phase += (2 * Math.PI * (46 + 110 * Math.exp(-t * 28))) / SAMPLE_RATE;
    out[i] = Math.sin(phase) * Math.exp(-t * 9) * 0.95;
  }
  return out;
}

function clap(rand: () => number): Float32Array {
  const n = Math.round(0.22 * SAMPLE_RATE);
  const out = new Float32Array(n);
  let low = 0;
  for (let i = 0; i < n; i++) {
    const t = i / SAMPLE_RATE;
    low += 0.35 * (rand() - low);
    out[i] = (rand() - low) * Math.exp(-t * 20) * 0.55;
  }
  return out;
}

function hat(rand: () => number): Float32Array {
  const n = Math.round(0.06 * SAMPLE_RATE);
  const out = new Float32Array(n);
  let prev = 0;
  for (let i = 0; i < n; i++) {
    const t = i / SAMPLE_RATE;
    const x = rand();
    out[i] = (x - prev) * Math.exp(-t * 70) * 0.28;
    prev = x;
  }
  return out;
}

/** Hz of a MIDI note. */
const hz = (midi: number) => 440 * 2 ** ((midi - 69) / 12);

/** How loud a voice is in a given bar (0 = silent). */
type Level = (bar: number) => number;

function drums(bars: number, level: Level): Float32Array {
  const out = new Float32Array(Math.round(bars * 4 * BEAT * SAMPLE_RATE));
  const rand = noise(7);
  const k = kick();
  const c = clap(rand);
  const h = hat(rand);
  for (let b = 0; b < bars * 4; b++) {
    const bar = Math.floor(b / 4);
    const lv = level(bar);
    if (lv <= 0) continue;
    // Below 0.5 only the hats play: a breakdown or an intro.
    if (lv >= 0.5) {
      add(out, b * BEAT, k, lv);
      if (b % 4 === 1 || b % 4 === 3) add(out, b * BEAT, c, lv);
    }
    add(out, b * BEAT + BEAT / 2, h, Math.min(1, lv + 0.3));
    if (b % 2 === 1) add(out, b * BEAT + BEAT * 0.75, h, 0.5 * Math.min(1, lv + 0.3));
  }
  return out;
}

function bass(bars: number, level: Level): Float32Array {
  const out = new Float32Array(Math.round(bars * 4 * BEAT * SAMPLE_RATE));
  // F, F, A#, C per bar, an octave pattern on the off-beats.
  const roots = [29, 29, 34, 36];
  for (let bar = 0; bar < bars; bar++) {
    const lv = level(bar);
    if (lv <= 0) continue;
    const root = roots[bar % 4];
    for (let step = 0; step < 8; step++) {
      const at = bar * 4 * BEAT + step * (BEAT / 2);
      const f = hz(root + (step % 2 === 1 ? 12 : 0));
      const n = Math.round(BEAT * 0.45 * SAMPLE_RATE);
      const voice = new Float32Array(n);
      for (let i = 0; i < n; i++) {
        const t = i / SAMPLE_RATE;
        const env = Math.min(1, t / 0.006) * Math.exp(-t * 5);
        const saw = 2 * ((t * f) % 1) - 1;
        voice[i] = (Math.sin(2 * Math.PI * f * t) * 0.7 + saw * 0.22) * env * 0.6;
      }
      add(out, at, voice, lv);
    }
  }
  return out;
}

function pad(bars: number, level: Level): Float32Array {
  const out = new Float32Array(Math.round(bars * 4 * BEAT * SAMPLE_RATE));
  // Fmaj7, Dm7, Bbmaj7, C: two bars each, a slow swell on every chord.
  const chords = [
    [53, 57, 60, 64],
    [50, 53, 57, 60],
    [58, 62, 65, 69],
    [48, 55, 60, 64],
  ];
  const bar = 4 * BEAT;
  for (let c = 0; c < bars / 2; c++) {
    const lv = level(c * 2);
    if (lv <= 0) continue;
    const notes = chords[c % chords.length];
    const len = 2 * bar;
    const n = Math.round(len * SAMPLE_RATE);
    const voice = new Float32Array(n);
    for (let i = 0; i < n; i++) {
      const t = i / SAMPLE_RATE;
      const swell = Math.sin(Math.PI * (t / len)) ** 1.4;
      let v = 0;
      for (const m of notes) {
        v += Math.sin(2 * Math.PI * hz(m) * t) + 0.5 * Math.sin(2 * Math.PI * hz(m) * 1.004 * t);
      }
      voice[i] = (v / (notes.length * 1.5)) * swell * 0.55;
    }
    add(out, c * len, voice, lv);
  }
  return out;
}

function arp(bars: number, level: Level): Float32Array {
  const out = new Float32Array(Math.round(bars * 4 * BEAT * SAMPLE_RATE));
  const scale = [65, 69, 72, 76, 72, 69, 67, 70, 74, 77, 74, 70];
  for (let s = 0; s < bars * 8; s++) {
    const lv = level(Math.floor(s / 8));
    if (lv <= 0) continue;
    const f = hz(scale[s % scale.length]);
    const n = Math.round(BEAT * 0.5 * SAMPLE_RATE);
    const voice = new Float32Array(n);
    for (let i = 0; i < n; i++) {
      const t = i / SAMPLE_RATE;
      const env = Math.min(1, t / 0.004) * Math.exp(-t * 9);
      voice[i] = (Math.sin(2 * Math.PI * f * t) + 0.3 * Math.sin(4 * Math.PI * f * t)) * env * 0.38;
    }
    add(out, s * (BEAT / 2), voice, lv);
  }
  return out;
}

function mix(parts: Float32Array[]): Float32Array {
  const out = new Float32Array(Math.max(...parts.map((p) => p.length)));
  for (const p of parts) for (let i = 0; i < p.length; i++) out[i] += p[i];
  let peak = 0;
  for (let i = 0; i < out.length; i++) peak = Math.max(peak, Math.abs(out[i]));
  const scale = peak > 0 ? 0.89 / peak : 1;
  for (let i = 0; i < out.length; i++) out[i] *= scale;
  return out;
}

/** Silence of `seconds` before `samples`: what `list_tracks` hands a placed clip's lane. */
function placed(samples: Float32Array, seconds: number): Float32Array {
  const lead = Math.round(seconds * SAMPLE_RATE);
  const out = new Float32Array(lead + samples.length);
  out.set(samples, lead);
  return out;
}

const BARS = 16;
const on = (from: number, to: number, v = 1): Level => (bar) => (bar >= from && bar < to ? v : 0);

/**
 * Two 16-bar loops at 120 BPM, arranged so each lane has a shape: A is a
 * full groove that thins out over its last four bars (the outgoing
 * track), B starts with a pad and an arpeggio and builds a beat under
 * them (the incoming one).
 */
const TRACKS: Record<string, () => Float32Array> = {
  a: () =>
    mix([
      drums(BARS, (b) => (b < 2 ? 0.35 : b < 12 ? 1 : b < 14 ? 0.6 : 0.3)),
      bass(BARS, on(2, 12, 1)),
      pad(BARS, (b) => (b < 12 ? 0.8 : 0.5)),
      arp(BARS, on(6, 12, 0.9)),
    ]),
  b: () =>
    mix([
      drums(BARS, (b) => (b < 4 ? 0 : b < 6 ? 0.35 : 1)),
      bass(BARS, on(8, 16, 1)),
      pad(BARS, on(0, 12, 0.9)),
      arp(BARS, (b) => (b < 4 ? 0.5 : b < 12 ? 0.9 : 0)),
    ]),
};

export interface Loop {
  /** Where the clip starts on the timeline, in seconds. */
  startSec: number;
  /** Clip length, in seconds. */
  lengthSec: number;
  /** The file as the clip's source (no lead-in). */
  source: string;
  /** The file as the lane plays it (lead-in silence included). */
  lane: string;
}

/**
 * Write each track, and its lane file for the start it is placed at,
 * into `dir`. Returns where each landed.
 */
export function writeLoops(dir: string, starts: Record<string, number>): Record<string, Loop> {
  mkdirSync(dir, { recursive: true });
  const out: Record<string, Loop> = {};
  for (const [name, make] of Object.entries(TRACKS)) {
    const samples = make();
    const startSec = starts[name] ?? 0;
    const source = join(dir, `track-${name}.wav`);
    const lane = startSec === 0 ? source : join(dir, `track-${name}-at-${startSec}s.wav`);
    mkdirSync(dirname(source), { recursive: true });
    writeFileSync(source, wav(samples));
    if (startSec !== 0) writeFileSync(lane, wav(placed(samples, startSec)));
    out[name] = { startSec, lengthSec: samples.length / SAMPLE_RATE, source, lane };
  }
  return out;
}
