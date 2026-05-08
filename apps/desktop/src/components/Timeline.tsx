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
      const path =
        (file as File & { path?: string }).path ?? file.name;
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
          width: 120,
          flexShrink: 0,
          background: "#171717",
          borderRight: "1px solid #262626",
          display: "flex",
          flexDirection: "column",
          alignItems: "flex-start",
          justifyContent: "center",
          padding: "6px 8px",
          gap: 4,
        }}
      >
        <span
          data-testid="timeline-lane-name"
          title={name}
          style={{
            fontSize: 11,
            fontWeight: 600,
            color: "#e5e7eb",
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
            background: muted ? "#7f1d1d" : "#262626",
            border: "1px solid",
            borderColor: muted ? "#991b1b" : "#404040",
            borderRadius: 4,
            color: muted ? "#fca5a5" : "#9ca3af",
            fontSize: 10,
            padding: "2px 6px",
            cursor: "pointer",
          }}
        >
          {muted ? "Muted" : "M"}
        </button>
      </div>

      {/* Waveform region */}
      <div
        style={{
          flex: 1,
          position: "relative",
          background: isDragging ? "#1e3a5f" : "#111827",
          padding: "8px 8px",
          outline: isDragging ? "2px solid #3b82f6" : "none",
          outlineOffset: -2,
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
              fontSize: 12,
              color: "#6b7280",
            }}
          >
            Drop an audio file or use the file menu
          </div>
        ) : null}
        {loadError ? (
          <div
            data-testid="timeline-lane-error"
            role="alert"
            style={{
              position: "absolute",
              bottom: 4,
              left: 4,
              right: 4,
              background: "rgba(127,29,29,0.8)",
              border: "1px solid #991b1b",
              borderRadius: 4,
              padding: "4px 8px",
              fontSize: 10,
              color: "#fca5a5",
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

  // Sync laneStates when the tracks prop or audioPath changes.
  useEffect(() => {
    setLaneStates(
      tracks && tracks.length > 0
        ? tracks
        : [{ name: "Mix", audioPath: audioPath ?? "", muted: false }],
    );
  }, [tracks, audioPath]);

  const handleToggleMute = (idx: number) => {
    setLaneStates((prev) =>
      prev.map((t, i) => (i === idx ? { ...t, muted: !t.muted } : t)),
    );
  };

  return (
    <div
      data-testid="timeline-root"
      style={{
        display: "flex",
        flexDirection: "column",
        height: "100%",
        width: "100%",
        background: "#111827",
        overflowY: "auto",
      }}
    >
      {/* Header */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          borderBottom: "1px solid #262626",
          padding: "6px 12px",
          flexShrink: 0,
          background: "#171717",
        }}
      >
        <h2 style={{ fontSize: 12, fontWeight: 600, color: "#e5e7eb", margin: 0 }}>
          Timeline
        </h2>
        <span style={{ fontSize: 10, color: "#6b7280" }}>
          {laneStates.length} track{laneStates.length !== 1 ? "s" : ""}
        </span>
      </div>

      {/* Lanes */}
      {laneStates.map((track, idx) => (
        <TrackLane
          key={`${track.name}-${idx}`}
          name={track.name}
          audioPath={track.audioPath || null}
          muted={track.muted}
          onToggleMute={() => handleToggleMute(idx)}
          onFileDropped={idx === 0 ? onFileDropped : undefined}
          showDropHint={idx === 0 && !audioPath}
        />
      ))}
    </div>
  );
}
