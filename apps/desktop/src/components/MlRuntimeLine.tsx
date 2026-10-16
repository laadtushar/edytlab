/**
 * One line in Settings: which ONNX Runtime library the app found (#383).
 *
 * "ONNX Runtime: 1.23.2 (bundled)", "ONNX Runtime: not installed in this
 * build", or the reason the one it found cannot be used. A release
 * installer after v0.6.1 answers the first (#383); a build from source, or
 * an older release, the second.
 *
 * The status is asked for once, when the line mounts. The backend loads the
 * library to answer, off the main thread, and a missing or broken one is an
 * answer rather than a failure, so there is nothing to retry.
 */

import { useEffect, useState } from "react";

import { mlRuntimeLine } from "../lib/mlRuntimeLine";
import { mlRuntimeStatus, type MlRuntimeStatus } from "../lib/tauri-bridge";

const TONE_CLASS = {
  ok: "text-[var(--text-dim)]",
  info: "text-[var(--text-faint)]",
  problem: "text-[var(--warning)]",
} as const;

export function MlRuntimeLine() {
  const [status, setStatus] = useState<MlRuntimeStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    mlRuntimeStatus().then(
      (s) => {
        if (!cancelled) setStatus(s);
      },
      (e: unknown) => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      },
    );
    return () => {
      cancelled = true;
    };
  }, []);

  const line = mlRuntimeLine(status, error);
  // Plain text, no live region. The status arrives a moment after the tab
  // opens, and a `role="status"` would read "checking…" and then the result
  // aloud every time the Account tab is opened. It is something to read when
  // you get to it, not an event. The file or the places searched are text
  // too (a `title` cannot be reached from the keyboard or by a screen
  // reader).
  return (
    <div className="mt-4 border-t border-[var(--border)] pt-3">
      <p
        data-testid="settings-ml-runtime"
        data-state={status?.state ?? (error === null ? "pending" : "error")}
        className={`break-words font-mono text-[11px] ${TONE_CLASS[line.tone]}`}
      >
        {line.text}
      </p>
      {line.detail !== null ? (
        <p
          data-testid="settings-ml-runtime-detail"
          className="mt-1 break-all font-mono text-[10px] text-[var(--text-faint)]"
        >
          {line.detail}
        </p>
      ) : null}
    </div>
  );
}
