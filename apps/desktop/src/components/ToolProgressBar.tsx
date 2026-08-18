/**
 * What a long tool is doing, and the way to stop it (#169 §1).
 *
 * A tool call is one round trip: it returns when it is finished. A
 * twelve-file batch is therefore an unexplained pause of unknown
 * length, which is indistinguishable from a hang. This is the strip
 * that makes the difference visible.
 *
 * Cancel stops *between* files, never inside one, so a stopped batch
 * leaves projects that either ran the whole chain or were never
 * started — never one whose history ends halfway through.
 */

import { useEffect, useState } from "react";

import {
  cancelLongRunningTool,
  onToolProgress,
  type ToolProgress,
} from "../lib/tauri-bridge";

export function ToolProgressBar() {
  const [progress, setProgress] = useState<ToolProgress | null>(null);
  const [cancelling, setCancelling] = useState(false);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;

    void onToolProgress((p) => {
      if (p.done) {
        // Clear on completion rather than leaving a finished bar up.
        // The result lands in the chat; this strip is only about the
        // wait.
        setProgress(null);
        setCancelling(false);
        return;
      }
      setProgress(p);
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  if (!progress) return null;

  const done = progress.index ?? 0;
  const pct = progress.total > 0 ? (done / progress.total) * 100 : 0;
  const name = progress.file?.split(/[/\\]/).pop() ?? "";

  return (
    <div
      data-testid="tool-progress"
      style={{
        display: "flex",
        alignItems: "center",
        gap: 10,
        padding: "6px 12px",
        borderBottom: "1px solid var(--border)",
        background: "var(--surface-elev, rgba(255,255,255,0.03))",
        fontSize: 12,
      }}
    >
      <span style={{ fontFamily: "var(--font-mono)", color: "var(--text-dim)" }}>
        {/* One-based for reading: "1 of 3" while the first is running. */}
        {done + 1} of {progress.total}
      </span>
      <span
        data-testid="tool-progress-file"
        style={{
          flex: 1,
          minWidth: 0,
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
          color: "var(--text)",
        }}
        title={progress.file}
      >
        {name}
      </span>
      {progress.refused > 0 ? (
        <span data-testid="tool-progress-refused" style={{ color: "var(--warn, #e0a03a)" }}>
          {progress.refused} refused
        </span>
      ) : null}
      <div
        aria-hidden
        style={{
          width: 120,
          height: 3,
          borderRadius: 2,
          background: "var(--border)",
          overflow: "hidden",
        }}
      >
        <div
          data-testid="tool-progress-fill"
          style={{
            width: `${pct}%`,
            height: "100%",
            background: "var(--accent)",
            transition: "width 0.2s ease",
          }}
        />
      </div>
      <button
        type="button"
        data-testid="tool-progress-cancel"
        disabled={cancelling}
        onClick={() => {
          setCancelling(true);
          void cancelLongRunningTool().catch(() => setCancelling(false));
        }}
        style={{
          fontSize: 11,
          padding: "2px 8px",
          borderRadius: 3,
          border: "1px solid var(--border)",
          background: "transparent",
          color: "var(--text-dim)",
          cursor: cancelling ? "default" : "pointer",
        }}
      >
        {/* The label changes because the stop is not instant — it lands
            at the end of the file in flight. */}
        {cancelling ? "Stopping…" : "Cancel"}
      </button>
    </div>
  );
}
