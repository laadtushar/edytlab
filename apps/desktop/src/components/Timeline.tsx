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
import { AutomationLane } from "./AutomationLane";
import { ClipStrip } from "./ClipStrip";
import type { ClipSummary, EnvelopePoint } from "../lib/tauri-bridge";
import { Ruler } from "./Ruler";
import { MarkerLayer } from "./MarkerLayer";

// -----------------------------------------------------------------------------
// Types
// -----------------------------------------------------------------------------

export interface TrackDescriptor {
  name: string;
  audioPath: string;
  muted: boolean;
  /**
   * Position in the *session's* track list.
   *
   * Not the lane's position: App filters out tracks with no audio
   * before handing them over, so lane 1 can be session track 3. The
   * mixer commands address tracks by session index, and without this
   * a pan on the second visible lane would land on whichever track
   * happened to be second overall. Absent falls back to lane order,
   * which is right whenever nothing was filtered.
   */
  index?: number;
  /**
   * Clips on this track, for the automation lane. Absent or empty
   * means no lane is drawn — a track with no clips has nothing to
   * automate.
   */
  clips?: ClipSummary[];
  /** Track gain in dB. Absent is treated as 0 (unity). */
  gainDb?: number;
  /** -1 hard left, 0 centre, 1 hard right. Absent is treated as 0. */
  pan?: number;
  soloed?: boolean;
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
  audioPath?: string | null;
  /** Alias for audioPath — accepted for backwards compat with tests. */
  src?: string | null;
  onFileDropped?: (path: string) => void;
  selection?: Selection | null;
  onSelectionChange?: (sel: Selection | null) => void;
  markers?: Marker[];
  onAddMarker?: (timeSec: number) => void;
  onRemoveMarker?: (id: string) => void;
  onSeekToMarker?: (timeSec: number) => void;
  zoom?: number;
  onZoomChange?: (zoom: number) => void;
  loop?: boolean;
  onLoopChange?: (loop: boolean) => void;
  spectrogramEnabled?: boolean;
  onSpectrogramChange?: (enabled: boolean) => void;
  /**
   * Mixer commits. Called with the lane index and the new value once
   * the user finishes a gesture (pointer release / keyboard change),
   * not on every intermediate slider position — each commit appends a
   * session node, and a drag would otherwise write one per pixel.
   *
   * Omitting a handler leaves that control local-only, which is what
   * the mute button did on its own before these existed.
   */
  onTrackGainChange?: (index: number, gainDb: number) => void;
  onTrackPanChange?: (index: number, pan: number) => void;
  onTrackMuteChange?: (index: number, muted: boolean) => void;
  onTrackSoloChange?: (index: number, soloed: boolean) => void;
  /**
   * Volume automation commit, once per finished gesture. Points are
   * relative to the clip's own start, matching `set_clip_envelope`.
   * Omitting it hides the automation lanes entirely.
   */
  onClipEnvelopeChange?: (
    trackIndex: number,
    clipIndex: number,
    points: EnvelopePoint[],
  ) => void;
  /**
   * Clip placement. Omitting these hides the clip strip, which is what
   * every existing caller and test gets.
   */
  onMoveClip?: (
    trackIndex: number,
    clipIndex: number,
    startSec: number,
  ) => void;
  onRemoveClip?: (trackIndex: number, clipIndex: number) => void;
}

// -----------------------------------------------------------------------------
// Lane control presentation
// -----------------------------------------------------------------------------

const toggleStyle = (on: boolean): React.CSSProperties => ({
  background: on ? "var(--accent-soft)" : "var(--surface-elev-2)",
  border: "1px solid",
  borderColor: on ? "rgba(255,138,61,0.45)" : "var(--border-strong)",
  borderRadius: 4,
  color: on ? "var(--accent)" : "var(--text-dim)",
  fontFamily: "var(--font-mono)",
  fontSize: 10,
  letterSpacing: "0.05em",
  textTransform: "uppercase",
  padding: "2px 6px",
  cursor: "pointer",
});

const faderLabelStyle: React.CSSProperties = {
  display: "flex",
  justifyContent: "space-between",
  width: "100%",
  fontFamily: "var(--font-mono)",
  fontSize: 9,
  letterSpacing: "0.05em",
  textTransform: "uppercase",
  color: "var(--text-dim)",
};

/**
 * Pan as mixing desks write it: C at centre, then L/R with the distance
 * as a percentage. A bare "-0.34" tells the user nothing about which
 * speaker it went to.
 */
function panLabel(pan: number): string {
  const pct = Math.round(Math.abs(pan) * 100);
  if (pct === 0) return "C";
  return `${pan < 0 ? "L" : "R"}${pct}`;
}

// -----------------------------------------------------------------------------
// Single track lane
// -----------------------------------------------------------------------------

interface LaneProps {
  name: string;
  audioPath: string | null;
  muted: boolean;
  onToggleMute: () => void;
  gainDb: number;
  pan: number;
  soloed: boolean;
  /** Live value while dragging; no session write. */
  onGainInput: (gainDb: number) => void;
  onPanInput: (pan: number) => void;
  /** Gesture finished — persist. */
  onGainCommit: (gainDb: number) => void;
  onPanCommit: (pan: number) => void;
  onToggleSolo: () => void;
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
  loop?: boolean;
}

function TrackLane({
  name,
  audioPath,
  muted,
  onToggleMute,
  gainDb,
  pan,
  soloed,
  onGainInput,
  onPanInput,
  onGainCommit,
  onPanCommit,
  onToggleSolo,
  onFileDropped,
  showDropHint,
  onWavesurfer,
  selection,
  onSelectionChange,
  onDurationChange,
  zoom,
  loop,
}: LaneProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const waveformWrapperRef = useRef<HTMLDivElement>(null);
  const wsRef = useRef<WaveSurfer | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [isDragging, setIsDragging] = useState(false);
  const [duration, setDuration] = useState(0);
  const [draftSelection, setDraftSelection] = useState<Selection | null>(null);
  const dragStateRef = useRef<{
    originPx: number;
    rectLeft: number;
    rectWidth: number;
  } | null>(null);
  const loopRef = useRef(loop);
  const selectionRef = useRef(selection);
  useEffect(() => {
    loopRef.current = loop;
  }, [loop]);
  useEffect(() => {
    selectionRef.current = selection;
  }, [selection]);

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
    const onAudioProcess = () => {
      if (!loopRef.current || !selectionRef.current) return;
      const t = ws.getCurrentTime();
      if (t >= selectionRef.current.end) {
        const dur = ws.getDuration();
        if (dur > 0) ws.seekTo(selectionRef.current.start / dur);
      }
    };
    ws.on("audioprocess", onAudioProcess);
    return () => {
      ws.un("ready", onReady);
      ws.un("decode", onReady);
      ws.un("audioprocess", onAudioProcess);
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

  // Preview level follows the fader as well as the mute button, so
  // dragging gain does something audible before a render.
  //
  // Only downward: WaveSurfer drives a media element, whose volume is
  // clamped to [0, 1], so boosts above unity cannot be previewed. They
  // are still written to the session and still apply at render — the
  // dB readout is the honest indicator there, not the loudspeaker.
  useEffect(() => {
    const linear = muted ? 0 : Math.min(1, 10 ** (gainDb / 20));
    wsRef.current?.setVolume(linear);
  }, [muted, gainDb]);

  useEffect(() => {
    if (!wsRef.current || duration === 0) return;
    wsRef.current.zoom(zoom ?? 0);
  }, [zoom, duration]);

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
      if (!onSelectionChange || !duration || !waveformWrapperRef.current)
        return;
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
        <div style={{ display: "flex", gap: 4 }}>
          <button
            type="button"
            data-testid="timeline-lane-mute"
            onClick={onToggleMute}
            aria-label={muted ? `Unmute ${name}` : `Mute ${name}`}
            aria-pressed={muted}
            style={toggleStyle(muted)}
          >
            {muted ? "muted" : "mute"}
          </button>
          <button
            type="button"
            data-testid="timeline-lane-solo"
            onClick={onToggleSolo}
            aria-label={soloed ? `Un-solo ${name}` : `Solo ${name}`}
            aria-pressed={soloed}
            style={toggleStyle(soloed)}
          >
            {soloed ? "soloed" : "solo"}
          </button>
        </div>

        {/* Gain. `onChange` tracks the drag for feedback; `onPointerUp`
            and `onKeyUp` are what write to the session, so one drag is
            one undoable node rather than one per pixel. */}
        <label style={faderLabelStyle}>
          <span>gain</span>
          <span data-testid="timeline-lane-gain-readout">
            {gainDb > 0 ? `+${gainDb.toFixed(1)}` : gainDb.toFixed(1)} dB
          </span>
        </label>
        <input
          type="range"
          data-testid="timeline-lane-gain"
          aria-label={`${name} gain in decibels`}
          min={-60}
          max={24}
          step={0.5}
          value={gainDb}
          onChange={(e) => onGainInput(Number(e.target.value))}
          onPointerUp={(e) => onGainCommit(Number(e.currentTarget.value))}
          onKeyUp={(e) => onGainCommit(Number(e.currentTarget.value))}
          onBlur={(e) => onGainCommit(Number(e.currentTarget.value))}
          style={{ width: "100%", accentColor: "var(--accent)" }}
        />

        <label style={faderLabelStyle}>
          <span>pan</span>
          <span data-testid="timeline-lane-pan-readout">{panLabel(pan)}</span>
        </label>
        <input
          type="range"
          data-testid="timeline-lane-pan"
          aria-label={`${name} stereo pan`}
          min={-1}
          max={1}
          step={0.02}
          value={pan}
          onChange={(e) => onPanInput(Number(e.target.value))}
          onPointerUp={(e) => onPanCommit(Number(e.currentTarget.value))}
          onKeyUp={(e) => onPanCommit(Number(e.currentTarget.value))}
          onBlur={(e) => onPanCommit(Number(e.currentTarget.value))}
          // Double-click returns to centre. A 0.02 step cannot always
          // land exactly on 0 from a drag, and "almost centred" is a
          // real mixing annoyance.
          onDoubleClick={() => {
            onPanInput(0);
            onPanCommit(0);
          }}
          style={{ width: "100%", accentColor: "var(--accent)" }}
        />
      </div>

      {/* Waveform region */}
      <div
        ref={waveformWrapperRef}
        onMouseDown={beginSelection}
        style={{
          flex: 1,
          position: "relative",
          overflowX: "auto",
          background: isDragging ? "var(--accent-soft)" : "var(--surface-elev)",
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

export const Timeline = forwardRef<TimelineHandle, TimelineProps>(
  function Timeline(
    {
      tracks,
      audioPath: audioPathProp,
      src,
      onFileDropped,
      selection,
      onSelectionChange,
      markers,
      onAddMarker,
      onRemoveMarker,
      onSeekToMarker,
      zoom,
      onZoomChange,
      loop,
      onLoopChange,
      spectrogramEnabled,
      onSpectrogramChange,
      onTrackGainChange,
      onTrackPanChange,
      onTrackMuteChange,
      onTrackSoloChange,
      onClipEnvelopeChange,
      onMoveClip,
      onRemoveClip,
    },
    ref,
  ) {
    const audioPath = audioPathProp ?? src ?? null;
    const headWsRef = useRef<WaveSurfer | null>(null);
    const [timelineDuration, setTimelineDuration] = useState(0);

    const defaultTracks: TrackDescriptor[] =
      tracks && tracks.length > 0
        ? tracks
        : [{ name: "Mix", audioPath: audioPath ?? "", muted: false }];

    const [laneStates, setLaneStates] =
      useState<TrackDescriptor[]>(defaultTracks);

    // Reconcile from the props, which come from `list_tracks` — the
    // session is the authority.
    //
    // This used to carry the previous lane's `muted` forward instead,
    // which made the toggle purely local: a mute set by the agent never
    // reached the button, and a mute set by the button never reached the
    // session. Optimistic writes below are overwritten by the next
    // refresh, which is the point of them.
    useEffect(() => {
      setLaneStates(
        tracks && tracks.length > 0
          ? tracks
          : [{ name: "Mix", audioPath: audioPath ?? "", muted: false }],
      );
    }, [tracks, audioPath]);

    /** Optimistic local edit, applied before the round trip. */
    const patchLane = (idx: number, patch: Partial<TrackDescriptor>) =>
      setLaneStates((prev) =>
        prev.map((t, i) => (i === idx ? { ...t, ...patch } : t)),
      );

    /**
     * Which clip the user last clicked, as `"laneIndex:clipIndex"`.
     *
     * One selection across the whole timeline rather than one per lane:
     * clicking a clip on another track should deselect the first, the
     * same way a file manager behaves.
     */
    const [selectedClip, setSelectedClip] = useState<string | null>(null);

    /** Session-level index for a lane — see `TrackDescriptor.index`. */
    const trackIndex = (idx: number) => laneStates[idx]?.index ?? idx;

    const handleToggleMute = (idx: number) => {
      const next = !(laneStates[idx]?.muted ?? false);
      patchLane(idx, { muted: next });
      onTrackMuteChange?.(trackIndex(idx), next);
    };

    const handleToggleSolo = (idx: number) => {
      const next = !(laneStates[idx]?.soloed ?? false);
      patchLane(idx, { soloed: next });
      onTrackSoloChange?.(trackIndex(idx), next);
    };

    const rootRef = useRef<HTMLDivElement>(null);

    useEffect(() => {
      const el = rootRef.current;
      if (!el) return;
      const handler = (e: WheelEvent) => {
        if (!e.ctrlKey) return;
        e.preventDefault();
        const delta = e.deltaY > 0 ? -20 : 20;
        onZoomChange?.(Math.max(0, (zoom ?? 0) + delta));
      };
      el.addEventListener("wheel", handler, { passive: false });
      return () => el.removeEventListener("wheel", handler);
    }, [zoom, onZoomChange]);

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
        ref={rootRef}
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
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <button
              type="button"
              data-testid="zoom-out-btn"
              onClick={() =>
                onZoomChange?.(Math.max(10, Math.round((zoom ?? 50) / 1.5)))
              }
              className="text-xs px-1.5 py-1 rounded border border-neutral-600 text-neutral-400 hover:border-neutral-400 transition-colors"
              title="Zoom out (Ctrl+scroll)"
            >
              −
            </button>
            <button
              type="button"
              data-testid="zoom-in-btn"
              onClick={() =>
                onZoomChange?.(Math.min(500, Math.round((zoom ?? 50) * 1.5)))
              }
              className="text-xs px-1.5 py-1 rounded border border-neutral-600 text-neutral-400 hover:border-neutral-400 transition-colors"
              title="Zoom in (Ctrl+scroll)"
            >
              +
            </button>
            <button
              type="button"
              data-testid="loop-btn"
              onClick={() => onLoopChange?.(!loop)}
              className={`text-xs px-2 py-1 rounded border transition-colors ${
                loop
                  ? "border-amber-400 text-amber-400 bg-amber-400/10"
                  : "border-neutral-600 text-neutral-400 hover:border-neutral-400"
              }`}
              title="Toggle loop (L)"
            >
              ↺
            </button>
            <button
              type="button"
              data-testid="spectrogram-btn"
              onClick={() => onSpectrogramChange?.(!spectrogramEnabled)}
              className={`text-xs px-2 py-1 rounded border transition-colors ${
                spectrogramEnabled
                  ? "border-amber-400 text-amber-400 bg-amber-400/10"
                  : "border-neutral-600 text-neutral-400 hover:border-neutral-400"
              }`}
              title="Toggle spectrogram"
            >
              Spec
            </button>
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
        </div>

        <Ruler duration={timelineDuration} onAddMarker={onAddMarker} />

        <div
          style={{
            flex: 1,
            position: "relative",
            overflow: "hidden",
            overflowY: "auto",
          }}
        >
          {laneStates.map((track, idx) => (
            <div key={track.name}>
              <TrackLane
                name={track.name}
                audioPath={track.audioPath || null}
                muted={track.muted}
                onToggleMute={() => handleToggleMute(idx)}
                gainDb={track.gainDb ?? 0}
                pan={track.pan ?? 0}
                soloed={track.soloed ?? false}
                onGainInput={(v) => patchLane(idx, { gainDb: v })}
                onPanInput={(v) => patchLane(idx, { pan: v })}
                onGainCommit={(v) => onTrackGainChange?.(trackIndex(idx), v)}
                onPanCommit={(v) => onTrackPanChange?.(trackIndex(idx), v)}
                onToggleSolo={() => handleToggleSolo(idx)}
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
                loop={idx === 0 ? loop : undefined}
              />
              {onMoveClip && (track.clips?.length ?? 0) > 0 && (
                <ClipStrip
                  trackName={track.name}
                  clips={track.clips ?? []}
                  duration={timelineDuration}
                  selectedClip={
                    selectedClip?.startsWith(`${idx}:`)
                      ? Number(selectedClip.split(":")[1])
                      : null
                  }
                  onSelectClip={(clipIndex) =>
                    setSelectedClip(
                      clipIndex === null ? null : `${idx}:${clipIndex}`,
                    )
                  }
                  onMoveClip={(clipIndex, startSec) =>
                    onMoveClip(trackIndex(idx), clipIndex, startSec)
                  }
                  onRemoveClip={(clipIndex) => {
                    setSelectedClip(null);
                    onRemoveClip?.(trackIndex(idx), clipIndex);
                  }}
                />
              )}
              {onClipEnvelopeChange && (track.clips?.length ?? 0) > 0 && (
                <AutomationLane
                  trackName={track.name}
                  clips={track.clips ?? []}
                  duration={timelineDuration}
                  onCommit={(clipIndex, points) =>
                    onClipEnvelopeChange(trackIndex(idx), clipIndex, points)
                  }
                />
              )}
            </div>
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
  },
);
