/**
 * ABCompareBar — M26 A/B compare control strip.
 *
 * Renders a dark 56 px horizontal bar with:
 *  - "A" button (left) — switches audio to the A-side WAV
 *  - centre label "↕ Switch on playhead" (decorative)
 *  - "B" button (right of centre) — switches audio to the B-side WAV
 *  - "Accept B ✓" button — calls `acceptB`, then notifies the parent
 *  - "✕" close button — calls `onClose`
 *
 * On mount the component calls `prepareCompare(aNodeId, bNodeId)` once
 * to pre-render both WAVs so the toggle is gapless. It shows a
 * "Preparing…" indicator while the render runs and an error banner if
 * it fails.
 *
 * A side whose session stores an effect the renderer cannot run plays
 * without it. The two sides then sound alike for the very thing being
 * compared, so what each side leaves out is listed under the bar (#532).
 */

import { useEffect, useRef, useState } from "react";

import { compareNotice } from "../lib/renderWarnings";
import {
  acceptB as bridgeAcceptB,
  prepareCompare,
  type RenderWarning,
} from "../lib/tauri-bridge";

import { RenderWarningsNotice } from "./RenderWarningsNotice";

export interface ABCompareBarProps {
  /** A-side node id (parent / before). */
  aNodeId: string | null;
  /** B-side node id (candidate / after). */
  bNodeId: string | null;
  /** Called whenever the user switches sides; receives the WAV path. */
  onAudioPathChange: (path: string) => void;
  /**
   * Move the session head to the B node. The parent puts it in line with
   * the session's other edits (#498); without it the bridge is called
   * directly.
   */
  acceptB?: (bNodeId: string) => Promise<string>;
  /** Called after `acceptB` resolves successfully. */
  onAcceptB: () => void;
  /** Dismiss the compare bar without accepting. */
  onClose: () => void;
}

type Side = "a" | "b";

export function ABCompareBar({
  aNodeId,
  bNodeId,
  onAudioPathChange,
  acceptB = bridgeAcceptB,
  onAcceptB,
  onClose,
}: ABCompareBarProps) {
  const [side, setSide] = useState<Side>("a");
  const [preparing, setPreparing] = useState(true);
  const [prepareError, setPrepareError] = useState<string | null>(null);
  const [accepting, setAccepting] = useState(false);
  const [left, setLeft] = useState<{ a: readonly RenderWarning[]; b: readonly RenderWarning[] }>({
    a: [],
    b: [],
  });

  // Stable refs so the effect closure captures the latest paths without
  // re-running the pre-render on every render.
  const aPathRef = useRef<string | null>(null);
  const bPathRef = useRef<string | null>(null);

  useEffect(() => {
    if (!aNodeId || !bNodeId) return;

    let cancelled = false;
    setPreparing(true);
    setPrepareError(null);
    setLeft({ a: [], b: [] });

    prepareCompare(aNodeId, bNodeId)
      .then(({ a_path, b_path, a_warnings, b_warnings }) => {
        if (cancelled) return;
        aPathRef.current = a_path;
        bPathRef.current = b_path;
        setLeft({ a: a_warnings ?? [], b: b_warnings ?? [] });
        setPreparing(false);
        // Default to the A side on first load.
        onAudioPathChange(a_path);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setPrepareError(String(err));
        setPreparing(false);
      });

    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [aNodeId, bNodeId, onAudioPathChange]);

  const handleSideSwitch = (newSide: Side) => {
    if (preparing) return;
    setSide(newSide);
    const path = newSide === "a" ? aPathRef.current : bPathRef.current;
    if (path) onAudioPathChange(path);
  };

  const handleAcceptB = async () => {
    if (!bNodeId || accepting) return;
    setAccepting(true);
    try {
      await acceptB(bNodeId);
      onAcceptB();
    } catch (err) {
      // Surface briefly — the parent may handle this differently in the
      // future, but for now show in-bar.
      setPrepareError(String(err));
    } finally {
      setAccepting(false);
    }
  };

  const bar = (
    <div
      data-testid="ab-compare-bar"
      style={{
        display: "flex",
        alignItems: "center",
        height: 56,
        background: "var(--surface-elev)",
        borderBottom: "1px solid var(--border)",
        padding: "0 12px",
        gap: 8,
        flexShrink: 0,
        position: "relative",
      }}
    >
      {/* A button */}
      <button
        type="button"
        data-testid="ab-side-a"
        disabled={preparing}
        aria-pressed={side === "a"}
        onClick={() => handleSideSwitch("a")}
        style={{
          minWidth: 48,
          height: 36,
          borderRadius: 6,
          border: side === "a" ? "2px solid var(--accent)" : "1px solid var(--border-strong)",
          background: side === "a" ? "var(--accent-soft)" : "var(--surface-elev-2)",
          color: side === "a" ? "var(--accent-text)" : "var(--text-dim)",
          fontWeight: 700,
          fontSize: 14,
          cursor: preparing ? "default" : "pointer",
          letterSpacing: 1,
          // The switch was instant, so an A/B comparison looked like
          // two unrelated states rather than like one thing being
          // compared (#211). Easing the colour makes the toggle read
          // as a toggle. Only the colours move — the button must not
          // change size under a pointer that is about to click it
          // again, which is what an A/B test is: repeated clicking.
          transition:
            "background var(--dur-2) var(--ease-out), " +
            "border-color var(--dur-2) var(--ease-out), " +
            "color var(--dur-2) var(--ease-out)",
        }}
      >
        A
      </button>

      {/* Centre label */}
      <span
        data-testid="ab-switch-label"
        style={{
          flex: 1,
          textAlign: "center",
          fontSize: 11,
          color: "var(--text-faint)",
          userSelect: "none",
          pointerEvents: "none",
        }}
      >
        {preparing ? "Preparing…" : "↕ Switch on playhead"}
      </span>

      {/* B button */}
      <button
        type="button"
        data-testid="ab-side-b"
        disabled={preparing}
        aria-pressed={side === "b"}
        onClick={() => handleSideSwitch("b")}
        style={{
          minWidth: 48,
          height: 36,
          borderRadius: 6,
          border: side === "b" ? "2px solid var(--accent-2)" : "1px solid var(--border-strong)",
          background:
            side === "b"
              ? "color-mix(in srgb, var(--accent-2) 14%, transparent)"
              : "var(--surface-elev-2)",
          color: side === "b" ? "hsl(290 90% 75%)" : "var(--text-dim)",
          fontWeight: 700,
          fontSize: 14,
          cursor: preparing ? "default" : "pointer",
          letterSpacing: 1,
          transition:
            "background var(--dur-2) var(--ease-out), " +
            "border-color var(--dur-2) var(--ease-out), " +
            "color var(--dur-2) var(--ease-out)",
        }}
      >
        B
      </button>

      {/* Accept B */}
      <button
        type="button"
        data-testid="ab-accept-b"
        disabled={preparing || accepting || !bNodeId}
        onClick={() => void handleAcceptB()}
        style={{
          height: 36,
          padding: "0 14px",
          borderRadius: 6,
          border: "1px solid color-mix(in srgb, var(--success) 45%, transparent)",
          background: "color-mix(in srgb, var(--success) 14%, transparent)",
          color: "var(--success)",
          fontSize: 12,
          fontWeight: 600,
          cursor: preparing || accepting || !bNodeId ? "default" : "pointer",
          opacity: preparing || accepting ? 0.5 : 1,
          whiteSpace: "nowrap",
        }}
      >
        {accepting ? "Accepting…" : "Accept B ✓"}
      </button>

      {/* Close */}
      <button
        type="button"
        data-testid="ab-close"
        onClick={onClose}
        aria-label="Close compare bar"
        style={{
          width: 28,
          height: 28,
          borderRadius: 6,
          border: "1px solid var(--border-strong)",
          background: "transparent",
          color: "var(--text-dim)",
          fontSize: 14,
          cursor: "pointer",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          padding: 0,
        }}
      >
        &times;
      </button>

      {/* Inline error */}
      {prepareError ? (
        <div
          role="alert"
          data-testid="ab-error"
          style={{
            position: "absolute",
            bottom: "calc(100% + 4px)",
            left: 12,
            right: 12,
            // Opaque: it floats over whatever is above the bar.
            background: "color-mix(in srgb, var(--danger) 12%, var(--surface-elev))",
            border: "1px solid var(--danger-border)",
            borderRadius: 6,
            padding: "6px 10px",
            fontSize: 11,
            color: "var(--danger)",
            zIndex: 50,
          }}
        >
          {prepareError}
        </div>
      ) : null}
    </div>
  );

  return (
    <>
      {bar}
      <RenderWarningsNotice
        testId="ab-render-warnings"
        notice={compareNotice(left.a, left.b)}
      />
    </>
  );
}
