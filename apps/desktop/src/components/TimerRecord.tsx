/**
 * TimerRecord — arm an unattended take (#225 §4).
 *
 * `timer_record` shipped in #203 §2 and had no caller. The whole point
 * of it is starting and stopping when nobody is at the machine, which
 * is precisely the thing you could not ask for: the only way in was to
 * describe it to the agent in a sentence, and that needs a working LLM
 * (#321) to do something entirely deterministic.
 *
 * Two independent halves, because "start in ten minutes" and "record
 * for thirty seconds" are different requests and either alone is
 * useful. The backend rejects a schedule with neither, so this does
 * too — before the round-trip, with a reason.
 *
 * The countdown is not here. It arrives on the tool-progress channel
 * and `ToolProgressBar` renders it, along with the Cancel that stops
 * a take mid-countdown. This component's job ends when the take is
 * armed.
 */

import { useCallback, useEffect, useRef, useState } from "react";

export interface TimerRecordProps {
  /** Disabled while a take is already running. */
  busy?: boolean;
  /** Arm it. Seconds; `undefined` means that half is not set. */
  onArm: (schedule: {
    startAfterSec?: number;
    durationSec?: number;
  }) => void;
}

/**
 * Minutes and seconds as one number of seconds.
 *
 * Both fields are optional and empty means "not set", which is not the
 * same as zero — a zero delay is a legitimate "start now, stop after
 * N", and treating a blank as 0 would silently arm that.
 */
function toSeconds(min: string, sec: string): number | undefined {
  const m = min.trim();
  const s = sec.trim();
  if (!m && !s) return undefined;
  const mv = m ? Number(m) : 0;
  const sv = s ? Number(s) : 0;
  if (!Number.isFinite(mv) || !Number.isFinite(sv)) return undefined;
  if (mv < 0 || sv < 0) return undefined;
  return mv * 60 + sv;
}

export function TimerRecord({ busy, onArm }: TimerRecordProps) {
  const [open, setOpen] = useState(false);
  const [startMin, setStartMin] = useState("");
  const [startSec, setStartSec] = useState("");
  const [forMin, setForMin] = useState("");
  const [forSec, setForSec] = useState("");
  const rootRef = useRef<HTMLDivElement>(null);

  const startAfterSec = toSeconds(startMin, startSec);
  const durationSec = toSeconds(forMin, forSec);
  // Mirrors `Schedule::is_armed` on the Rust side, which returns the
  // same refusal. Checking here turns a round-trip and an error banner
  // into a disabled button with the reason next to it.
  const armed = startAfterSec !== undefined || durationSec !== undefined;

  // Escape closes, like every other transient surface in the app.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open]);

  // A click outside closes it. Without this the panel outlives the
  // button that opened it and sits over the timeline.
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [open]);

  const arm = useCallback(() => {
    if (!armed) return;
    onArm({ startAfterSec, durationSec });
    setOpen(false);
  }, [armed, onArm, startAfterSec, durationSec]);

  const field = (
    label: string,
    minV: string,
    setMin: (v: string) => void,
    secV: string,
    setSec: (v: string) => void,
    testid: string,
  ) => (
    <div className="flex items-center justify-between gap-2">
      <span className="text-[11px] text-[var(--text-dim)]">{label}</span>
      <span className="flex items-center gap-1">
        <input
          type="number"
          min={0}
          inputMode="numeric"
          value={minV}
          onChange={(e) => setMin(e.target.value)}
          aria-label={`${label} minutes`}
          data-testid={`${testid}-min`}
          className="w-12 rounded border border-[var(--border-strong)] bg-[var(--surface)] px-1.5 py-1 text-right text-xs text-[var(--text)]"
        />
        <span className="text-[10px] text-[var(--text-faint)]">m</span>
        <input
          type="number"
          min={0}
          inputMode="numeric"
          value={secV}
          onChange={(e) => setSec(e.target.value)}
          aria-label={`${label} seconds`}
          data-testid={`${testid}-sec`}
          className="w-12 rounded border border-[var(--border-strong)] bg-[var(--surface)] px-1.5 py-1 text-right text-xs text-[var(--text)]"
        />
        <span className="text-[10px] text-[var(--text-faint)]">s</span>
      </span>
    </div>
  );

  return (
    <div ref={rootRef} className="relative">
      <button
        type="button"
        data-testid="timer-record-btn"
        aria-label="Scheduled recording"
        aria-expanded={open}
        title="Record unattended — start after a delay, stop after a duration"
        disabled={busy}
        onClick={() => setOpen((v) => !v)}
        className="
          rounded px-2 py-1 text-sm font-medium
          bg-neutral-700 text-neutral-300
          transition
          hover:bg-neutral-600
          disabled:cursor-not-allowed disabled:opacity-40
        "
      >
        ⏱
      </button>

      {open ? (
        <div
          data-testid="timer-record-panel"
          className="
            absolute right-0 top-full z-50 mt-1 w-64
            rounded-md border border-[var(--border-strong)]
            bg-[var(--surface-elev)] p-3
            shadow-[0_10px_30px_-10px_rgba(0,0,0,0.6)]
          "
        >
          <p className="mb-2 font-mono text-[9px] uppercase tracking-[0.18em] text-[var(--text-faint)]">
            Scheduled recording
          </p>

          <div className="flex flex-col gap-2">
            {field("Start after", startMin, setStartMin, startSec, setStartSec, "timer-start")}
            {field("Record for", forMin, setForMin, forSec, setForSec, "timer-duration")}
          </div>

          <p
            data-testid="timer-record-hint"
            className="mt-2 text-[11px] leading-snug text-[var(--text-faint)]"
          >
            {armed
              ? durationSec === undefined
                ? "Starts on the timer and records until you press Stop."
                : startAfterSec === undefined
                  ? "Starts now and stops on its own."
                  : "Starts on the timer and stops on its own."
              : "Set a delay, a duration, or both."}
          </p>

          <button
            type="button"
            data-testid="timer-record-arm"
            disabled={!armed}
            onClick={arm}
            className="
              mt-3 w-full rounded-md
              bg-[var(--accent)] px-3 py-1.5
              text-sm font-medium text-[var(--onyx-0,#07080b)]
              transition
              hover:bg-[#ffa05f]
              disabled:cursor-not-allowed disabled:opacity-40
            "
          >
            Arm
          </button>
        </div>
      ) : null}
    </div>
  );
}
