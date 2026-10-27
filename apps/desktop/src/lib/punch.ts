/**
 * Punch-in: when the control is on, what it says, and how an outcome
 * reaches the person (#203).
 *
 * A punch-in replaces a selected region of a track with a take recorded
 * while the rest of the mix plays. The backend (`punch_record`) does the
 * playing, the recording and the placing; these are the parts the window
 * decides, kept as plain functions so the rules and the wording can be
 * asserted without mounting the application (`App.tsx` mounts a Tauri
 * surface a unit test cannot render, #273):
 *
 * - the control is on only with a region selected and a track to put it
 *   on, and not while a take or another punch is running;
 * - the take lands late by the delay between the speakers and the
 *   microphone, so with no delay kept for these devices the control says
 *   so and points at Settings → Audio, where it can be measured or typed;
 * - what became of a punch is a value (`applied`, `cancelled`,
 *   `uncalibrated`, `failed`), as `lib/recording.ts` does for a take.
 */

import type { PunchResult, RecordingLatency, ToolProgress } from "./tauri-bridge";
import { formatSeconds } from "./recording";
import { ageText, latencyText } from "./recordingLatency";

/** The line in the popover, and the title of the disabled Start. */
export const UNCALIBRATED_LINE = "Not calibrated — Settings → Audio";

/**
 * How the backend's refusal for "no latency kept" begins
 * (`PunchError::NotCalibrated`, `recorder/src/punch.rs`). The text is the
 * backend's, so this is the one place the window looks inside it, and a test
 * keeps it in step.
 */
export const UNCALIBRATED_MARKER = "No recording latency is kept";

export function isUncalibratedMessage(message: string): boolean {
  return message.includes(UNCALIBRATED_MARKER);
}

/** The default lead-in, in seconds: the backend's default as well. */
export const DEFAULT_PREROLL_SEC = 2;
export const MAX_PREROLL_SEC = 30;

/** A region longer than this drifts enough between the devices to mention. */
export const LONG_REGION_SEC = 60;

export interface PunchGate {
  hasProject: boolean;
  trackCount: number;
  selection: { start: number; end: number } | null;
  /** A take (Record, or a scheduled one) is running. */
  isRecording: boolean;
  /** A punch-in is running. */
  punching: boolean;
}

/**
 * Why the Punch control is off, or `null` when it is on. The first reason
 * that applies, in the order a person fixes them in.
 */
export function punchDisabledReason(g: PunchGate): string | null {
  if (g.punching) return "A punch-in is running";
  if (g.isRecording) return "Stop the recording first";
  if (!g.hasProject || g.trackCount < 1) return "Load a track to punch into";
  if (!g.selection || !(g.selection.end > g.selection.start)) {
    return "Select the region to replace first";
  }
  return null;
}

/** The region as a person reads it: where it is and how long. */
export function regionText(sel: { start: number; end: number }): string {
  const t = (s: number) => {
    const m = Math.floor(s / 60);
    const rest = s - m * 60;
    return `${m}:${rest.toFixed(1).padStart(4, "0")}`;
  };
  return `${t(sel.start)} – ${t(sel.end)} (${(sel.end - sel.start).toFixed(1)} s)`;
}

/**
 * What the latency line says. `undefined` is "not read yet"; the line is
 * the uncalibrated one whenever the devices have no value kept, whether
 * nothing was measured or the machine has no devices to measure.
 */
export function latencyLine(
  status: RecordingLatency | undefined | "failed",
  now: number = Math.floor(Date.now() / 1000),
): { text: string; ready: boolean } {
  if (status === undefined) return { text: "Checking the latency…", ready: false };
  if (status === "failed") return { text: UNCALIBRATED_LINE, ready: false };
  if (status.devices === null) {
    return { text: status.problem ?? "No speakers or microphone found", ready: false };
  }
  if (status.latency === null) return { text: UNCALIBRATED_LINE, ready: false };
  const l = status.latency;
  const when = l.source === "measured" ? ` · measured ${ageText(l.measured_at, now)}` : "";
  return { text: `Latency ${latencyText(l)}${when}`, ready: true };
}

/** The progress strip's label for a `punch_record` report. */
export function punchProgressText(p: Pick<ToolProgress, "phase" | "remaining_sec">): string {
  const left = p.remaining_sec;
  switch (p.phase) {
    case "preroll":
      return left === undefined
        ? "Lead-in…"
        : `Lead-in — recording starts in ${formatSeconds(left)}`;
    case "recording":
      return left === undefined ? "Recording…" : `Recording — ${formatSeconds(left)} left`;
    case "postroll":
      return left === undefined || left <= 0 ? "Finishing…" : `Tail — ${formatSeconds(left)}`;
    default:
      return "Preparing the monitor…";
  }
}

export type PunchOutcome =
  | {
      kind: "applied";
      head: string;
      latencyMs: number;
      source: "measured" | "manual";
    }
  | { kind: "cancelled" }
  /** No delay is kept for these devices: send the person to Settings. */
  | { kind: "uncalibrated"; message: string }
  | { kind: "failed"; message: string };

/**
 * Run a punch-in and say what became of it.
 *
 * A refusal or a failure is passed through as the backend wrote it: it says
 * what to try (a measurement running, a take that lost frames, a project
 * that changed under it, where a kept take is). Cancel is the person's own
 * doing and not a failure.
 */
export async function punchOutcome(run: () => Promise<PunchResult>): Promise<PunchOutcome> {
  let r: PunchResult;
  try {
    r = await run();
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e);
    return isUncalibratedMessage(message)
      ? { kind: "uncalibrated", message }
      : { kind: "failed", message };
  }
  if (r.cancelled) return { kind: "cancelled" };
  if (!r.head) {
    return { kind: "failed", message: "The punch-in finished but reported no new version." };
  }
  return { kind: "applied", head: r.head, latencyMs: r.latency_ms, source: r.latency_source };
}

/** One line for a punch that went in, with the delay it was placed with. */
export function appliedText(o: Extract<PunchOutcome, { kind: "applied" }>): string {
  const ms = Math.round(o.latencyMs * 10) / 10;
  return `Punched in, placed with ${ms} ms of latency (${
    o.source === "manual" ? "entered by hand" : "measured"
  }).`;
}

/** The track's name for the picker, never blank. */
export function trackLabel(name: string, index: number): string {
  return `${index + 1}. ${name.trim() || "Untitled"}`;
}
