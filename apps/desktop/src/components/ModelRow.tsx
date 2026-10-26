/**
 * A downloadable model's row in Settings → Account: its size and source, and
 * the buttons that download it, stop the download and remove it (#383, #168).
 *
 * Two rows are made from this one component, the speech model's
 * (`SpeechModelRow`) and the speaker model's (`SpeakerModelRow`); each hands
 * it a {@link ModelCopy} with its id and its words. This file holds the only
 * call of `downloadMlModel` in the app, and it is made when the user presses
 * Download (or Retry). Nothing starts a download at launch, when Settings
 * opens, or from the agent. Opening Settings asks for the status
 * (`ml_model_status`, which reads the disk and makes no request); a download
 * already running when it opens is shown as it is, because the transfer
 * belongs to the backend, not to this row.
 *
 * What the row promises is small, and it says so: the files will be on
 * disk, checked against their hashes, and the assistant uses them when asked.
 * The copy never calls a model enabled or installed as a feature, and only a
 * model that is Ready is told to be used (`lib/speechModel.ts` and
 * `lib/speakerModel.ts` hold the words).
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
 * Progress and the end of a transfer arrive on `ml-model-progress`, for every
 * model; a row keeps the events of its own id. The row subscribes before it
 * asks for the status, so a change between the two is not lost, and an answer
 * to one of its own commands is dropped when an event arrived while it was
 * on the way (the event is newer).
 */

import { useCallback, useEffect, useRef, useState } from "react";

import { confirmAction } from "../lib/confirmAction";
import {
  failedText,
  isFullSize,
  megabytes,
  partialText,
  percent,
  progressText,
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

/** The id and the words of one model's row. */
export interface ModelCopy {
  /** The model id in `models.json`; the model commands take it as `model`. */
  id: string;
  /** The row's test ids are `settings-<prefix>` and `<prefix>-*`. */
  prefix: string;
  /** The small heading: "Speech model". */
  heading: string;
  /** What the model is called: "Whisper base". */
  title: string;
  /** Where Download fetches from. */
  source: string;
  /** The paragraph under the row, whatever its state. */
  note: string;
  /** "Checking…" */
  pending: string;
  /** What the status failing to load is about: "the speech model". */
  subject: string;
  /** The progress bar's label. */
  progressLabel: string;
  /** The confirmation before Remove deletes a model that is ready. */
  removeQuestion: (size: number) => string;
  /** Credit for the model's authors and licences, under the note. */
  licences?: string;
  /** The confirmation before unfinished files are deleted. */
  discardQuestion: string;
  /** The line for a model that is ready. */
  readyText: (status: MlModelStatus) => string;
}

export function ModelRow({ copy }: { copy: ModelCopy }) {
  const p = copy.prefix;
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
      if (!live || next.model !== copy.id) return;
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
      .then(() => mlModelStatus(copy.id))
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
  }, [copy.id]);

  const check = useCallback(() => {
    setProblem(null);
    mlModelStatus(copy.id).then(
      (next) => {
        if (alive.current) setStatus(next);
      },
      (e: unknown) => {
        if (alive.current) setProblem(messageOf(e));
      },
    );
  }, [copy.id]);

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
  const download = () => void run(() => downloadMlModel(copy.id));

  const cancel = () => {
    setCancelling(true);
    void run(() => cancelMlModelDownload(copy.id));
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
    await run(() => removeMlModel(copy.id));
  };

  const state = status?.state ?? (problem === null ? "pending" : "error");
  const size = status?.size ?? null;
  const partial = status !== null && status.bytes > 0 ? status.bytes : 0;
  // Files as big as the model that are not ready: not a partial download.
  const fullSize = status !== null && isFullSize(status.bytes, status.size);

  return (
    <div
      data-testid={`settings-${p}`}
      data-state={state}
      className="mt-3 border-t border-[var(--border)] pt-3"
    >
      <div className="font-mono text-[10px] uppercase tracking-[0.18em] text-[var(--text-faint)]">
        {copy.heading}
      </div>
      <p
        data-testid={`${p}-about`}
        className="mt-1 text-xs text-[var(--text-dim)]"
      >
        {copy.title}
        {size !== null ? `, ${megabytes(size)}` : ""}, from {copy.source}.
      </p>

      {status === null && problem === null ? (
        <p
          data-testid={`${p}-status`}
          className="mt-1 font-mono text-[11px] text-[var(--text-faint)]"
        >
          {copy.pending}
        </p>
      ) : null}

      {status === null && problem !== null ? (
        <div className="mt-1 flex flex-wrap items-center gap-2">
          <p
            data-testid={`${p}-status`}
            className="break-words font-mono text-[11px] text-[var(--warning)]"
          >
            Could not check {copy.subject} ({problem})
          </p>
          <button
            type="button"
            data-testid={`${p}-check`}
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
              data-testid={`${p}-status`}
              className="mb-2 font-mono text-[11px] text-[var(--text-dim)]"
            >
              {partialText(partial, status.size)}
            </p>
          ) : null}
          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              data-testid={`${p}-download`}
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
                data-testid={`${p}-discard`}
                disabled={busy}
                onClick={() => void remove(copy.discardQuestion)}
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
            aria-label={copy.progressLabel}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={percent(status.received, status.total)}
            data-testid={`${p}-progress`}
            className="h-1 w-full overflow-hidden rounded-sm bg-[var(--border)]"
          >
            <div
              data-testid={`${p}-progress-fill`}
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
              data-testid={`${p}-status`}
              className="font-mono text-[11px] text-[var(--text-dim)]"
            >
              {progressText(status.received, status.total)}
            </p>
            <button
              type="button"
              data-testid={`${p}-cancel`}
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
            data-testid={`${p}-status`}
            className="font-mono text-[11px] text-[var(--text-dim)]"
          >
            {copy.readyText(status)}
          </p>
          <button
            type="button"
            data-testid={`${p}-remove`}
            disabled={busy}
            onClick={() => void remove(copy.removeQuestion(status.size))}
            className="btn-secondary rounded px-3 py-1 text-xs transition"
          >
            Remove
          </button>
        </div>
      ) : null}

      {status?.state === "failed" ? (
        <div className="mt-2">
          <p
            data-testid={`${p}-status`}
            role="alert"
            className="mb-2 break-words text-xs text-[var(--warning)]"
          >
            {failedText(status.reason)}
          </p>
          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              data-testid={`${p}-retry`}
              disabled={busy}
              onClick={download}
              className="btn-secondary rounded px-3 py-1 text-xs transition"
            >
              Retry
            </button>
            {partial > 0 ? (
              <button
                type="button"
                data-testid={`${p}-discard`}
                disabled={busy}
                onClick={() => void remove(copy.discardQuestion)}
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
          data-testid={`${p}-problem`}
          role="alert"
          className="mt-2 break-words text-xs text-[var(--warning)]"
        >
          {problem}
        </p>
      ) : null}

      <p
        data-testid={`${p}-note`}
        className="mt-2 text-[11px] leading-snug text-[var(--text-faint)]"
      >
        {copy.note}
      </p>
      {copy.licences ? (
        <p
          data-testid={`${p}-licences`}
          className="mt-1 text-[11px] leading-snug text-[var(--text-faint)]"
        >
          {copy.licences}
        </p>
      ) : null}
    </div>
  );
}
