/**
 * What the app plays, and when.
 *
 * The page's audio is an HTML media element playing the rendered mix
 * (`render_preview`'s file). It plays in real time even with no sound card.
 * The in-page kit (`kit.ts`) records every play, pause and end on any media
 * element, on the page's clock, so a shot can write the audio the app played as a WAV that
 * lines up with the footage: silence until the mix started, the mix from
 * where it started, silence where it paused.
 */

import { writeFileSync } from "node:fs";
import { join } from "node:path";

import type { App } from "../../../../apps/desktop/e2e/fixtures";

import { FOOTAGE_DIR } from "./paths";
import { RATE, wavBytes } from "./music";

type Page = App["page"];

export interface MediaEvent {
  type: string;
  /** Epoch ms. */
  at: number;
  /** The element's position in its file, in seconds. */
  position: number;
}

export const readMedia = (page: Page): Promise<MediaEvent[]> =>
  page.evaluate(() => (window as unknown as { __PROMO_MEDIA__: MediaEvent[] }).__PROMO_MEDIA__.slice());

export interface PlayedSpan {
  /** Seconds into the footage the sound starts. */
  footageStart: number;
  /** Seconds into the footage it stops. */
  footageEnd: number;
  /** Where in the mix file it started, in seconds. */
  mixStart: number;
}

/** The spans of sound in `events`: from each `playing` to the next pause, end or unload. */
export function playedSpans(events: MediaEvent[], originEpochMs: number, endEpochMs: number): PlayedSpan[] {
  const spans: PlayedSpan[] = [];
  let open: MediaEvent | null = null;
  const close = (e: MediaEvent | null, at: number) => {
    if (!open) return;
    spans.push({
      footageStart: (open.at - originEpochMs) / 1000,
      footageEnd: (at - originEpochMs) / 1000,
      mixStart: open.position,
    });
    open = null;
    void e;
  };
  for (const e of events) {
    if (e.type === "playing") {
      if (open) close(e, e.at);
      open = e;
    } else if (e.type === "pause" || e.type === "ended" || e.type === "emptied") {
      close(e, e.at);
    }
  }
  close(null, endEpochMs);
  return spans.filter((s) => s.footageEnd > 0 && s.footageStart < (endEpochMs - originEpochMs) / 1000);
}

/**
 * The audio of a shot: one sample array as long as the footage, with the mix
 * placed where it played. `mixFor(span)` names which mix a span played, since
 * a shot can play more than one (before and after an edit).
 */
export function shotAudio(
  spans: PlayedSpan[],
  mixFor: (span: PlayedSpan) => Float32Array,
  footageSeconds: number,
): Float32Array {
  const out = new Float32Array(Math.ceil(footageSeconds * RATE));
  for (const span of spans) {
    const mix = mixFor(span);
    const dst0 = Math.round(Math.max(0, span.footageStart) * RATE);
    const src0 = Math.round((span.mixStart + Math.max(0, -span.footageStart)) * RATE);
    const n = Math.round((span.footageEnd - Math.max(0, span.footageStart)) * RATE);
    for (let i = 0; i < n && dst0 + i < out.length && src0 + i < mix.length; i++) out[dst0 + i] = mix[src0 + i];
  }
  return out;
}

/** Write `footage/<name>.wav`. */
export function writeShotWav(name: string, audio: Float32Array): string {
  const path = join(FOOTAGE_DIR, `${name}.wav`);
  writeFileSync(path, wavBytes(audio, RATE));
  return path;
}
