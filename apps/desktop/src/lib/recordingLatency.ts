/**
 * What Settings → Audio says about the recording latency, and how a
 * measurement's outcome reaches the person (#203).
 *
 * The latency is how late the microphone hears what this computer plays:
 * output buffers, converters, the speaker, the air, input buffers. It is
 * measured with test sweeps, and kept for the pair of devices it was
 * measured on. These are pure so the wording and the outcome mapping can be
 * asserted without mounting the row.
 */

import type {
  CalibrationResult,
  RecordingDevices,
  StoredLatency,
  ToolProgress,
} from "./tauri-bridge";

/** The one line of setup, shown whenever the row can measure. */
export const SETUP_LINE =
  "Put the speakers close to the microphone (or hold the headphones against it), turn the volume up and keep the room quiet.";

/** What the row is for, in a sentence. */
export const ROW_PURPOSE =
  "How late the microphone hears what this computer plays, measured with test tones. It is kept for this pair of speakers and microphone.";

/** A measured delay, to a tenth of a millisecond, with its steadiness. */
export function latencyText(l: StoredLatency): string {
  // Rounding first, so a measurement of -0.02 ms is "0.0", not "-0.0".
  const ms = Math.round(l.latency_ms * 10) / 10 || 0;
  const shown = l.source === "manual" ? `${trimmed(ms)} ms` : `${ms.toFixed(1)} ms`;
  if (l.source === "manual") return `${shown} (entered by hand)`;
  return l.spread_ms === undefined
    ? shown
    : `${shown} (steady to ${l.spread_ms.toFixed(1)} ms)`;
}

function trimmed(n: number): string {
  return Number.isInteger(n) ? String(n) : n.toFixed(1);
}

/** How long ago `measuredAt` was, in words a person reads at a glance. */
export function ageText(measuredAt: number, now: number): string {
  const s = now - measuredAt;
  if (s < 60) return "just now";
  const plural = (n: number, unit: string) => `${n} ${unit}${n === 1 ? "" : "s"} ago`;
  if (s < 3_600) return plural(Math.floor(s / 60), "minute");
  if (s < 86_400) return plural(Math.floor(s / 3_600), "hour");
  if (s < 30 * 86_400) return plural(Math.floor(s / 86_400), "day");
  return `on ${new Date(measuredAt * 1000).toISOString().slice(0, 10)}`;
}

/** 44100 as "44.1 kHz", 48000 as "48 kHz". */
export function rateText(hz: number): string {
  return `${parseFloat((hz / 1000).toFixed(2))} kHz`;
}

/** Which devices a latency is for. */
export function devicesText(d: RecordingDevices): string {
  return `Speakers: ${d.output} · Microphone: ${d.input} · ${d.host}`;
}

/** The line while the sweeps play. */
export function measuringText(p: ToolProgress | undefined): string {
  return p?.run !== undefined && p.runs !== undefined
    ? `Measuring… run ${p.run} of ${p.runs}`
    : "Measuring…";
}

export type CalibrateOutcome =
  | { kind: "measured"; latency: StoredLatency; devices: RecordingDevices }
  | { kind: "cancelled" }
  | { kind: "refused"; message: string };

/**
 * Run a measurement and say what became of it.
 *
 * A refusal is an outcome the person acts on (the sweeps were not heard,
 * the room was loud), so its text is passed through as the backend wrote
 * it, in place, and not sent to the error banner. Cancel is not a failure.
 */
export async function calibrateOutcome(
  run: () => Promise<CalibrationResult>,
): Promise<CalibrateOutcome> {
  try {
    const r = await run();
    if (r.cancelled) return { kind: "cancelled" };
    return { kind: "measured", latency: r.latency, devices: r.devices };
  } catch (e) {
    return { kind: "refused", message: e instanceof Error ? e.message : String(e) };
  }
}

/** What the manual field is for, in a sentence. */
export const MANUAL_PURPOSE =
  "Or type the delay yourself, in milliseconds: it counts exactly as a measured one. Use it where you cannot make noise in the room, or when you measured it another way. Whichever you set last is the one used.";

/** The most a typed delay may be, as the backend checks it. */
export const MANUAL_MAX_MS = 2_000;

export type ManualLatency = { ok: true; ms: number } | { ok: false; message: string };

/**
 * A delay typed in the field. The backend checks it again (0 to 2000 ms,
 * `put_manual`); checking here turns a round trip into a reason beside the
 * field. Empty is not a number, not zero.
 */
export function parseManualLatency(text: string): ManualLatency {
  const t = text.trim().replace(",", ".");
  if (t === "") return { ok: false, message: "Type a delay in milliseconds." };
  const ms = Number(t);
  if (!Number.isFinite(ms)) return { ok: false, message: `"${text.trim()}" is not a number.` };
  if (ms < 0 || ms > MANUAL_MAX_MS) {
    return { ok: false, message: `A delay is from 0 to ${MANUAL_MAX_MS} ms.` };
  }
  return { ok: true, ms };
}
