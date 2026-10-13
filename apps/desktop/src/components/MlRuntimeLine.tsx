/**
 * One line in Settings: which ONNX Runtime library the app found (#383).
 *
 * "ONNX Runtime: 1.23.2 (bundled)", "ONNX Runtime: not installed in this
 * build", or the reason the one it found cannot be used. Until the
 * installers carry the library, a release build answers the second.
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
  return (
    <p
      data-testid="settings-ml-runtime"
      data-state={status?.state ?? (error === null ? "pending" : "error")}
      role="status"
      title={line.detail ?? undefined}
      className={`mt-4 break-words border-t border-[var(--border)] pt-3 font-mono text-[11px] ${TONE_CLASS[line.tone]}`}
    >
      {line.text}
    </p>
  );
}
