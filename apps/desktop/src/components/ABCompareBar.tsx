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
 */

import { useEffect, useRef, useState } from "react";

import { acceptB as bridgeAcceptB, prepareCompare } from "../lib/tauri-bridge";

export interface ABCompareBarProps {
  /** A-side node id (parent / before). */
  aNodeId: string | null;
  /** B-side node id (candidate / after). */
  bNodeId: string | null;
  /** Called whenever the user switches sides; receives the WAV path. */
  onAudioPathChange: (path: string) => void;
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
  onAcceptB,
  onClose,
}: ABCompareBarProps) {
  const [side, setSide] = useState<Side>("a");
  const [preparing, setPreparing] = useState(true);
  const [prepareError, setPrepareError] = useState<string | null>(null);
  const [accepting, setAccepting] = useState(false);

  // Stable refs so the effect closure captures the latest paths without
  // re-running the pre-render on every render.
  const aPathRef = useRef<string | null>(null);
  const bPathRef = useRef<string | null>(null);

  useEffect(() => {
    if (!aNodeId || !bNodeId) return;

    let cancelled = false;
    setPreparing(true);
    setPrepareError(null);

    prepareCompare(aNodeId, bNodeId)
      .then(({ a_path, b_path }) => {
        if (cancelled) return;
        aPathRef.current = a_path;
        bPathRef.current = b_path;
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
      await bridgeAcceptB(bNodeId);
      onAcceptB();
    } catch (err) {
      // Surface briefly — the parent may handle this differently in the
      // future, but for now show in-bar.
      setPrepareError(String(err));
    } finally {
      setAccepting(false);
    }
  };

  return (
    <div
      data-testid="ab-compare-bar"
      style={{
        display: "flex",
        alignItems: "center",
        height: 56,
        background: "#0f172a",
        borderBottom: "1px solid #1e293b",
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
          border: side === "a" ? "2px solid #3b82f6" : "1px solid #334155",
          background: side === "a" ? "#1e3a5f" : "#1e293b",
          color: side === "a" ? "#93c5fd" : "#94a3b8",
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
          color: "#475569",
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
          border: side === "b" ? "2px solid #f59e0b" : "1px solid #334155",
          background: side === "b" ? "#451a03" : "#1e293b",
          color: side === "b" ? "#fcd34d" : "#94a3b8",
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
          border: "1px solid #16a34a",
          background: "#14532d",
          color: "#86efac",
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
          border: "1px solid #334155",
          background: "transparent",
          color: "#64748b",
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
            background: "#450a0a",
            border: "1px solid #991b1b",
            borderRadius: 6,
            padding: "6px 10px",
            fontSize: 11,
            color: "#fca5a5",
            zIndex: 50,
          }}
        >
          {prepareError}
        </div>
      ) : null}
    </div>
  );
}
