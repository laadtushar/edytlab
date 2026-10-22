/**
 * The speech model in Settings → Account: its size and source, and the
 * buttons that download it, stop the download and remove it (#383).
 *
 * This is the only place in the app that downloads a model, and it does so
 * when the user presses Download. Nothing starts it at launch, when
 * Settings opens, or from the agent. Opening Settings asks for the status
 * (`ml_model_status`, which reads the disk and makes no request); a
 * download already running when it opens is shown as it is, because the
 * transfer belongs to the backend, not to this row.
 *
 * What the row promises is small, and it says so: the files will be on
 * disk, checked against their hashes. Transcription is not shipped (#384),
 * so the copy never calls the model enabled, installed as a feature or
 * working (`lib/speechModel.ts` holds the words).
 *
 * States, from the backend's `MlModelStatus`:
 *
 * - not downloaded: Download (or Resume download, and Delete partial
 *   download, when an earlier attempt left part of it; or Check and
 *   download, and Delete these files, when it left files as big as the
 *   model that are not verified);
 * - downloading: a progress bar with the bytes so far, and Cancel;
 * - ready: Ready, and Remove (which asks first);
 * - failed: the reason, and Retry.
 *
 * Progress and the end of a transfer arrive on `ml-model-progress`. The row
 * subscribes before it asks for the status, so a change between the two is
 * not lost, and an answer to one of its own commands is dropped when an
 * event arrived while it was on the way (the event is newer).
 */

import { useCallback, useEffect, useRef, useState } from "react";

import { confirmAction } from "../lib/confirmAction";
import {
  DISCARD_QUESTION,
  SPEECH_MODEL_ID,
  SPEECH_MODEL_NOTE,
  SPEECH_MODEL_PENDING,
  SPEECH_MODEL_SOURCE,
  SPEECH_MODEL_TITLE,
  failedText,
  isFullSize,
  megabytes,
  partialText,
  percent,
  progressText,
  readyText,
  removeQuestion,
} from "../lib/speechModel";
import {
  cancelMlModelDownload,
  downloadMlModel,
  mlModelStatus,
  onMlModelProgress,
  removeMlModel,
  type MlModelStatus,
} from "../lib/tauri-bridge";

function messageOf(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export function SpeechModelRow() {
  const [status, setStatus] = useState<MlModelStatus | null>(null);
  // A command that was rejected (the status could not be read, a remove was
  // refused). A download that fails is a `failed` status, not this.
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [cancelling, setCancelling] = useState(false);

  const alive = useRef(true);
  // Counts the events heard, so an answer that was overtaken by one is not
  // allowed to put an older state back.
  const heard = useRef(0);

  useEffect(() => {
    alive.current = true;
    // Per run of the effect, not the shared `alive` ref: under StrictMode
    // the first run's listener registers after the second run has set
    // `alive` again, and has to be let go all the same.
    let live = true;
    let unlisten: (() => void) | null = null;

    const subscribed = onMlModelProgress((next) => {
      if (!live || next.model !== SPEECH_MODEL_ID) return;
      heard.current += 1;
      setStatus(next);
      setProblem(null);
      if (next.state !== "downloading") setCancelling(false);
    }).then(
      (fn) => {
        if (live) unlisten = fn;
        else fn();
      },
      // No event channel: the status still shows, it just does not move.
      () => undefined,
    );

    void subscribed
      .then(() => mlModelStatus(SPEECH_MODEL_ID))
      .then(
        (first) => {
          // An event that came first is newer than this answer.
          if (live && heard.current === 0) setStatus(first);
        },
        (e: unknown) => {
          if (live) setProblem(messageOf(e));
        },
      );

    return () => {
      live = false;
      alive.current = false;
      unlisten?.();
    };
  }, []);

  const check = useCallback(() => {
    setProblem(null);
    mlModelStatus(SPEECH_MODEL_ID).then(
      (next) => {
        if (alive.current) setStatus(next);
      },
      (e: unknown) => {
        if (alive.current) setProblem(messageOf(e));
      },
    );
  }, []);

  const run = useCallback(
    async (call: () => Promise<MlModelStatus>) => {
      setProblem(null);
      setBusy(true);
      const mark = heard.current;
      try {
        const next = await call();
        if (alive.current && heard.current === mark) setStatus(next);
      } catch (e) {
        if (alive.current) {
          setProblem(messageOf(e));
          setCancelling(false);
        }
      } finally {
        if (alive.current) setBusy(false);
      }
    },
    [],
  );

  // The user's click, and the only call of `downloadMlModel` in the app.
  const download = () => void run(() => downloadMlModel(SPEECH_MODEL_ID));

  const cancel = () => {
    setCancelling(true);
    void run(() => cancelMlModelDownload(SPEECH_MODEL_ID));
  };

  const remove = async (question: string) => {
    let confirmed = false;
    try {
      confirmed = await confirmAction(question);
    } catch (e) {
      // The dialog could not be shown: that is not a yes.
      if (alive.current) setProblem(messageOf(e));
      return;
    }
    if (!confirmed) return;
    await run(() => removeMlModel(SPEECH_MODEL_ID));
  };

  const state = status?.state ?? (problem === null ? "pending" : "error");
  const size = status?.size ?? null;
  const partial = status !== null && status.bytes > 0 ? status.bytes : 0;
  // Files as big as the model that are not ready: not a partial download.
  const fullSize = status !== null && isFullSize(status.bytes, status.size);

  return (
    <div
      data-testid="settings-speech-model"
      data-state={state}
      className="mt-3 border-t border-[var(--border)] pt-3"
    >
      <div className="font-mono text-[10px] uppercase tracking-[0.18em] text-[var(--text-faint)]">
        Speech model
      </div>
      <p
        data-testid="speech-model-about"
        className="mt-1 text-xs text-[var(--text-dim)]"
      >
        {SPEECH_MODEL_TITLE}
        {size !== null ? `, ${megabytes(size)}` : ""}, from{" "}
        {SPEECH_MODEL_SOURCE}.
      </p>

      {status === null && problem === null ? (
        <p
          data-testid="speech-model-status"
          className="mt-1 font-mono text-[11px] text-[var(--text-faint)]"
        >
          {SPEECH_MODEL_PENDING}
        </p>
      ) : null}

      {status === null && problem !== null ? (
        <div className="mt-1 flex flex-wrap items-center gap-2">
          <p
            data-testid="speech-model-status"
            className="break-words font-mono text-[11px] text-[var(--warning)]"
          >
            Could not check the speech model ({problem})
          </p>
          <button
            type="button"
            data-testid="speech-model-check"
            onClick={check}
            className="btn-secondary rounded px-2 py-0.5 text-[10px] uppercase tracking-wider transition"
          >
            Check again
          </button>
        </div>
      ) : null}

      {status?.state === "not_downloaded" ? (
        <div className="mt-2">
          {partial > 0 ? (
            <p
              data-testid="speech-model-status"
              className="mb-2 font-mono text-[11px] text-[var(--text-dim)]"
            >
              {partialText(partial, status.size)}
            </p>
          ) : null}
          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              data-testid="speech-model-download"
              disabled={busy}
              onClick={download}
              className="btn-secondary rounded px-3 py-1 text-xs transition"
            >
              {fullSize
                ? "Check and download"
                : partial > 0
                  ? "Resume download"
                  : `Download (${megabytes(status.size)})`}
            </button>
            {partial > 0 ? (
              <button
                type="button"
                data-testid="speech-model-discard"
                disabled={busy}
                onClick={() => void remove(DISCARD_QUESTION)}
                className="btn-secondary rounded px-3 py-1 text-xs transition"
              >
                {fullSize ? "Delete these files" : "Delete partial download"}
              </button>
            ) : null}
          </div>
        </div>
      ) : null}

      {status?.state === "downloading" ? (
        <div className="mt-2">
          <div
            role="progressbar"
            aria-label="Speech model download"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={percent(status.received, status.total)}
            data-testid="speech-model-progress"
            className="h-1 w-full overflow-hidden rounded-sm bg-[var(--border)]"
          >
            <div
              data-testid="speech-model-progress-fill"
              style={{
                width: `${percent(status.received, status.total)}%`,
                background: "var(--gradient-brand)",
                transition: "width var(--dur-2) var(--ease-out)",
              }}
              className="h-full"
            />
          </div>
          <div className="mt-2 flex flex-wrap items-center gap-3">
            <p
              data-testid="speech-model-status"
              className="font-mono text-[11px] text-[var(--text-dim)]"
            >
              {progressText(status.received, status.total)}
            </p>
            <button
              type="button"
              data-testid="speech-model-cancel"
              disabled={cancelling}
              onClick={cancel}
              className="btn-secondary rounded px-3 py-1 text-xs transition"
            >
              {cancelling ? "Cancelling…" : "Cancel"}
            </button>
          </div>
        </div>
      ) : null}

      {status?.state === "ready" ? (
        <div className="mt-2 flex flex-wrap items-center gap-3">
          <p
            data-testid="speech-model-status"
            className="font-mono text-[11px] text-[var(--text-dim)]"
          >
            {readyText(status)}
          </p>
          <button
            type="button"
            data-testid="speech-model-remove"
            disabled={busy}
            onClick={() => void remove(removeQuestion(status.size))}
            className="btn-secondary rounded px-3 py-1 text-xs transition"
          >
            Remove
          </button>
        </div>
      ) : null}

      {status?.state === "failed" ? (
        <div className="mt-2">
          <p
            data-testid="speech-model-status"
            role="alert"
            className="mb-2 break-words text-xs text-[var(--warning)]"
          >
            {failedText(status.reason)}
          </p>
          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              data-testid="speech-model-retry"
              disabled={busy}
              onClick={download}
              className="btn-secondary rounded px-3 py-1 text-xs transition"
            >
              Retry
            </button>
            {partial > 0 ? (
              <button
                type="button"
                data-testid="speech-model-discard"
                disabled={busy}
                onClick={() => void remove(DISCARD_QUESTION)}
                className="btn-secondary rounded px-3 py-1 text-xs transition"
              >
                {fullSize ? "Delete these files" : "Delete partial download"}
              </button>
            ) : null}
          </div>
        </div>
      ) : null}

      {status !== null && problem !== null ? (
        <p
          data-testid="speech-model-problem"
          role="alert"
          className="mt-2 break-words text-xs text-[var(--warning)]"
        >
          {problem}
        </p>
      ) : null}

      <p
        data-testid="speech-model-note"
        className="mt-2 text-[11px] leading-snug text-[var(--text-faint)]"
      >
        {SPEECH_MODEL_NOTE}
      </p>
    </div>
  );
}
