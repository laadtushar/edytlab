// One brand module for every composition: the website's colours (generated from website/app/globals.css), Inter,
// the words the edit repeats, and small helpers. No component draws a colour or a claim that is not from here.
import {TOKENS} from './tokens.generated.ts';

export const C = TOKENS;

export const FONT = "'Inter', system-ui, -apple-system, 'Segoe UI', Roboto, sans-serif";

export const hexToRgb = (hex: string): [number, number, number] => {
  const n = parseInt(hex.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
};

export const rgba = (hex: string, a: number): string => {
  const [r, g, b] = hexToRgb(hex);
  return `rgba(${r}, ${g}, ${b}, ${a})`;
};

/** The site's `.gradient-text`: light violet to magenta-violet. */
export const GRADIENT = `linear-gradient(135deg, ${C.gradient[0]} 0%, ${C.gradient[1]} 100%)`;

/** The site's `.glow` shadow, scaled. */
export const glow = (k = 1): string => `0 0 ${80 * k}px -${20 * k}px ${rgba(C.aurora[0], 0.4)}, 0 0 ${30 * k}px -${10 * k}px ${rgba(C.aurora[0], 0.3)}`;

/**
 * The note on every shot of app footage (the footage README makes it binding): the app is the released v0.7.0 build and the
 * footage was recorded live. Where the assistant is in the shot it is real Claude, through the app's own provider; the shots
 * that use no assistant (faders, Settings, the mix, b-roll) say only that they were recorded live.
 */
export const NOTE_CLAUDE = 'v0.7.0 · real app · real Claude (Sonnet 5.5)';
export const NOTE_LIVE = 'v0.7.0 · real app · recorded live';

/** The end card's words. */
export const END = {
  wordmark: 'edytlab',
  version: 'v0.7.0',
  url: 'edytlab.com',
  free: 'Free and open source (MIT)',
  platforms: 'macOS · Windows · Linux',
  unsigned: 'Unsigned installers',
} as const;

export const WEIGHTS = [500, 600, 700] as const;

let ctx: CanvasRenderingContext2D | null = null;
/** Width in px of `text` set in Inter at `weight` and `size` (needs the font loaded: FontGate). */
export function measureText(text: string, weight: number, size: number, letterSpacingEm = 0): number {
  if (typeof document === 'undefined') return text.length * size * 0.58;
  ctx ??= document.createElement('canvas').getContext('2d');
  if (!ctx) return text.length * size * 0.58;
  ctx.font = `${weight} ${size}px Inter`;
  return ctx.measureText(text).width + letterSpacingEm * size * text.length;
}

export const clamp01 = (v: number) => Math.min(1, Math.max(0, v));
/** Decelerating ease, for things arriving. */
export const easeOut = (p: number) => 1 - Math.pow(1 - clamp01(p), 3);
/** Accelerating ease, for things leaving. */
export const easeIn = (p: number) => Math.pow(clamp01(p), 2.2);
