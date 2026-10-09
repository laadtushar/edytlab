/**
 * The waveform motif on a post's social image, as plain functions so a
 * test can check that posts get different ones without rendering a PNG.
 */

/** A small stable number from a string, so a post's motif is its own and never changes. */
export function seedOf(text: string): number {
  let h = 2166136261;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return (h >>> 0) % 1000;
}

function rand(seed: number, i: number): number {
  const x = Math.sin(seed * 127.1 + i * 311.7) * 43758.5453;
  return x - Math.floor(x);
}

/** A waveform as one stroked path: bars whose envelope peaks where the seed says. */
export function motifPath(seed: number, width = 1200, cy = 80, amp = 66): string {
  const step = 12;
  const count = Math.floor((width - 80) / step);
  const centre = 0.25 + 0.5 * rand(seed, 99);
  let d = "";
  for (let i = 0; i <= count; i++) {
    const t = i / count;
    const envelope = Math.exp(-(((t - centre) / 0.28) ** 2)) * 0.85 + 0.15;
    const h = Math.max(2, amp * envelope * (0.25 + 0.75 * rand(seed, i)));
    const x = 40 + i * step;
    d += `M${x} ${Math.round((cy - h) * 10) / 10}V${Math.round((cy + h) * 10) / 10}`;
  }
  return d;
}
