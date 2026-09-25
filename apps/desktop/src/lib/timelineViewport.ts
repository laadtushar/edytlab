/**
 * The one horizontal axis every timeline row is drawn on (#344, #348).
 *
 * The ruler, each lane's waveform, the selection overlay, the playhead,
 * the clip chips, the automation curves and the marker flags all map
 * between session seconds and pixels. Each used to do it its own way —
 * each lane over its own audio's length, the ruler over lane 0's
 * scroll, everything else over the whole session — and they agreed only
 * when there was one track, starting at zero, at fit. So the mapping
 * lives here, once, and every row takes the same viewport.
 *
 * A viewport is the session's length, the pixel width of a row's time
 * surface, the density in pixels per second, and the time at the
 * surface's left edge. Everything else is derived.
 */

export interface Viewport {
  /** The session's length, in seconds: the furthest any clip reaches. */
  durationSec: number;
  /** Width of a row's time surface, in CSS pixels. */
  widthPx: number;
  /** Density. At fit, the whole session across `widthPx`. */
  pxPerSec: number;
  /** The session time at the surface's left edge. */
  scrollSec: number;
}

/** A stretch of the session, in seconds. */
export interface TimeSpan {
  start: number;
  end: number;
}

/**
 * The density to draw at: `zoom` when the user chose one, else the one
 * that fits the whole session across the surface. `zoom` is 0 for fit,
 * which is not a density at all.
 */
export function pxPerSecFor(zoom: number, widthPx: number, durationSec: number): number {
  if (zoom > 0) return zoom;
  if (widthPx > 0 && durationSec > 0) return widthPx / durationSec;
  return 0;
}

/** The latest left-edge time that still keeps the surface on the session. */
export function maxScrollSec(v: Pick<Viewport, "durationSec" | "widthPx" | "pxPerSec">): number {
  if (!(v.pxPerSec > 0)) return 0;
  return Math.max(0, v.durationSec - v.widthPx / v.pxPerSec);
}

/** `sec` as a left-edge time this viewport can show. */
export function clampScrollSec(
  sec: number,
  v: Pick<Viewport, "durationSec" | "widthPx" | "pxPerSec">,
): number {
  if (!Number.isFinite(sec)) return 0;
  return Math.min(Math.max(0, sec), maxScrollSec(v));
}

/**
 * The stretch of time the surface spans, left edge to right, or null
 * when nothing can be measured yet — no width, no audio.
 *
 * Not clamped to the session. Zoomed out past fit, the session ends
 * before the surface does, and a row that stretched the session's end
 * to the surface's edge would be drawn on a different axis from every
 * row that did not.
 */
export function visibleSpan(v: Viewport): TimeSpan | null {
  if (!(v.pxPerSec > 0) || !(v.widthPx > 0) || !(v.durationSec > 0)) return null;
  const start = v.scrollSec;
  return { start, end: start + v.widthPx / v.pxPerSec };
}

/** Where session time `sec` falls, in pixels from the surface's left edge. */
export function secToPx(sec: number, v: Viewport): number {
  return (sec - v.scrollSec) * v.pxPerSec;
}

/**
 * The session time under `px` pixels from the surface's left edge,
 * kept on the session.
 */
export function pxToSec(px: number, v: Viewport): number {
  if (!(v.pxPerSec > 0)) return 0;
  return Math.min(Math.max(0, v.scrollSec + px / v.pxPerSec), v.durationSec);
}

/**
 * Where `sec` falls across a surface showing `span`, as a percentage of
 * its width. For rows laid out in percentages; below 0 or above 100 is
 * off screen, and the row clips it.
 */
export function pctOf(sec: number, span: TimeSpan): number {
  const width = span.end - span.start;
  return width > 0 ? ((sec - span.start) / width) * 100 : 0;
}
