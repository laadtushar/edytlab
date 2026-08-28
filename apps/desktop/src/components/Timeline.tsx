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
  useLayoutEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
} from "react";

import WaveSurfer from "wavesurfer.js";
import Spectrogram from "wavesurfer.js/dist/plugins/spectrogram.esm.js";

/**
 * The drawn height of one lane, in CSS pixels.
 *
 * Shared by the waveform and the spectrogram so the two occupy exactly
 * the same box — the playhead and the selection overlay are positioned
 * against that box, and a spectrogram of a different height would slide
 * them off the audio they point at.
 */
const LANE_HEIGHT = 72;
import { convertFileSrc } from "@tauri-apps/api/core";
import { sendMessage as bridgeSendMessage } from "../lib/tauri-bridge";
import type { Marker } from "../lib/tauri-bridge";
import { snapRange } from "../lib/zeroCrossing";
import { AutomationLane } from "./AutomationLane";
import { TrackMenu } from "./TrackMenu";
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
  /** Fill the pane with the current selection, and scroll to it. */
  zoomToSelection: () => void;
  /** Back to the whole session across the pane. */
  fitToWindow: () => void;
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
  /**
   * The rendered mix — the output of `render_preview` for the current
   * head, with gain, pan, mute, solo, chains, sends and the master
   * chain in it (#155).
   *
   * This is the **only** thing that is ever played. The lanes draw
   * their own source audio and are silent: a lane holds one track with
   * no mixer state applied, so playing lane 0 played one track raw and
   * every other track not at all — the bug this closes.
   */
  mixPath?: string | null;
  /** Snap selection edges to zero crossings. Off is today's behaviour. */
  snapToZero?: boolean;
  onSnapToZeroChange?: (enabled: boolean) => void;
  /**
   * Sync-lock: an edit that shifts time on one track shifts them all
   * (#170 §3). Shown as a toggle in the header rather than parked in a
   * menu, because it silently changes what the next cut does and the
   * user has to be able to see that it is on.
   */
  syncLock?: boolean;
  onSyncLockChange?: (enabled: boolean) => void;
  /** Waveform height multiplier; 1 is the real amplitude. */
  verticalZoom?: number;
  onVerticalZoomChange?: (factor: number) => void;
  /**
   * Playhead position in session seconds. Omitted, the timeline follows
   * its own transport; supplied, the caller is the authority — which is
   * what a seek driven from outside (a marker click, a chapter jump)
   * needs.
   */
  playheadSec?: number;
  /**
   * Track-head actions. All three are required together — the menu is
   * hidden unless every item in it can do something.
   */
  onRenameTrack?: (trackIndex: number, name: string) => void;
  onDuplicateTrack?: (trackIndex: number) => void;
  onRemoveTrack?: (trackIndex: number) => void;
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
  /**
   * Length of the *session*, which is the axis the ruler, the clip
   * strip and every range-taking tool use.
   *
   * Selection used to be measured against this lane's own decoded
   * duration and then handed to `render_range` as session-absolute
   * seconds (#171). On a 60 s session whose first track is a 10 s clip,
   * dragging across half the lane exported 0–5 s of the session — a
   * different span of different audio. The two agree only when the
   * lane happens to be as long as the session, which is why a
   * single-file session never showed it.
   */
  sessionDuration?: number;
  /**
   * Snap selection edges to the nearest zero crossing before committing
   * them. Off by default, because off is the behaviour that existed.
   */
  snapToZero?: boolean;
  /**
   * Waveform height multiplier. 1 draws the samples at their real
   * amplitude; higher magnifies quiet material without changing it.
   */
  verticalZoom?: number;
  /**
   * Playhead position in *session* seconds, drawn by the lane itself.
   *
   * WaveSurfer's own cursor cannot be used for this. `setTime` clamps
   * to the lane's media duration (`player.js`), so a 3-second lane
   * asked to show t=30 pins at 3 and a lane with no audio pins at 0 —
   * and at zoom 0 every lane stretches its own duration across the full
   * width, so the same x means a different time on every one. Absent
   * means no playhead is drawn.
   */
  playheadSec?: number;
  /**
   * Track-head menu. Absent hides the menu entirely, which is what a
   * caller that cannot act on these gets — a menu whose items do
   * nothing is worse than no menu.
   */
  trackIndex?: number;
  onRenameTrack?: (trackIndex: number, name: string) => void;
  onDuplicateTrack?: (trackIndex: number) => void;
  onRemoveTrack?: (trackIndex: number) => void;
  /** Pixels per second zoom level. 0 = auto-fit. */
  zoom?: number;
  loop?: boolean;
  /** Draw a spectrogram in place of the waveform. */
  spectrogramEnabled?: boolean;
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
  sessionDuration,
  snapToZero,
  verticalZoom,
  playheadSec,
  trackIndex,
  onRenameTrack,
  onDuplicateTrack,
  onRemoveTrack,
  zoom,
  loop,
  spectrogramEnabled,
}: LaneProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const spectrogramHostRef = useRef<HTMLDivElement>(null);
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
      // No cursor of its own: it is drawn on this lane's duration,
       // which is not the axis the ruler, the clips or the selection
       // use. The playhead div below is drawn on the session axis, so
       // every lane agrees with every other and with the ruler.
      cursorColor: "transparent",
      cursorWidth: 0,
      height: LANE_HEIGHT,
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
    // No playback handlers here. A lane is a picture of one track's
    // own audio, with no mixer state applied — it is never played, so
    // looping and level belong to the mix player instead.
    return () => {
      ws.un("ready", onReady);
      ws.un("decode", onReady);
      ws.destroy();
      wsRef.current = null;
      onWavesurfer?.(null);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /**
   * Magnify the drawn waveform.
   *
   * `normalize` has to come off above 1x. Normalising scales each
   * lane's peak to full height, which is pleasant to look at and hides
   * exactly what this control exists to show: normalised, a -40 dBFS
   * passage and a hot one are drawn the same, so magnifying one
   * magnifies nothing. At 1x it stays on, because that is how the lanes
   * have always looked.
   */
  useEffect(() => {
    const ws = wsRef.current;
    if (!ws) return;
    const factor = verticalZoom && verticalZoom > 0 ? verticalZoom : 1;
    ws.setOptions({ barHeight: factor, normalize: factor <= 1 });
  }, [verticalZoom]);

  /**
   * Draw a spectrogram instead of the waveform while "Spec" is on.
   *
   * The toggle used to set state that nothing read: it coloured its own
   * button and the lanes went on drawing the same waveform, while the
   * changelog announced the feature as shipped (#254). The plugin half
   * was never written — the commit that added the button touched no
   * plugin at all.
   *
   * Registered per lane, into a host element of the lane's own height,
   * rather than letting the plugin append its canvas below the
   * waveform: the playhead and selection overlays are absolutely
   * positioned against the waveform box, and a canvas that grew the
   * lane would slide them off the audio they point at.
   *
   * Guarded on `duration` for the same reason `zoom()` is — the plugin
   * reads decoded audio, and there is none before the first decode.
   */
  useEffect(() => {
    const ws = wsRef.current;
    const host = spectrogramHostRef.current;
    if (!ws || !host || !spectrogramEnabled || duration === 0) return;

    const plugin = ws.registerPlugin(
      Spectrogram.create({
        container: host,
        height: LANE_HEIGHT,
        // The lane is 72px of a much wider strip; axis labels would
        // take more of it than the picture.
        labels: false,
        fftSamples: 512,
      }),
    );
    return () => plugin.destroy();
  }, [spectrogramEnabled, duration]);

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

  // A lane makes no sound, so its volume is not a preview of anything
  // — but it is set to zero anyway, so that a lane which somehow gets
  // played by a future change is silent rather than quietly wrong.
  //
  // This used to follow the fader, which was a real preview back when
  // lane 0 was the transport. It is not one now: what you hear is the
  // rendered mix, and a fader move is audible after the next render.
  // The status bar's stale-mix indicator is what says so.
  useEffect(() => {
    wsRef.current?.setVolume(0);
  }, []);

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

  // What a pixel means. The session axis when we know it; this lane's
  // own audio only as a fallback for a lane with no session context,
  // which is the single-file case where the two are equal anyway.
  const axis = sessionDuration && sessionDuration > 0 ? sessionDuration : duration;

  const beginSelection = useCallback(
    (e: React.MouseEvent<HTMLDivElement>) => {
      if (!onSelectionChange || !axis || !waveformWrapperRef.current) return;
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
        start: pxToSeconds(originPx, rect.width, axis),
        end: pxToSeconds(originPx, rect.width, axis),
      });
    },
    [axis, onSelectionChange],
  );

  /**
   * Move the committed edges onto zero crossings, when asked to and
   * when it is this lane's audio the selection is over.
   *
   * That second condition is not fussiness. Selection is measured on
   * the session axis (#171), and this lane's samples are only the audio
   * at that time when the lane runs the length of the session. Snapping
   * against the wrong buffer would move the boundary to a crossing that
   * is not where the user is cutting — worse than not snapping, and
   * invisible. So when the axes disagree we leave the selection alone,
   * which is the behaviour that existed before the toggle.
   */
  const maybeSnap = useCallback(
    (range: Selection): Selection => {
      if (!snapToZero) return range;
      const ws = wsRef.current;
      if (!ws) return range;
      const laneIsTheAxis = duration > 0 && Math.abs(axis - duration) < 0.01;
      if (!laneIsTheAxis) return range;

      const decoded = ws.getDecodedData?.();
      if (!decoded) return range;
      const channel = decoded.getChannelData(0);
      if (!channel?.length) return range;

      return snapRange(channel, decoded.sampleRate, range);
    },
    [snapToZero, axis, duration],
  );

  useEffect(() => {
    if (!draftSelection) return;
    const onMove = (e: MouseEvent) => {
      const drag = dragStateRef.current;
      if (!drag) return;
      const px = clamp(e.clientX - drag.rectLeft, 0, drag.rectWidth);
      const tEnd = pxToSeconds(px, drag.rectWidth, axis);
      const tOrigin = pxToSeconds(drag.originPx, drag.rectWidth, axis);
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
        onSelectionChange?.(maybeSnap(final));
      }
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  }, [draftSelection, axis, onSelectionChange, maybeSnap]);

  /**
   * The drawing surface's width, in pixels.
   *
   * Measured into state rather than read from the ref inside a memo:
   * the ref is null on the first render, so a memo that closed over it
   * would compute `null` once and never re-run — the playhead would
   * simply never appear. It also has to follow the window, since the
   * pane is a flex child of a resizable layout.
   */
  const [paneWidth, setPaneWidth] = useState(0);
  useLayoutEffect(() => {
    const measure = () =>
      setPaneWidth(waveformWrapperRef.current?.clientWidth ?? 0);
    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, []);

  /**
   * Where the playhead sits on this lane, in pixels, or null when there
   * is nothing to draw.
   *
   * Measured on the same axis as the ruler and the selection, so a
   * seek moves every lane's playhead to the same place — including
   * lanes whose own audio is shorter than the session, which is exactly
   * where WaveSurfer's clamped cursor gave the wrong answer.
   */
  const playhead = useMemo(() => {
    if (playheadSec === undefined || !axis || paneWidth <= 0) return null;
    return (clamp(playheadSec, 0, axis) / axis) * paneWidth;
  }, [playheadSec, axis, paneWidth]);

  const overlay = useMemo(() => {
    const range = draftSelection ?? selection ?? null;
    if (!range || !axis || paneWidth <= 0) return null;
    const width = paneWidth;
    // Same axis the ruler above is drawn on, so the overlay lines up
    // with the ticks rather than merely looking plausible.
    const startPx = (range.start / axis) * width;
    const endPx = (range.end / axis) * width;
    return {
      left: Math.min(startPx, endPx),
      width: Math.abs(endPx - startPx),
    };
  }, [draftSelection, selection, axis, paneWidth]);

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
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 6,
            width: "100%",
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
              flex: 1,
              minWidth: 0,
            }}
          >
            {name}
          </span>
          {onRenameTrack && onDuplicateTrack && onRemoveTrack && (
            <TrackMenu
              trackIndex={trackIndex ?? 0}
              trackName={name}
              onRename={onRenameTrack}
              onDuplicate={onDuplicateTrack}
              onRemove={onRemoveTrack}
            />
          )}
        </div>
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
          cursor: axis > 0 ? "crosshair" : "default",
        }}
      >
        <div
          ref={containerRef}
          data-testid="timeline-lane-waveform"
          style={{
            height: "100%",
            width: "100%",
            pointerEvents: "none",
            // Hidden rather than unmounted: WaveSurfer owns this
            // element, and tearing it out from under the instance
            // would mean rebuilding the lane on every toggle.
            visibility: spectrogramEnabled ? "hidden" : "visible",
          }}
        />
        <div
          ref={spectrogramHostRef}
          data-testid="timeline-lane-spectrogram"
          style={{
            position: "absolute",
            top: 10,
            left: 12,
            right: 12,
            height: LANE_HEIGHT,
            pointerEvents: "none",
            display: spectrogramEnabled ? "block" : "none",
          }}
        />
        {playhead !== null ? (
          <div
            data-testid="timeline-playhead"
            data-playhead-sec={playheadSec}
            style={{
              position: "absolute",
              top: 4,
              bottom: 4,
              // +12 matches the wrapper's horizontal padding, the same
              // offset the selection overlay uses.
              left: playhead + 12,
              width: 1,
              background: "rgba(255, 138, 61, 0.85)",
              pointerEvents: "none",
            }}
          />
        ) : null}
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

/**
 * Zoom bounds, in pixels per second. The lower bound keeps a zoomed
 * view readable; the upper stops a one-frame selection from asking for
 * a scale no browser will draw.
 */
const MIN_ZOOM_PX_PER_SEC = 1;
const MAX_ZOOM_PX_PER_SEC = 2000;

/**
 * Vertical zoom bounds. 64x lifts a -36 dBFS passage to full height,
 * which covers the noise floors and fade tails this exists for; past
 * that the drawing is all clipping and no information.
 */
const MIN_VERTICAL_ZOOM = 1;
const MAX_VERTICAL_ZOOM = 64;

function pxToSeconds(px: number, totalPx: number, durationSec: number): number {
  if (totalPx <= 0) return 0;
  return clamp((px / totalPx) * durationSec, 0, durationSec);
}

/**
 * WaveSurfer 7 rejects a load that a newer one superseded. On a rapid
 * A/B toggle that is expected, so it must not reach the user as an
 * error (#246).
 */
function isAbort(err: unknown): boolean {
  if (err instanceof DOMException) return err.name === "AbortError";
  return /abort/i.test(String(err));
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
      mixPath,
      snapToZero,
      onSnapToZeroChange,
      syncLock,
      onSyncLockChange,
      verticalZoom,
      onVerticalZoomChange,
      playheadSec: playheadSecProp,
      onRenameTrack,
      onDuplicateTrack,
      onRemoveTrack,
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
    /**
     * How long lane 0's own audio decoded to. Still needed — it is the
     * only length available before any clip metadata arrives — but it
     * is not the session's length, and treating it as such is what
     * #171 was.
     */
    const [headLaneDuration, setHeadLaneDuration] = useState(0);

    /**
     * Playhead position in session seconds, published to every lane.
     *
     * Sourced from the one player that actually plays (lane 0's) but
     * *drawn* by each lane against the session axis, so seeking moves
     * every lane's playhead together — the thing #155 says is broken.
     * When the mix becomes the thing being played, this is the value
     * that changes and nothing else has to.
     */
    const [transportSec, setTransportSec] = useState(0);

    /**
     * A failure from the one thing that makes sound (#246).
     *
     * The lanes each surface their own load error; the mix player —
     * which is the transport, and the only audible source — swallowed
     * its rejection with a blanket `.catch`. When the mix WAV failed to
     * load, the lanes kept drawing normally and the only signal the
     * user got was that the app had gone mute.
     */
    const [mixError, setMixError] = useState<string | null>(null);

    const reportPlayFailure = useCallback((result: unknown) => {
      void Promise.resolve(result).catch((err: unknown) =>
        setMixError(String(err)),
      );
    }, []);

    /**
     * The one player. Hidden, because it has no waveform to show — the
     * lanes draw the picture and this makes the sound.
     */
    const mixWsRef = useRef<WaveSurfer | null>(null);
    const mixHostRef = useRef<HTMLDivElement>(null);
    const loopRef = useRef(loop);
    const selectionRef = useRef(selection);
    useEffect(() => {
      loopRef.current = loop;
    }, [loop]);
    useEffect(() => {
      selectionRef.current = selection;
    }, [selection]);

    useEffect(() => {
      if (!mixHostRef.current) return;
      const ws = WaveSurfer.create({
        container: mixHostRef.current,
        height: 1,
        cursorWidth: 0,
        // Never drawn, so nothing here is a visual decision.
        waveColor: "transparent",
        progressColor: "transparent",
      });
      mixWsRef.current = ws;

      const publish = () => setTransportSec(ws.getCurrentTime());
      ws.on("audioprocess", publish);
      ws.on("seeking", publish);
      ws.on("timeupdate", publish);

      // Looping belongs to whatever is actually playing. It used to
      // live on lane 0, which is no longer the thing making sound.
      const onProcess = () => {
        if (!loopRef.current || !selectionRef.current) return;
        if (ws.getCurrentTime() >= selectionRef.current.end) {
          ws.setTime(selectionRef.current.start);
        }
      };
      ws.on("audioprocess", onProcess);

      return () => {
        ws.un("audioprocess", publish);
        ws.un("seeking", publish);
        ws.un("timeupdate", publish);
        ws.un("audioprocess", onProcess);
        ws.destroy();
        mixWsRef.current = null;
      };
    }, []);

    // Load the mix when it changes. A null path means there is nothing
    // to play yet — a cold start with no head — and the transport
    // simply does nothing rather than throwing.
    useEffect(() => {
      const ws = mixWsRef.current;
      if (!ws || !mixPath) return;

      // Where we were, before the load takes it away (#246).
      //
      // WaveSurfer's `loadAudio()` pauses when playing, and `setSrc()`
      // reassigns `media.src`, which zeroes `currentTime`. A/B compare
      // swaps only the path, so every A→B click stopped playback and
      // dropped the playhead to 0 — making it impossible to compare the
      // same moment on both sides without manually re-seeking and
      // re-pressing Space.
      const resumeAt = ws.getCurrentTime();
      const wasPlaying = ws.isPlaying();

      setMixError(null);
      let superseded = false;

      try {
        void ws
          .load(convertFileSrc(mixPath))
          .then(() => {
            if (superseded) return;
            const duration = ws.getDuration() || 0;
            if (resumeAt > 0 && duration > 0) {
              ws.setTime(Math.min(resumeAt, duration));
            }
            if (wasPlaying) void ws.play().catch(() => undefined);
          })
          .catch((err: unknown) => {
            // A rapid A/B toggle aborts the previous load. That is the
            // system working, not a failure to report — and it is the
            // likely reason the blanket `.catch` was written in the
            // first place.
            if (superseded || isAbort(err)) return;
            setMixError(`Could not load the mix: ${String(err)}`);
          });
      } catch (err) {
        // A path the webview cannot convert. The lanes still draw, so
        // without this the app would simply be mute.
        setMixError(`Could not load the mix: ${String(err)}`);
      }

      return () => {
        superseded = true;
      };
    }, [mixPath]);
    const playheadSec = playheadSecProp ?? transportSec;

    /**
     * The session's length: the furthest point any clip on any track
     * reaches. This is the axis the ruler, the clip strip and every
     * range-taking tool agree on, so it is the one selection has to be
     * measured against.
     *
     * Falls back to lane 0's decoded duration when no clip metadata has
     * arrived yet — for a single loaded file the two are the same
     * number, which is exactly why the bug stayed invisible.
     */
    const timelineDuration = useMemo(() => {
      const end = (tracks ?? []).reduce((max, t) => {
        for (const c of t.clips ?? []) {
          const e = c.start_sec + c.length_sec;
          if (e > max) max = e;
        }
        return max;
      }, 0);
      return end > 0 ? end : headLaneDuration;
    }, [tracks, headLaneDuration]);

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

    /**
     * Width of the drawing surface, in pixels — the lane minus its head.
     * Read from the DOM rather than tracked in state: it changes with
     * the window and with the panel layout, and a stale number here
     * would frame the wrong region.
     */
    const paneWidth = useCallback(() => {
      const el = rootRef.current?.querySelector<HTMLElement>(
        "[data-testid='timeline-lane-waveform']",
      );
      return el?.clientWidth ?? 0;
    }, []);

    /**
     * Fill the pane with the selection. Along with fit-to-window these
     * are the two most-used zoom verbs on any timeline, and until now
     * getting to a selected region meant zooming with ± and then
     * scrolling to find it by hand.
     */
    const zoomToSelection = useCallback(() => {
      if (!selection || !onZoomChange) return;
      const span = selection.end - selection.start;
      const width = paneWidth();
      if (span <= 0 || width <= 0) return;

      const pxPerSec = clamp(width / span, MIN_ZOOM_PX_PER_SEC, MAX_ZOOM_PX_PER_SEC);
      onZoomChange(pxPerSec);

      // Scroll after the zoom has been applied — the scrollable width
      // does not exist until wavesurfer has redrawn at the new scale.
      requestAnimationFrame(() => {
        const surfaces = rootRef.current?.querySelectorAll<HTMLElement>(
          "[data-testid='timeline-lane-waveform']",
        );
        surfaces?.forEach((el) => {
          const scroller = el.parentElement;
          if (scroller) scroller.scrollLeft = selection.start * pxPerSec;
        });
      });
    }, [selection, onZoomChange, paneWidth]);

    /** Zero means auto-fit, which is what the lanes already do. */
    const fitToWindow = useCallback(() => {
      onZoomChange?.(0);
      const surfaces = rootRef.current?.querySelectorAll<HTMLElement>(
        "[data-testid='timeline-lane-waveform']",
      );
      surfaces?.forEach((el) => {
        if (el.parentElement) el.parentElement.scrollLeft = 0;
      });
    }, [onZoomChange]);

    useImperativeHandle(
      ref,
      () => ({
        togglePlay: () => {
          const ws = mixWsRef.current;
          if (!ws) return;
          if (ws.isPlaying()) ws.pause();
          // A rejected `play()` is how "nothing is decoded" reaches the
          // caller, and it was discarded — so pressing Space on a mix
          // that never loaded did nothing and said nothing (#246).
          //
          // Wrapped in `Promise.resolve` rather than chaining directly:
          // WaveSurfer types this as returning a promise, but a media
          // element's `play()` can return undefined on older engines,
          // and the transport must not throw on the way to reporting an
          // error.
          else reportPlayFailure(ws.play());
        },
        play: () => reportPlayFailure(mixWsRef.current?.play()),
        pause: () => mixWsRef.current?.pause(),
        seekTo: (sec: number) => {
          const ws = mixWsRef.current;
          if (!ws) return;
          const d = ws.getDuration() || 0;
          if (d <= 0) return;
          ws.setTime(clamp(sec, 0, d));
        },
        seekBy: (delta: number) => {
          const ws = mixWsRef.current;
          if (!ws) return;
          const d = ws.getDuration() || 0;
          if (d <= 0) return;
          ws.setTime(clamp(ws.getCurrentTime() + delta, 0, d));
        },
        getCurrentTime: () => mixWsRef.current?.getCurrentTime() ?? 0,
        getDuration: () => mixWsRef.current?.getDuration() ?? 0,
        zoomToSelection,
        fitToWindow,
      }),
      [zoomToSelection, fitToWindow],
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
        {mixError ? (
          <div
            data-testid="timeline-mix-error"
            role="alert"
            style={{
              margin: "8px 16px 0",
              background: "rgba(239,111,114,0.12)",
              border: "1px solid rgba(239,111,114,0.4)",
              borderRadius: 6,
              padding: "6px 10px",
              fontSize: 11,
              color: "var(--danger)",
            }}
          >
            {mixError}
          </div>
        ) : null}
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
              data-testid="vzoom-out-btn"
              onClick={() =>
                onVerticalZoomChange?.(
                  clamp(
                    (verticalZoom ?? 1) / 2,
                    MIN_VERTICAL_ZOOM,
                    MAX_VERTICAL_ZOOM,
                  ),
                )
              }
              disabled={(verticalZoom ?? 1) <= MIN_VERTICAL_ZOOM}
              className="text-xs px-1.5 py-1 rounded border border-neutral-600 text-neutral-400 hover:border-neutral-400 transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
              title="Shrink the waveform vertically"
            >
              ↕−
            </button>
            <button
              type="button"
              data-testid="vzoom-in-btn"
              onClick={() =>
                onVerticalZoomChange?.(
                  clamp(
                    (verticalZoom ?? 1) * 2,
                    MIN_VERTICAL_ZOOM,
                    MAX_VERTICAL_ZOOM,
                  ),
                )
              }
              disabled={(verticalZoom ?? 1) >= MAX_VERTICAL_ZOOM}
              className="text-xs px-1.5 py-1 rounded border border-neutral-600 text-neutral-400 hover:border-neutral-400 transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
              title="Magnify the waveform vertically — makes a quiet passage readable"
            >
              ↕+
            </button>
            <button
              type="button"
              data-testid="zoom-to-selection-btn"
              onClick={zoomToSelection}
              disabled={!selection}
              className="text-xs px-1.5 py-1 rounded border border-neutral-600 text-neutral-400 hover:border-neutral-400 transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
              title="Zoom to selection (Ctrl+E)"
            >
              ⇱⇲
            </button>
            <button
              type="button"
              data-testid="fit-to-window-btn"
              onClick={fitToWindow}
              className="text-xs px-1.5 py-1 rounded border border-neutral-600 text-neutral-400 hover:border-neutral-400 transition-colors"
              title="Fit to window (Ctrl+F)"
            >
              ⇤⇥
            </button>
            <button
              type="button"
              data-testid="sync-lock-btn"
              onClick={() => onSyncLockChange?.(!syncLock)}
              aria-pressed={syncLock ? "true" : "false"}
              aria-label={syncLock ? "Turn sync-lock off" : "Turn sync-lock on"}
              className={`text-xs px-2 py-1 rounded border transition-colors ${
                syncLock
                  ? "border-amber-400 text-amber-400 bg-amber-400/10"
                  : "border-neutral-600 text-neutral-400 hover:border-neutral-400"
              }`}
              title="Sync-lock — cuts and inserts move every track together, so a multitrack recording stays aligned"
            >
              ⛓
            </button>
            <button
              type="button"
              data-testid="snap-zero-btn"
              onClick={() => onSnapToZeroChange?.(!snapToZero)}
              aria-pressed={snapToZero ? "true" : "false"}
              className={`text-xs px-2 py-1 rounded border transition-colors ${
                snapToZero
                  ? "border-amber-400 text-amber-400 bg-amber-400/10"
                  : "border-neutral-600 text-neutral-400 hover:border-neutral-400"
              }`}
              title="Snap selection to zero crossings — avoids clicks at cut boundaries"
            >
              ⌇
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

        {/*
          The transport. One player for the whole session, on the
          rendered mix, drawn as nothing — the lanes are the picture.
        */}
        <div
          ref={mixHostRef}
          data-testid="timeline-mix-player"
          aria-hidden="true"
          style={{ position: "absolute", width: 0, height: 0, overflow: "hidden" }}
        />

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
                spectrogramEnabled={spectrogramEnabled}
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

                selection={idx === 0 ? selection : null}
                onSelectionChange={idx === 0 ? onSelectionChange : undefined}
                onDurationChange={idx === 0 ? setHeadLaneDuration : undefined}
                sessionDuration={timelineDuration}
                playheadSec={playheadSec}
                snapToZero={snapToZero}
                verticalZoom={verticalZoom}
                trackIndex={trackIndex(idx)}
                onRenameTrack={onRenameTrack}
                onDuplicateTrack={onDuplicateTrack}
                onRemoveTrack={onRemoveTrack}
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
