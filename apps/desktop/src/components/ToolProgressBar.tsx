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

/**
 * The kinds that are actually progress (#252).
 *
 * `progress::report` is one channel and everything on it landed here
 * unfiltered — including `select_region`'s selection report, which
 * carries no `total`, no `index`, no `file` and no `done`. That
 * rendered a 0%-filled strip reading "1 of " with a blank filename,
 * pinned above the timeline until some *unrelated* long-running tool
 * happened to emit `done`. In an ordinary editing session: indefinitely.
 *
 * An allow-list rather than a deny-list, so a new report kind has to opt
 * in rather than accidentally pinning a broken strip.
 */
const PROGRESS_KINDS = new Set(["batch_apply", "timer_record"]);

export function ToolProgressBar() {
  const [progress, setProgress] = useState<ToolProgress | null>(null);
  const [cancelling, setCancelling] = useState(false);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;

    void onToolProgress((p) => {
      if (!PROGRESS_KINDS.has(p.kind)) return;
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
    // The strip inserts itself above the timeline, so everything below
    // shifts down the moment a batch starts. `strip-in` animates the
    // height open so that shift is a movement to follow rather than a
    // relayout to re-read — which matters here more than anywhere else,
    // because this component exists specifically to cover a wait, and a
    // thing that covers a wait should not itself arrive as a jolt.
    <div className="strip-in" data-testid="tool-progress-shell">
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
        <span
          style={{ fontFamily: "var(--font-mono)", color: "var(--text-dim)" }}
        >
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
          <span
            data-testid="tool-progress-refused"
            style={{ color: "var(--warn, #e0a03a)" }}
          >
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
              // Was a hard-coded 200ms/ease. Same timing, named — this
              // is the vocabulary's "something arriving", which is what
              // a bar advancing to a new position is.
              transition: "width var(--dur-2) var(--ease-out)",
            }}
          />
        </div>
        <button
          type="button"
          data-testid="tool-progress-cancel"
          disabled={cancelling}
          onClick={() => {
            setCancelling(true);
            // Reset when the call settles, not only on a `done` event
            // (#252). `cancel_long_running_tool` returns `Ok(())`
            // unconditionally, so the old `.catch` never fired — and if
            // no `done` followed, the button stayed on "Stopping…"
            // forever with no way back.
            void cancelLongRunningTool().finally(() => setCancelling(false));
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
    </div>
  );
}
