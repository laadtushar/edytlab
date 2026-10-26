/**
 * Recording latency in Settings → Audio (#203): how late the microphone
 * hears what this computer plays, measured once per pair of devices.
 *
 * Nothing plays until Measure is pressed. Opening Settings only asks which
 * devices are the defaults and what is kept for them
 * (`get_recording_latency`, which plays and records nothing). The
 * measurement (`calibrate_recording_latency`) takes about ten seconds and
 * makes noise, which is why the setup line is on screen before the button
 * is pressed, not after a refusal.
 *
 * A refusal ("heard 2 of 5 test sweeps…") is shown here, where the person
 * is looking, and leaves the value that was kept alone. Cancel is not an
 * error. The progress it shows is the backend's own `calibrate_latency`
 * report; other jobs on the same channel are not this row's.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import {
  ROW_PURPOSE,
  SETUP_LINE,
  ageText,
  calibrateOutcome,
  devicesText,
  latencyText,
  measuringText,
} from "../lib/recordingLatency";
import {
  calibrateRecordingLatency,
  cancelLongRunningTool,
  getRecordingLatency,
  onToolProgress,
  type RecordingLatency,
  type ToolProgress,
} from "../lib/tauri-bridge";

function messageOf(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export function RecordingLatencyRow() {
  const [status, setStatus] = useState<RecordingLatency | null>(null);
  // The latency could not be read (a file from a newer app).
  const [unreadable, setUnreadable] = useState<string | null>(null);
  const [measuring, setMeasuring] = useState(false);
  const [progress, setProgress] = useState<ToolProgress | undefined>(undefined);
  const [refusal, setRefusal] = useState<string | null>(null);
  const [cancelling, setCancelling] = useState(false);
  const alive = useRef(true);

  const read = useCallback(() => {
    getRecordingLatency().then(
      (s) => {
        if (!alive.current) return;
        setStatus(s);
        setUnreadable(null);
      },
      (e: unknown) => {
        if (alive.current) setUnreadable(messageOf(e));
      },
    );
  }, []);

  useEffect(() => {
    alive.current = true;
    // Per run of the effect, not the shared ref: under StrictMode the
    // first run's listener registers after the second run has set `alive`
    // again, and has to be let go all the same.
    let live = true;
    let unlisten: (() => void) | null = null;

    void onToolProgress((p) => {
      if (live && p.kind === "calibrate_latency") setProgress(p);
    }).then(
      (fn) => {
        if (live) unlisten = fn;
        else fn();
      },
      () => undefined,
    );

    read();
    // The default devices can change while Settings is open (headphones,
    // a USB microphone), and the value belongs to the pair.
    const onFocus = () => read();
    window.addEventListener("focus", onFocus);

    return () => {
      live = false;
      alive.current = false;
      unlisten?.();
      window.removeEventListener("focus", onFocus);
    };
  }, [read]);

  const measure = useCallback(async () => {
    setMeasuring(true);
    setRefusal(null);
    setProgress(undefined);
    const outcome = await calibrateOutcome(calibrateRecordingLatency);
    if (!alive.current) return;
    setMeasuring(false);
    setCancelling(false);
    setProgress(undefined);
    if (outcome.kind === "measured") {
      setStatus({ devices: outcome.devices, problem: null, latency: outcome.latency });
    } else if (outcome.kind === "refused") {
      setRefusal(outcome.message);
    }
  }, []);

  const cancel = useCallback(() => {
    setCancelling(true);
    // `cancel_long_running_tool` answers at once and the run ends at its
    // next checkpoint; the button comes back when `measure` settles.
    void cancelLongRunningTool().catch(() => setCancelling(false));
  }, []);

  const devices = status?.devices ?? null;
  const latency = status?.latency ?? null;

  let value: string;
  if (measuring) value = measuringText(progress);
  else if (unreadable !== null) value = `Could not read the kept latency (${unreadable})`;
  else if (status === null) value = "Checking…";
  else if (devices === null) value = status.problem ?? "No speakers or microphone found";
  else if (latency === null) value = "Not measured yet";
  else value = `${latencyText(latency)} · measured ${ageText(latency.measured_at, Math.floor(Date.now() / 1000))}`;

  return (
    <div data-testid="recording-latency">
      <p className="mb-1.5 font-mono text-[10px] uppercase tracking-[0.18em] text-[var(--text-faint)]">
        Recording latency
      </p>
      <p className="mb-2 text-sm leading-relaxed text-[var(--text-dim)]">{ROW_PURPOSE}</p>

      {devices !== null ? (
        <p
          data-testid="recording-latency-devices"
          className="mb-1 break-words text-[11px] text-[var(--text-faint)]"
        >
          {devicesText(devices)}
        </p>
      ) : null}

      <p
        data-testid="recording-latency-value"
        aria-live="polite"
        className={
          "break-words font-mono text-[11px] " +
          (unreadable !== null ? "text-[var(--warning)]" : "text-[var(--text-dim)]")
        }
      >
        {value}
      </p>

      {refusal !== null ? (
        <p
          data-testid="recording-latency-refusal"
          role="alert"
          className="mt-2 break-words text-xs text-[var(--warning)]"
        >
          {refusal}
        </p>
      ) : null}

      {devices !== null ? (
        <>
          <p
            data-testid="recording-latency-setup"
            className="mt-2 text-[11px] leading-snug text-[var(--text-faint)]"
          >
            {SETUP_LINE}
          </p>
          <div className="mt-2 flex flex-wrap gap-2">
            <button
              type="button"
              data-testid="recording-latency-measure"
              disabled={measuring}
              onClick={() => void measure()}
              className="btn-secondary rounded px-3 py-1 text-xs transition"
            >
              {latency === null ? "Measure" : "Measure again"}
            </button>
            {measuring ? (
              <button
                type="button"
                data-testid="recording-latency-cancel"
                disabled={cancelling}
                onClick={cancel}
                className="btn-secondary rounded px-3 py-1 text-xs transition"
              >
                {cancelling ? "Cancelling…" : "Cancel"}
              </button>
            ) : null}
          </div>
        </>
      ) : null}
    </div>
  );
}
