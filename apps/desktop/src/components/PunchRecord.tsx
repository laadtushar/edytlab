/**
 * PunchRecord — replace the selected region of a track with a new take,
 * while the rest of the mix plays (#203).
 *
 * The control sits next to the scheduled-recording one. It is on only with
 * a region selected and a track to put it on, and not while a take or
 * another punch is running (`punchDisabledReason`). Its popover names the
 * region, asks for the track and the lead-in, and says what the take will be
 * placed with: the delay measured (or typed) for these speakers and this
 * microphone. With none, it says "Not calibrated" and points at
 * Settings → Audio, and Start stays off: a take placed with a guess lands
 * late by exactly that, which is the failure this exists to prevent. The
 * backend refuses too; this is the sentence before the round trip.
 *
 * The recording itself is the application's (`onPunch`): it pauses the
 * transport, calls `punch_record` and adopts the new head. The countdown and
 * its Cancel are the progress strip's, on the channel the backend reports
 * to; this component's job ends when the punch is started.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import {
  DEFAULT_PREROLL_SEC,
  LONG_REGION_SEC,
  MAX_PREROLL_SEC,
  UNCALIBRATED_LINE,
  latencyLine,
  punchDisabledReason,
  regionText,
  trackLabel,
  type PunchGate,
} from "../lib/punch";
import { getRecordingLatency, type RecordingLatency } from "../lib/tauri-bridge";

export interface PunchRecordProps extends PunchGate {
  /** The tracks to choose from, in session order. */
  tracks: { name: string }[];
  /** Start the punch-in. Seconds; the region is the selection. */
  onPunch: (request: { track: number; prerollSec: number }) => void;
  /** Open Settings → Audio, where a delay is measured or typed. */
  onOpenAudioSettings?: () => void;
}

/** A lead-in as typed: empty is the default, anything else a number. */
function parsePreroll(text: string): number | null {
  if (text.trim() === "") return DEFAULT_PREROLL_SEC;
  const n = Number(text.trim().replace(",", "."));
  return Number.isFinite(n) && n >= 0 && n <= MAX_PREROLL_SEC ? n : null;
}

export function PunchRecord({
  tracks,
  onPunch,
  onOpenAudioSettings,
  ...gate
}: PunchRecordProps) {
  const [open, setOpen] = useState(false);
  const [track, setTrack] = useState(0);
  const [preroll, setPreroll] = useState(String(DEFAULT_PREROLL_SEC));
  const [latency, setLatency] = useState<RecordingLatency | undefined | "failed">(undefined);
  const rootRef = useRef<HTMLDivElement>(null);

  const reason = punchDisabledReason(gate);

  // The delay belongs to the pair of devices that are the defaults *now*, so
  // it is read each time the popover opens, not once.
  useEffect(() => {
    if (!open) return;
    let live = true;
    setLatency(undefined);
    getRecordingLatency().then(
      (s) => {
        if (live) setLatency(s);
      },
      () => {
        if (live) setLatency("failed");
      },
    );
    return () => {
      live = false;
    };
  }, [open]);

  // A track removed while the popover is open leaves the picker on one that
  // exists.
  const picked = Math.min(track, Math.max(0, tracks.length - 1));

  // Escape closes, and a click outside, like every other transient surface.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    const onDown = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("mousedown", onDown);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("mousedown", onDown);
    };
  }, [open]);

  // Nothing to punch into any more (the selection was cleared, a take
  // started): the popover goes, as its button is off.
  useEffect(() => {
    if (reason !== null) setOpen(false);
  }, [reason]);

  const line = latencyLine(latency);
  const prerollSec = parsePreroll(preroll);
  const region = gate.selection;
  const long = region !== null && region.end - region.start > LONG_REGION_SEC;
  const canStart = reason === null && line.ready && prerollSec !== null;

  const start = useCallback(() => {
    if (!canStart || prerollSec === null) return;
    onPunch({ track: picked, prerollSec });
    setOpen(false);
  }, [canStart, onPunch, picked, prerollSec]);

  return (
    <div ref={rootRef} className="relative">
      <button
        type="button"
        data-testid="punch-btn"
        aria-label="Punch in"
        aria-expanded={open}
        title={
          reason ??
          "Replace the selected region with a new take, hearing the rest of the mix"
        }
        disabled={reason !== null}
        onClick={() => setOpen((v) => !v)}
        className="
          btn-secondary
          rounded-md px-[7px] py-[3px] text-sm font-medium
          transition
          disabled:cursor-not-allowed disabled:opacity-40
        "
      >
        Punch
      </button>

      {open && region ? (
        <div
          data-testid="punch-panel"
          className="
            absolute right-0 top-full z-50 mt-1 w-72
            rounded-md border border-[var(--border-strong)]
            bg-[var(--surface-elev)] p-3
            shadow-(--shadow-pop)
          "
        >
          <p className="mb-2 font-mono text-[9px] uppercase tracking-[0.18em] text-[var(--text-faint)]">
            Punch in
          </p>

          <p
            data-testid="punch-region"
            className="mb-2 font-mono text-[11px] text-[var(--text-dim)]"
          >
            {regionText(region)}
          </p>

          <div className="flex flex-col gap-2">
            <label className="flex items-center justify-between gap-2">
              <span className="text-[11px] text-[var(--text-dim)]">Track</span>
              <select
                data-testid="punch-track"
                value={picked}
                onChange={(e) => setTrack(Number(e.target.value))}
                className="field min-w-0 max-w-[10rem] flex-1 px-1.5 py-1 text-xs"
              >
                {tracks.map((t, i) => (
                  <option key={i} value={i}>
                    {trackLabel(t.name, i)}
                  </option>
                ))}
              </select>
            </label>
            <label className="flex items-center justify-between gap-2">
              <span className="text-[11px] text-[var(--text-dim)]">Lead-in</span>
              <span className="flex items-center gap-1">
                <input
                  type="number"
                  min={0}
                  max={MAX_PREROLL_SEC}
                  step={0.5}
                  inputMode="decimal"
                  value={preroll}
                  onChange={(e) => setPreroll(e.target.value)}
                  aria-label="Lead-in seconds"
                  data-testid="punch-preroll"
                  className="field w-14 px-1.5 py-1 text-right text-xs"
                />
                <span className="text-[10px] text-[var(--text-faint)]">s</span>
              </span>
            </label>
          </div>

          <p
            data-testid="punch-latency"
            aria-live="polite"
            className={
              "mt-2 break-words font-mono text-[11px] " +
              (line.ready ? "text-[var(--text-dim)]" : "text-[var(--warning)]")
            }
          >
            {line.text}
          </p>
          {!line.ready && latency !== undefined && onOpenAudioSettings ? (
            <button
              type="button"
              data-testid="punch-open-settings"
              onClick={() => {
                setOpen(false);
                onOpenAudioSettings();
              }}
              className="btn-secondary mt-1 rounded px-2 py-0.5 text-[11px] transition"
            >
              Open Settings → Audio
            </button>
          ) : null}

          <p className="mt-2 text-[11px] leading-snug text-[var(--text-faint)]">
            {line.text === UNCALIBRATED_LINE
              ? "Measure the delay between the speakers and the microphone, or type it, so the take lands where it was played. "
              : ""}
            You hear the whole mix from the lead-in, with this track silent inside the region and
            heard again after it. Headphones keep the mix out of the microphone.
          </p>
          {long ? (
            <p
              data-testid="punch-long"
              className="mt-1 text-[11px] leading-snug text-[var(--text-faint)]"
            >
              A region this long can drift by a few milliseconds between the speakers and the
              microphone. Fine for a line, less so for a verse.
            </p>
          ) : null}
          {prerollSec === null ? (
            <p role="alert" className="mt-1 text-[11px] text-[var(--warning)]">
              The lead-in is from 0 to {MAX_PREROLL_SEC} seconds.
            </p>
          ) : null}

          <button
            type="button"
            data-testid="punch-start"
            disabled={!canStart}
            onClick={start}
            title={line.ready ? undefined : line.text}
            className="
              btn-primary
              mt-3 w-full px-3 py-1.5
              text-sm
              transition
            "
          >
            Start
          </button>
        </div>
      ) : null}
    </div>
  );
}
