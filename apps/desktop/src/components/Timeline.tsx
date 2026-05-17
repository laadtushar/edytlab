/**
 * Timeline — multi-track lane view with playhead, region selection,
 * and an imperative transport handle for window-level keyboard
 * shortcuts.
 *
 * The first lane (index 0) owns the timecode source for keyboard
 * transport (Space, Home/End, ←/→) bound at the App level. Every
 * lane now renders its own audio when `tracks[i].audioPath` is set.
 *
 * Region selection: mousedown + drag inside the waveform creates a
 * selection range expressed in seconds, hoisted to App via
 * `onSelectionChange`. Selection is rendered as a translucent amber
 * overlay; clicking outside the overlay (without dragging) clears.
 *
 * Per-track waveforms: when the caller supplies a `tracks` prop, each
 * lane renders the audio at its own `audioPath`. Multi-clip tracks
 * (no single source path) are filtered out upstream until M22+
 * realtime mixdown lands.
 */

import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
} from "react";

import WaveSurfer from "wavesurfer.js";
import { convertFileSrc } from "@tauri-apps/api/core";
import { sendMessage as bridgeSendMessage } from "../lib/tauri-bridge";
import type { Marker } from "../lib/tauri-bridge";
import { Ruler } from "./Ruler";
import { MarkerLayer } from "./MarkerLayer";

// -----------------------------------------------------------------------------
// Types
// -----------------------------------------------------------------------------

export interface TrackDescriptor {
  name: string;
  audioPath: string;
  muted: boolean;
}

/** A region selected on the waveform, expressed in seconds. */
export interface Selection {
  start: number;
  end: number;
}

/** Imperative transport handle exposed to App for keyboard shortcuts. */
export interface TimelineHandle {
  togglePlay: () => void;
  play: () => void;
  pause: () => void;
  seekTo: (seconds: number) => void;
  seekBy: (deltaSeconds: number) => void;
  getCurrentTime: () => number;
  getDuration: () => number;
}

export interface TimelineProps {
  tracks?: TrackDescriptor[];
  audioPath: string | null;
  onFileDropped?: (path: string) => void;
  selection?: Selection | null;
  onSelectionChange?: (sel: Selection | null) => void;
  markers?: Marker[];
  onAddMarker?: (timeSec: number) => void;
  onRemoveMarker?: (id: string) => void;
  onSeekToMarker?: (timeSec: number) => void;
  zoom?: number;
  onZoomChange?: (zoom: number) => void;
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
  /** Called once with the wavesurfer instance the first time it
   *  mounts; called again with null on unmount. Only the head lane
   *  publishes — passing undefined opts a lane out. */
  onWavesurfer?: (ws: WaveSurfer | null) => void;
  selection?: Selection | null;
  onSelectionChange?: (sel: Selection | null) => void;
  /** Called when the wavesurfer reports the audio duration. */
  onDurationChange?: (d: number) => void;
  /** Pixels per second zoom level. 0 = auto-fit. */
  zoom?: number;
}

function TrackLane({
  name,
  audioPath,
  muted,
  onToggleMute,
  onFileDropped,
  showDropHint,
  onWavesurfer,
  selection,
  onSelectionChange,
  onDurationChange,
  zoom,
}: LaneProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const waveformWrapperRef = useRef<HTMLDivElement>(null);
  const wsRef = useRef<WaveSurfer | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [isDragging, setIsDragging] = useState(false);
  const [duration, setDuration] = useState(0);
  const [draftSelection, setDraftSelection] = useState<Selection | null>(null);
  const dragStateRef = useRef<{ originPx: number; rectLeft: number; rectWidth: number } | null>(
    null,
  );

  // Mount wavesurfer once.
  useEffect(() => {
    if (!containerRef.current) return;
    const ws = WaveSurfer.create({
      container: containerRef.current,
      waveColor: "rgba(236, 237, 242, 0.35)",
      progressColor: "var(--accent)",
      cursorColor: "rgba(255, 138, 61, 0.85)",
      cursorWidth: 1,
      height: 72,
      barWidth: 2,
      barGap: 1,
      barRadius: 1,
      normalize: true,
    });
    wsRef.current = ws;
    onWavesurfer?.(ws);
    const onReady = () => {
      const d = ws.getDuration();
      setDuration(d);
      onDurationChange?.(d);
    };
    ws.on("ready", onReady);
    ws.on("decode", onReady);
    return () => {
      ws.un("ready", onReady);
      ws.un("decode", onReady);
      ws.destroy();
      wsRef.current = null;
      onWavesurfer?.(null);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
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

  useEffect(() => {
    wsRef.current?.setVolume(muted ? 0 : 1);
  }, [muted]);

  useEffect(() => {
    if (!wsRef.current) return;
    wsRef.current.zoom(zoom ?? 0);
  }, [zoom]);

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

  const beginSelection = useCallback(
    (e: React.MouseEvent<HTMLDivElement>) => {
      if (!onSelectionChange || !duration || !waveformWrapperRef.current) return;
      // Only left-click; Shift is reserved for multi-select later.
      if (e.button !== 0) return;
      const rect = waveformWrapperRef.current.getBoundingClientRect();
      const originPx = e.clientX - rect.left;
      dragStateRef.current = {
        originPx,
        rectLeft: rect.left,
        rectWidth: rect.width,
      };
      setDraftSelection({
        start: pxToSeconds(originPx, rect.width, duration),
        end: pxToSeconds(originPx, rect.width, duration),
      });
    },
    [duration, onSelectionChange],
  );

  useEffect(() => {
    if (!draftSelection) return;
    const onMove = (e: MouseEvent) => {
      const drag = dragStateRef.current;
      if (!drag) return;
      const px = clamp(e.clientX - drag.rectLeft, 0, drag.rectWidth);
      const tEnd = pxToSeconds(px, drag.rectWidth, duration);
      const tOrigin = pxToSeconds(drag.originPx, drag.rectWidth, duration);
      setDraftSelection({
        start: Math.min(tOrigin, tEnd),
        end: Math.max(tOrigin, tEnd),
      });
    };
    const onUp = () => {
      const final = draftSelection;
      dragStateRef.current = null;
      setDraftSelection(null);
      if (!final) return;
      // Treat a sub-50 ms drag as a click — clear selection rather
      // than create a degenerate range.
      if (final.end - final.start < 0.05) {
        onSelectionChange?.(null);
      } else {
        onSelectionChange?.(final);
      }
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  }, [draftSelection, duration, onSelectionChange]);

  const overlay = useMemo(() => {
    const range = draftSelection ?? selection ?? null;
    if (!range || !duration || !waveformWrapperRef.current) return null;
    const width = waveformWrapperRef.current.clientWidth || 1;
    const startPx = (range.start / duration) * width;
    const endPx = (range.end / duration) * width;
    return {
      left: Math.min(startPx, endPx),
      width: Math.abs(endPx - startPx),
    };
  }, [draftSelection, selection, duration]);

  return (
    <div
      data-testid="timeline-lane"
      style={{
        display: "flex",
        borderBottom: "1px solid var(--border)",
        minHeight: 92,
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
        ref={waveformWrapperRef}
        onMouseDown={beginSelection}
        style={{
          flex: 1,
          position: "relative",
          overflowX: "auto",
          background: isDragging
            ? "var(--accent-soft)"
            : "var(--surface-elev)",
          padding: "10px 12px",
          boxShadow: isDragging ? "inset 0 0 0 1px var(--accent)" : "none",
          transition: "background 160ms ease, box-shadow 160ms ease",
          cursor: duration > 0 ? "crosshair" : "default",
        }}
      >
        <div
          ref={containerRef}
          data-testid="timeline-lane-waveform"
          style={{ height: "100%", width: "100%", pointerEvents: "none" }}
        />
        {overlay ? (
          <div
            data-testid="timeline-selection-overlay"
            style={{
              position: "absolute",
              top: 4,
              bottom: 4,
              left: overlay.left + 12,
              width: overlay.width,
              background: "rgba(255, 138, 61, 0.18)",
              borderLeft: "1.5px solid var(--accent)",
              borderRight: "1.5px solid var(--accent)",
              pointerEvents: "none",
            }}
          />
        ) : null}
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

function pxToSeconds(px: number, totalPx: number, durationSec: number): number {
  if (totalPx <= 0) return 0;
  return clamp((px / totalPx) * durationSec, 0, durationSec);
}

function clamp(n: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, n));
}

// -----------------------------------------------------------------------------
// Timeline
// -----------------------------------------------------------------------------

export const Timeline = forwardRef<TimelineHandle, TimelineProps>(function Timeline(
  {
    tracks,
    audioPath,
    onFileDropped,
    selection,
    onSelectionChange,
    markers,
    onAddMarker,
    onRemoveMarker,
    onSeekToMarker,
    zoom,
    onZoomChange,
  },
  ref,
) {
  const headWsRef = useRef<WaveSurfer | null>(null);
  const [timelineDuration, setTimelineDuration] = useState(0);

  const defaultTracks: TrackDescriptor[] =
    tracks && tracks.length > 0
      ? tracks
      : [{ name: "Mix", audioPath: audioPath ?? "", muted: false }];

  const [laneStates, setLaneStates] = useState<TrackDescriptor[]>(defaultTracks);

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

  const handleWheel = useCallback(
    (e: React.WheelEvent) => {
      if (!e.ctrlKey) return;
      e.preventDefault();
      const delta = e.deltaY > 0 ? -20 : 20;
      onZoomChange?.(Math.max(0, (zoom ?? 0) + delta));
    },
    [zoom, onZoomChange],
  );

  useImperativeHandle(
    ref,
    () => ({
      togglePlay: () => {
        const ws = headWsRef.current;
        if (!ws) return;
        if (ws.isPlaying()) ws.pause();
        else void ws.play();
      },
      play: () => void headWsRef.current?.play(),
      pause: () => headWsRef.current?.pause(),
      seekTo: (sec: number) => {
        const ws = headWsRef.current;
        if (!ws) return;
        const d = ws.getDuration() || 0;
        if (d <= 0) return;
        ws.setTime(clamp(sec, 0, d));
      },
      seekBy: (delta: number) => {
        const ws = headWsRef.current;
        if (!ws) return;
        const d = ws.getDuration() || 0;
        if (d <= 0) return;
        ws.setTime(clamp(ws.getCurrentTime() + delta, 0, d));
      },
      getCurrentTime: () => headWsRef.current?.getCurrentTime() ?? 0,
      getDuration: () => headWsRef.current?.getDuration() ?? 0,
    }),
    [],
  );

  return (
    <div
      data-testid="timeline-root"
      className="app-fade-in"
      onWheel={handleWheel}
      style={{
        display: "flex",
        flexDirection: "column",
        height: "100%",
        width: "100%",
        background: "var(--surface)",
        overflowY: "auto",
      }}
    >
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

      <Ruler duration={timelineDuration} onAddMarker={onAddMarker} />

      <div style={{ flex: 1, position: "relative", overflow: "hidden", overflowY: "auto" }}>
        {laneStates.map((track, idx) => (
          <TrackLane
            key={track.name}
            name={track.name}
            audioPath={track.audioPath || null}
            muted={track.muted}
            onToggleMute={() => handleToggleMute(idx)}
            onFileDropped={idx === 0 ? onFileDropped : undefined}
            showDropHint={idx === 0 && !audioPath}
            onWavesurfer={
              idx === 0
                ? (ws) => {
                    headWsRef.current = ws;
                  }
                : undefined
            }
            selection={idx === 0 ? selection : null}
            onSelectionChange={idx === 0 ? onSelectionChange : undefined}
            onDurationChange={idx === 0 ? setTimelineDuration : undefined}
            zoom={zoom}
          />
        ))}
        {markers && markers.length > 0 && timelineDuration > 0 && (
          <MarkerLayer
            markers={markers}
            duration={timelineDuration}
            onSeek={(t) => onSeekToMarker?.(t)}
            onRemove={(id) => onRemoveMarker?.(id)}
          />
        )}
      </div>
    </div>
  );
});
