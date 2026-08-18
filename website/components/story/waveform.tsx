"use client";

/**
 * The waveforms the story is told with.
 *
 * Drawn as a single SVG `path` per lane rather than as N `div` bars,
 * for two reasons. A path can be *drawn* (DrawSVG animates the stroke
 * in) and it can be *morphed* — the same path data reshaped into the
 * post-edit version, which is exactly what "the silence came out and
 * the gap closed" looks like. Sixty separate elements can do neither.
 *
 * Every shape is generated from a fixed arithmetic sequence, never
 * `Math.random`, so the server and the client produce identical `d`
 * attributes and React never reports a hydration mismatch.
 */

/** Deterministic pseudo-random in [0, 1). */
function seeded(i: number, seed: number) {
  return ((Math.sin(i * 127.1 + seed * 311.7) * 43758.5453) % 1 + 1) / 2;
}

export const WAVE_W = 1000;
export const WAVE_H = 120;
const MID = WAVE_H / 2;

/**
 * A waveform outline as one closed path.
 *
 * `gaps` are ranges (in 0–1 of the width) forced to near-silence — the
 * dead air a `truncate_silence` pass would find.
 *
 * `bars` is fixed across every variant on purpose: MorphSVG can only
 * tween between paths cheaply when they share a point count, and a
 * mismatch makes it fall back to a much slower re-parameterisation.
 */
export function wavePath(
  seed: number,
  opts: { bars?: number; gaps?: Array<[number, number]>; scale?: number } = {},
): string {
  const bars = opts.bars ?? 120;
  const gaps = opts.gaps ?? [];
  const scale = opts.scale ?? 1;
  const step = WAVE_W / bars;

  const top: string[] = [];
  const bottom: string[] = [];
  for (let i = 0; i < bars; i++) {
    const x = i * step;
    const t = i / bars;
    const silent = gaps.some(([a, b]) => t >= a && t < b);
    const amp = silent ? 1.2 : (6 + seeded(i, seed) * 46) * scale;
    top.push(`${x.toFixed(2)},${(MID - amp).toFixed(2)}`);
    bottom.unshift(`${x.toFixed(2)},${(MID + amp).toFixed(2)}`);
  }
  return `M${top.join(" L")} L${bottom.join(" L")} Z`;
}

/**
 * The same waveform with the gaps *removed* and the tail slid left —
 * what the track looks like after the cut, not just muted.
 *
 * Built by walking the original bars, skipping the ones inside a gap,
 * and re-spacing what survives across the full width. The point count
 * is padded back to `bars` by repeating the last column, so the morph
 * target still matches the source point-for-point.
 */
export function waveClosedPath(
  seed: number,
  gaps: Array<[number, number]>,
  opts: { bars?: number; scale?: number } = {},
): string {
  const bars = opts.bars ?? 120;
  const scale = opts.scale ?? 1;

  const kept: number[] = [];
  for (let i = 0; i < bars; i++) {
    const t = i / bars;
    if (gaps.some(([a, b]) => t >= a && t < b)) continue;
    kept.push((6 + seeded(i, seed) * 46) * scale);
  }
  while (kept.length < bars) kept.push(kept[kept.length - 1] ?? 1.2);

  const step = WAVE_W / bars;
  const top: string[] = [];
  const bottom: string[] = [];
  kept.slice(0, bars).forEach((amp, i) => {
    const x = i * step;
    top.push(`${x.toFixed(2)},${(MID - amp).toFixed(2)}`);
    bottom.unshift(`${x.toFixed(2)},${(MID + amp).toFixed(2)}`);
  });
  return `M${top.join(" L")} L${bottom.join(" L")} Z`;
}

/** A flat line — the "nothing loaded yet" state the story opens on. */
export function flatPath(bars = 120): string {
  const step = WAVE_W / bars;
  const top: string[] = [];
  const bottom: string[] = [];
  for (let i = 0; i < bars; i++) {
    const x = i * step;
    top.push(`${x.toFixed(2)},${(MID - 0.6).toFixed(2)}`);
    bottom.unshift(`${x.toFixed(2)},${(MID + 0.6).toFixed(2)}`);
  }
  return `M${top.join(" L")} L${bottom.join(" L")} Z`;
}

/** The ducking curve drawn over the music lane in the fourth scene. */
export function duckPath(passages: Array<[number, number]>): string {
  const top = 18;
  const low = WAVE_H - 26;
  const pts: Array<[number, number]> = [[0, top]];
  for (const [a, b] of passages) {
    const x0 = a * WAVE_W;
    const x1 = b * WAVE_W;
    pts.push([x0 - 34, top], [x0, low], [x1, low], [x1 + 46, top]);
  }
  pts.push([WAVE_W, top]);
  return pts.map(([x, y], i) => `${i ? "L" : "M"}${x.toFixed(1)},${y.toFixed(1)}`).join(" ");
}
