// The virtual camera: a crop and zoom over a footage clip. Pure arithmetic (no React).
//
// The footage is a 1600x1000 CSS-pixel window captured at 2x (3200x2000). A camera is described by a *focus box* in
// CSS pixels of that window: "this much of the app must be in view". Each layout's card has its own aspect ratio, so
// the box is fitted into the card (contain), never zoomed past `MAX_SCALE` card px per CSS px (the file has 2 px per
// CSS px; 3 is a 1.5x upscale and still crisp), never zoomed out past the window (cover), and kept inside the window.
// One set of focus boxes therefore frames every layout.
import type {Box} from './layout.ts';

export const WIN = {w: 1600, h: 1000};
export const DEVICE_SCALE = 2;
export const MAX_SCALE = 3.0;

/** A camera position: the window point at the card's centre, and card px per window CSS px. */
export interface Cam {
  cx: number;
  cy: number;
  s: number;
}

/** The camera is at `box` at time `t` (seconds), having started moving `move` seconds before (default 0.9). */
export interface FocusKF {
  t: number;
  box: Box;
  /** The box to use instead in the wide (16:9) layout, whose card is much wider than tall: it can show more. */
  wide?: Box;
  /** The box to use instead in the square (1:1) layout, whose card is taller for its width than 9:16's. */
  square?: Box;
  move?: number;
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
export const lerp = (a: number, b: number, p: number) => a + (b - a) * p;
export const easeInOutCubic = (p: number) => (p < 0.5 ? 4 * p * p * p : 1 - Math.pow(-2 * p + 2, 3) / 2);

export function resolveCam(box: Box, card: Box): Cam {
  // Never zoomed out past the whole window (`contain`): a card wider than the window's shape shows it with dark margins
  // at the sides, which only a deliberate overview ever asks for.
  const contain = Math.min(card.w / WIN.w, card.h / WIN.h);
  const s = clamp(Math.min(card.w / box.w, card.h / box.h), contain, MAX_SCALE);
  const rw = card.w / s;
  const rh = card.h / s;
  const mid = (v: number, size: number, lim: number) => (size >= lim ? lim / 2 : clamp(v, size / 2, lim - size / 2));
  return {s, cx: mid(box.x + box.w / 2, rw, WIN.w), cy: mid(box.y + box.h / 2, rh, WIN.h)};
}

export function camAt(kfs: FocusKF[], card: Box, t: number, kind: 'portrait' | 'wide' | 'square' = 'portrait'): Cam {
  if (kfs.length === 0) return resolveCam({x: 0, y: 0, w: WIN.w, h: WIN.h}, card);
  const cams = kfs.map((k) => resolveCam((kind === 'wide' ? k.wide : kind === 'square' ? k.square : undefined) ?? k.box, card));
  if (t <= kfs[0].t) return cams[0];
  for (let i = 1; i < kfs.length; i++) {
    if (t <= kfs[i].t) {
      const arrive = kfs[i].t;
      const start = Math.max(kfs[i - 1].t, arrive - (kfs[i].move ?? 0.9));
      if (t <= start) return cams[i - 1];
      const p = easeInOutCubic((t - start) / (arrive - start));
      const a = cams[i - 1];
      const b = cams[i];
      return {cx: lerp(a.cx, b.cx, p), cy: lerp(a.cy, b.cy, p), s: Math.exp(lerp(Math.log(a.s), Math.log(b.s), p))};
    }
  }
  return cams[cams.length - 1];
}

/** A window CSS-pixel rectangle in card-local pixels under camera `cam`. */
export function toCard(cam: Cam, card: Box, r: Box): Box {
  return {
    x: card.w / 2 + (r.x - cam.cx) * cam.s,
    y: card.h / 2 + (r.y - cam.cy) * cam.s,
    w: r.w * cam.s,
    h: r.h * cam.s,
  };
}

/** The CSS transform that puts a 3200x2000 video, origin top-left, under `cam` in a card of `card` size. */
export function videoTransform(cam: Cam, card: Box): string {
  const k = cam.s / DEVICE_SCALE;
  const tx = card.w / 2 - cam.cx * cam.s;
  const ty = card.h / 2 - cam.cy * cam.s;
  return `translate(${tx.toFixed(2)}px, ${ty.toFixed(2)}px) scale(${k.toFixed(5)})`;
}
