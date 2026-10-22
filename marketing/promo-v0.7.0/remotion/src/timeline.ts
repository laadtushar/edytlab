// The edit as data: shots cut from the footage, with the camera, callouts and words on each. Pure data and arithmetic
// (no React), so scripts/cues.ts can read the same thing the compositions render: the sound cues and the .srt come
// from here, never from a second copy.
//
// Two clocks. `cam` and `rings` are in SOURCE time (seconds into the clip: they point at things the clip shows, and
// stay right whatever part of the clip a cut uses or how fast). `titles`, `chips` and `speed` badges are in SHOT time
// (seconds from the start of the shot as cut: editorial, they follow the cut).
import {easeInOutCubic, type FocusKF} from './camera.ts';
import type {Box, LayoutKind} from './layout.ts';

export const FPS = 30;

export type ClipId = 's1' | 's2' | 's3' | 's4' | 's5' | 's6' | 's7' | 's8a' | 's8b';

export const CLIPS: Record<ClipId, {file: string; wav?: string; dur: number}> = {
  s1: {file: 's1-describe-it-happens.mp4', wav: 's1-describe-it-happens.wav', dur: 18.93},
  s2: {file: 's2-faders-stay-put.mp4', dur: 6.73},
  s3: {file: 's3-history-graph.mp4', dur: 19.23},
  s4: {file: 's4-200-tracks.mp4', dur: 21.67},
  s5: {file: 's5-runtime-bundled.mp4', dur: 7.23},
  s6: {file: 's6-skills-note.mp4', dur: 11.2},
  s7: {file: 's7-mix-stays-playable.mp4', wav: 's7-mix-stays-playable.wav', dur: 15.6},
  s8a: {file: 's8a-broll-timeline.mp4', dur: 15.17},
  s8b: {file: 's8b-broll-graph.mp4', dur: 13.13},
};

/** A stretch of a clip: source seconds a..b, played at `rate`x. */
export interface Seg {
  a: number;
  b: number;
  rate?: number;
}

export type Tone = 'violet' | 'muted';
export type Side = 'above' | 'below' | 'left' | 'right';

export interface Label {
  t0: number;
  t1: number;
  text: string;
}

/** A callout ring around a rectangle of the app window (CSS px), with an optional label. SOURCE time. */
export interface Ring {
  t0: number;
  t1: number;
  rect: Box;
  /** One label for the whole ring, or a sequence of them (the readout steps). */
  label?: string;
  labels?: Label[];
  side?: Side;
  tone?: Tone;
  /** A label with no ring (the ring is not drawn): for a caption that points at a place without circling it. */
  noRing?: boolean;
}

/** A chip anchored to a corner of the card. SHOT time. */
export interface ChipSpec {
  t0: number;
  t1: number;
  text: string;
  tone?: Tone;
  at: 'tl' | 'tr' | 'bl' | 'br';
}

/** A title: words, with the ones in `hl` set in the gradient. SHOT time. `\n` breaks the line. */
export interface TitleSpec {
  t0: number;
  t1: number;
  text: string;
  hl?: string[];
  sub?: string;
  /** Larger type, centred in the card: the hook. */
  big?: boolean;
}

export interface Shot {
  kind: 'shot';
  id: string;
  clip: ClipId;
  segs: Seg[];
  cam: FocusKF[];
  rings: Ring[];
  chips: ChipSpec[];
  titles: TitleSpec[];
  /** The clip's own sound under this shot. Only at rate 1. */
  audio?: boolean;
  /** UI ticks for moments the footage has no sound for (the clip's own, in SOURCE time; `pitch` 1 is the base tick). */
  ticks: {t: number; pitch?: number}[];
  /** The "real app" note on this shot: true for every shot of app footage. */
  note: boolean;
}

export interface EndCard {
  kind: 'end';
  id: string;
  /** Seconds. */
  dur: number;
}

export type Part = Shot | EndCard;

export interface Cut {
  id: string;
  title: string;
  /** The output files are `edytlab-v0.7.0-<slug>.mp4|.srt` and `...-contact.png`. */
  slug: string;
  /** Where it is meant to be posted (the README's table). */
  platform: string;
  layout: LayoutKind;
  parts: Part[];
  /** Frames two neighbouring parts overlap by (the cross-fade). */
  overlap: number;
}

// ----------------------------------------------------------------------------------------------- arithmetic

export const segDur = (s: Seg) => (s.b - s.a) / (s.rate ?? 1);
export const shotDur = (s: Shot) => s.segs.reduce((t, g) => t + segDur(g), 0);
export const partDur = (p: Part) => (p.kind === 'end' ? p.dur : shotDur(p));
export const partFrames = (p: Part) => Math.round(partDur(p) * FPS);

/** Shot time (seconds) at which source time `t` is on screen. A time in a gap between segments is the start of the next one. */
export function srcToOut(segs: Seg[], t: number): number {
  let out = 0;
  for (const g of segs) {
    if (t < g.a) return out;
    if (t <= g.b) return out + (t - g.a) / (g.rate ?? 1);
    out += segDur(g);
  }
  return out;
}

/** True if source time `t` is inside a segment (on screen at some point). */
export const shown = (segs: Seg[], t: number): boolean => segs.some((g) => t >= g.a && t <= g.b);

/** A ring's span in shot time, or null if none of its span is on screen. */
export function ringSpan(segs: Seg[], r: {t0: number; t1: number}): [number, number] | null {
  if (!segs.some((g) => r.t1 > g.a && r.t0 < g.b)) return null;
  return [srcToOut(segs, r.t0), srcToOut(segs, r.t1)];
}

/** Source time on screen at shot time `t` (the last frame of the last stretch if `t` is past the end). */
export function srcAt(segs: Seg[], t: number): number {
  let out = 0;
  for (const g of segs) {
    const d = segDur(g);
    if (t < out + d) return g.a + (t - out) * (g.rate ?? 1);
    out += d;
  }
  return segs[segs.length - 1].b;
}

/**
 * True if part `i` carries straight on from part `i - 1`: the same clip, from the very moment the last one stopped. The
 * picture is then one continuous shot with new words on it, so it does not fade or lift between the two.
 */
export function joined(cut: Cut, i: number): boolean {
  const a = cut.parts[i - 1];
  const b = cut.parts[i];
  if (!a || !b || a.kind !== 'shot' || b.kind !== 'shot' || a.clip !== b.clip) return false;
  return Math.abs(a.segs[a.segs.length - 1].b - b.segs[0].a) < 1e-6;
}

/** Start frame of each part, and the total, given the overlaps (none where two parts are joined). */
export function place(cut: Cut): {starts: number[]; frames: number[]; overlaps: number[]; total: number} {
  const frames = cut.parts.map(partFrames);
  const overlaps = cut.parts.map((_, i) => (i === 0 ? 0 : joined(cut, i) ? 0 : cut.overlap));
  const starts: number[] = [];
  let at = 0;
  for (let i = 0; i < frames.length; i++) {
    at -= overlaps[i];
    starts.push(at);
    at += frames[i];
  }
  return {starts, frames, overlaps, total: at};
}

/** What a title shows in a layout: lines (a one-line title bar joins them) and the sub line if the layout has room for one. */
export function titleLines(spec: TitleSpec, subs: boolean, oneLine: boolean): {lines: string[]; sub?: string} {
  const lines = spec.text.split('\n');
  return {lines: oneLine ? [lines.join(' ')] : lines, sub: subs ? spec.sub : undefined};
}

export const secondsOf = (frames: number) => frames / FPS;

/** Card-entry/exit progress used by the shot wrapper and by the sound cues. */
export const ease = easeInOutCubic;
