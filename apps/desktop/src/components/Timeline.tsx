/**
 * Timeline — M26 multi-track lane view.
 *
 * Renders each track in a horizontally-spanning lane, stacked
 * vertically.  Each lane shows:
 *  - a fixed-width left sidebar with the track name and mute button
 *  - a waveform region (wavesurfer.js) that fills the remaining width
 *
 * When the `tracks` prop is empty (or not provided), falls back to a
 * single lane labelled "Mix" using the `audioPath` prop so the
 * component is a drop-in replacement for the previous `<Canvas>`-only
 * timeline pane.
 *
 * NOTE: individual stem playback is not yet wired — each lane renders
 * the same rendered-mix WAV.  Per-stem paths will be threaded through
 * once the stem-cache render pipeline lands in Phase 3.
 * TODO(Phase 3): pass per-track audioPath once stem rendering exists.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import WaveSurfer from "wavesurfer.js";
import { convertFileSrc } from "@tauri-apps/api/core";
import { sendMessage as bridgeSendMessage } from "../lib/tauri-bridge";

// -----------------------------------------------------------------------------
// Types
// -----------------------------------------------------------------------------

export interface TrackDescriptor {
  name: string;
  /** Absolute path to a WAV for this track.  When absent the lane
   *  renders empty.  All tracks currently receive the mix path until
   *  stem rendering lands. */
  audioPath: string;
  muted: boolean;
}

export interface TimelineProps {
  /**
   * Per-track metadata.  When empty or undefined a single "Mix" lane
   * is shown using `audioPath`.
   */
  tracks?: TrackDescriptor[];
  /** Mix render path — used as the fallback single-lane source. */
  audioPath: string | null;
  /** Called when the user drops an audio file onto the canvas area. */
  onFileDropped?: (path: string) => void;
}

// -----------------------------------------------------------------------------
// Single track lane
// -----------------------------------------------------------------------------

interface LaneProps {
  name: string;
  audioPath: string | null;
  muted: boolean;
  onToggleMute: () => void;
  onFileDropped?: (path: string) => void;
  showDropHint?: boolean;
}

function TrackLane({
  name,
  audioPath,
  muted,
  onToggleMute,
  onFileDropped,
  showDropHint,
}: LaneProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const wsRef = useRef<WaveSurfer | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [isDragging, setIsDragging] = useState(false);

  // Mount wavesurfer once.
  useEffect(() => {
    if (!containerRef.current) return;
    const ws = WaveSurfer.create({
      container: containerRef.current,
      waveColor: "#4b5563",
      progressColor: "#3b82f6",
      cursorColor: "#e5e7eb",
      cursorWidth: 1,
      height: 72,
      barWidth: 2,
      barGap: 1,
      barRadius: 1,
      normalize: true,
    });
    wsRef.current = ws;
    return () => {
      ws.destroy();
      wsRef.current = null;
    };
  }, []);

  // Reload when audioPath changes.
  useEffect(() => {
    const ws = wsRef.current;
    if (!ws || !audioPath) return;
    setLoadError(null);
    try {
      const url = convertFileSrc(audioPath);
      ws.load(url).catch((err: unknown) => setLoadError(String(err)));
    } catch (err) {
      setLoadError(String(err));
    }
  }, [audioPath]);

  // Apply mute to wavesurfer volume.
  useEffect(() => {
    wsRef.current?.setVolume(muted ? 0 : 1);
  }, [muted]);

  const handleDrop = useCallback(
    async (e: React.DragEvent<HTMLDivElement>) => {
      e.preventDefault();
      setIsDragging(false);
      const file = e.dataTransfer.files?.[0];
      if (!file) return;
      const path = (file as File & { path?: string }).path;
      if (!path) {
        setLoadError("Could not resolve absolute path for the dropped file.");
        return;
      }
      onFileDropped?.(path);
      try {
        await bridgeSendMessage(`load this file: ${path}`);
      } catch (err) {
        setLoadError(String(err));
      }
    },
    [onFileDropped],
  );

  return (
    <div
      data-testid="timeline-lane"
      style={{
        display: "flex",
        borderBottom: "1px solid #262626",
        minHeight: 88,
      }}
      onDrop={handleDrop}
      onDragOver={(e) => {
        e.preventDefault();
        setIsDragging(true);
      }}
      onDragLeave={(e) => {
        e.preventDefault();
        setIsDragging(false);
      }}
    >
      {/* Left sidebar */}
      <div
        data-testid="timeline-lane-sidebar"
        style={{
          width: 132,
          flexShrink: 0,
          background: "var(--surface-elev)",
          borderRight: "1px solid var(--border)",
          display: "flex",
          flexDirection: "column",
          alignItems: "flex-start",
          justifyContent: "center",
          padding: "8px 12px",
          gap: 6,
        }}
      >
        <span
          data-testid="timeline-lane-name"
          title={name}
          style={{
            fontSize: 11,
            fontWeight: 500,
            color: "var(--text)",
            letterSpacing: "0.01em",
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
            maxWidth: "100%",
          }}
        >
          {name}
        </span>
        <button
          type="button"
          data-testid="timeline-lane-mute"
          onClick={onToggleMute}
          aria-label={muted ? `Unmute ${name}` : `Mute ${name}`}
          aria-pressed={muted}
          style={{
            background: muted ? "var(--accent-soft)" : "var(--surface-elev-2)",
            border: "1px solid",
            borderColor: muted
              ? "rgba(255,138,61,0.45)"
              : "var(--border-strong)",
            borderRadius: 4,
            color: muted ? "var(--accent)" : "var(--text-dim)",
            fontFamily: "var(--font-mono)",
            fontSize: 10,
            letterSpacing: "0.05em",
            textTransform: "uppercase",
            padding: "2px 8px",
            cursor: "pointer",
          }}
        >
          {muted ? "muted" : "mute"}
        </button>
      </div>

      {/* Waveform region */}
      <div
        style={{
          flex: 1,
          position: "relative",
          background: isDragging
            ? "var(--accent-soft)"
            : "var(--surface-elev)",
          padding: "10px 12px",
          boxShadow: isDragging
            ? "inset 0 0 0 1px var(--accent)"
            : "none",
          transition: "background 160ms ease, box-shadow 160ms ease",
        }}
      >
        <div
          ref={containerRef}
          data-testid="timeline-lane-waveform"
          style={{ height: "100%", width: "100%" }}
        />
        {!audioPath && showDropHint ? (
          <div
            data-testid="timeline-empty-hint"
            style={{
              pointerEvents: "none",
              position: "absolute",
              inset: 0,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              fontSize: 11,
              fontFamily: "var(--font-mono)",
              letterSpacing: "0.18em",
              textTransform: "uppercase",
              color: "var(--text-faint)",
            }}
          >
            drop audio · or use Open Audio…
          </div>
        ) : null}
        {loadError ? (
          <div
            data-testid="timeline-lane-error"
            role="alert"
            style={{
              position: "absolute",
              bottom: 6,
              left: 6,
              right: 6,
              background: "rgba(239,111,114,0.12)",
              border: "1px solid rgba(239,111,114,0.4)",
              borderRadius: 6,
              padding: "4px 10px",
              fontSize: 10,
              color: "var(--danger)",
            }}
          >
            {loadError}
          </div>
        ) : null}
      </div>
    </div>
  );
}

// -----------------------------------------------------------------------------
// Timeline
// -----------------------------------------------------------------------------

export function Timeline({ tracks, audioPath, onFileDropped }: TimelineProps) {
  // Build the effective track list.  If no tracks are provided, use a
  // single "Mix" lane backed by the mix audioPath.
  const defaultTracks: TrackDescriptor[] =
    tracks && tracks.length > 0
      ? tracks
      : [{ name: "Mix", audioPath: audioPath ?? "", muted: false }];

  const [laneStates, setLaneStates] = useState<TrackDescriptor[]>(defaultTracks);

  // Sync laneStates when the tracks prop or audioPath changes, preserving
  // user mute toggles for tracks that still exist (matched by name).
  useEffect(() => {
    setLaneStates((prev) => {
      const newBase =
        tracks && tracks.length > 0
          ? tracks
          : [{ name: "Mix", audioPath: audioPath ?? "", muted: false }];
      return newBase.map((t) => {
        const existing = prev.find((p) => p.name === t.name);
        return existing ? { ...t, muted: existing.muted } : t;
      });
    });
  }, [tracks, audioPath]);

  const handleToggleMute = (idx: number) => {
    setLaneStates((prev) =>
      prev.map((t, i) => (i === idx ? { ...t, muted: !t.muted } : t)),
    );
  };

  return (
    <div
      data-testid="timeline-root"
      className="app-fade-in"
      style={{
        display: "flex",
        flexDirection: "column",
        height: "100%",
        width: "100%",
        background: "var(--surface)",
        overflowY: "auto",
      }}
    >
      {/* Header */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          borderBottom: "1px solid var(--border)",
          padding: "8px 16px",
          flexShrink: 0,
          background: "var(--surface-elev)",
        }}
      >
        <h2
          style={{
            fontFamily: "var(--font-mono)",
            fontSize: 11,
            fontWeight: 500,
            letterSpacing: "0.2em",
            textTransform: "uppercase",
            color: "var(--text-dim)",
            margin: 0,
          }}
        >
          Timeline
        </h2>
        <span
          style={{
            fontFamily: "var(--font-mono)",
            fontSize: 10,
            letterSpacing: "0.18em",
            textTransform: "uppercase",
            color: "var(--text-faint)",
          }}
        >
          {laneStates.length} track{laneStates.length !== 1 ? "s" : ""}
        </span>
      </div>

      {/* Lanes */}
      {laneStates.map((track, idx) => (
        <TrackLane
          key={track.name}
          name={track.name}
          audioPath={
            // Only the first lane gets the mix audio to avoid N× volume stacking.
            // Individual stem paths will replace this in Phase 3.
            idx === 0 ? (track.audioPath || null) : null
          }
          muted={track.muted}
          onToggleMute={() => handleToggleMute(idx)}
          onFileDropped={idx === 0 ? onFileDropped : undefined}
          showDropHint={idx === 0 && !audioPath}
        />
      ))}
    </div>
  );
}
