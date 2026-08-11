/**
 * SpectrumChart — FFT magnitude curve from `plot_spectrum` (Studio Onyx).
 *
 * Two things about this chart are deliberate:
 *
 * *Log frequency.* The tool returns evenly-spaced FFT bins up to
 * Nyquist. Plotted linearly, everything with a pitch lives in the
 * leftmost few percent and the other 95% is air. A log axis is what
 * makes the picture answer questions about music.
 *
 * *Device pixel ratio.* A canvas sized only in CSS pixels renders
 * soft on every retina display, which reads as a broken chart rather
 * than a blurry one.
 *
 * The canvas is `role="img"` with a label naming the peak, because a
 * canvas is otherwise completely opaque to a screen reader.
 */

import { useEffect, useRef } from "react";

import type { SpectrumPoint } from "../hooks/useAgentStream";

interface SpectrumChartProps {
  points: SpectrumPoint[];
  /** Shown under the chart. Usually the tool's own summary line. */
  caption?: string;
  width?: number;
  height?: number;
}

/** Bottom of the plotted range. Below this is subsonic, and log(0) is
 *  not a number we can put on an axis. */
const MIN_HZ = 20;
const MIN_DB = -120;
const MAX_DB = 0;
const GRID_HZ = [100, 1_000, 10_000];

/** Read a theme token off the live element, falling back to the value
 *  in `styles.css` — `getComputedStyle` returns "" in jsdom, and a
 *  canvas painted with "" is a canvas painted black-on-black. */
function token(el: Element, name: string, fallback: string): string {
  const v = getComputedStyle(el).getPropertyValue(name).trim();
  return v || fallback;
}

function formatHz(hz: number): string {
  return hz >= 1000 ? `${(hz / 1000).toFixed(hz >= 10_000 ? 0 : 1)}k` : `${Math.round(hz)}`;
}

/** The loudest bin, which is the one thing worth saying out loud. */
function peakOf(points: SpectrumPoint[]): SpectrumPoint | null {
  let best: SpectrumPoint | null = null;
  for (const p of points) {
    if (!best || p.db > best.db) best = p;
  }
  return best;
}

export function SpectrumChart({
  points,
  caption,
  width = 380,
  height = 160,
}: SpectrumChartProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    // Back the canvas with real device pixels, then work in CSS pixels.
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

    const surface = token(canvas, "--surface-elev", "#11131a");
    const grid = token(canvas, "--border", "#20232f");
    const faint = token(canvas, "--text-faint", "#5e6373");
    const accent = token(canvas, "--accent", "#ff8a3d");

    ctx.clearRect(0, 0, width, height);
    ctx.fillStyle = surface;
    ctx.fillRect(0, 0, width, height);

    const maxHz = Math.max(points[points.length - 1]?.hz ?? 22_050, MIN_HZ * 2);
    const logMin = Math.log10(MIN_HZ);
    const logSpan = Math.log10(maxHz) - logMin;
    const xOf = (hz: number) => ((Math.log10(hz) - logMin) / logSpan) * width;
    const yOf = (db: number) =>
      height - ((db - MIN_DB) / (MAX_DB - MIN_DB)) * height;

    // Decade grid, so the axis is readable without a legend.
    ctx.strokeStyle = grid;
    ctx.lineWidth = 1;
    ctx.fillStyle = faint;
    ctx.font = "10px ui-monospace, monospace";
    for (const hz of GRID_HZ) {
      if (hz >= maxHz) continue;
      const x = Math.round(xOf(hz)) + 0.5;
      ctx.beginPath();
      ctx.moveTo(x, 0);
      ctx.lineTo(x, height - 12);
      ctx.stroke();
      ctx.fillText(formatHz(hz), x + 3, height - 3);
    }
    for (const db of [-30, -60, -90]) {
      const y = Math.round(yOf(db)) + 0.5;
      ctx.beginPath();
      ctx.moveTo(0, y);
      ctx.lineTo(width, y);
      ctx.stroke();
    }
    ctx.fillText("0 dB", 2, 10);

    if (points.length === 0) {
      ctx.fillStyle = faint;
      ctx.fillText("no spectrum data", 8, height / 2);
      return;
    }

    ctx.strokeStyle = accent;
    ctx.lineWidth = 1.5;
    ctx.lineJoin = "round";
    ctx.beginPath();
    let started = false;
    for (const p of points) {
      // Bin 0 is DC and everything under 20 Hz is off the left edge.
      if (p.hz < MIN_HZ) continue;
      const x = xOf(p.hz);
      const y = yOf(Math.max(p.db, MIN_DB));
      if (started) ctx.lineTo(x, y);
      else {
        ctx.moveTo(x, y);
        started = true;
      }
    }
    ctx.stroke();
  }, [points, width, height]);

  const peak = peakOf(points);
  const label = peak
    ? `Frequency spectrum, loudest at ${formatHz(peak.hz)}Hz`
    : "Frequency spectrum, no data";

  return (
    <figure data-testid="spectrum-chart" className="m-0 flex flex-col gap-1">
      <canvas
        ref={canvasRef}
        role="img"
        aria-label={label}
        style={{ width, height }}
        className="rounded-md border border-[var(--border)]"
      />
      {caption ? (
        <figcaption
          data-testid="spectrum-caption"
          className="text-[10px] text-[var(--text-faint)]"
        >
          {caption}
        </figcaption>
      ) : null}
    </figure>
  );
}
