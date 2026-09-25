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
 * lane renders the audio at its own `audioPath`, which starts at
 * session zero (`list_tracks` flattens any track that is not a whole
 * file at zero).
 *
 * One axis: every row — ruler, lanes, clip strips, automation, markers,
 * labels — maps time to pixels through one viewport
 * (`lib/timelineViewport.ts`) and pans with one scrollbar (#344, #348).
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
import type { Marker } from "../lib/tauri-bridge";
import { snapRange } from "../lib/zeroCrossing";
import { AutomationLane } from "./AutomationLane";
import { TrackMenu } from "./TrackMenu";
import { ClipStrip } from "./ClipStrip";
import type { ClipSummary, EnvelopePoint } from "../lib/tauri-bridge";
import { Ruler } from "./Ruler";
import { MarkerLayer } from "./MarkerLayer";
import {
  clampScrollSec,
  maxScrollSec,
  pxPerSecFor,
  pxToSec,
  secToPx,
  visibleSpan,
  type TimeSpan,
  type Viewport,
} from "../lib/timelineViewport";

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
  /**
   * Raised when the head lane's audio fails to decode, and again with
   * `null` once a later load succeeds.
   *
   * The failure used to live only in this component, so the status bar
   * — which derives its state from "is there a path" — reported
   * `ready` next to the filename of a file that had 404'd, directly
   * under the error box saying so. One of the two had to be wrong, and
   * it was the one the user reads first.
   */
  onLoadErrorChange?: (error: string | null) => void;
  /**
   * A row drawn under the lanes, on the timeline's axis: given the
   * stretch of the session on screen (null until it can be measured),
   * it returns the row. The label lane lives here.
   *
   * A slot rather than a sibling of the timeline because a row outside
   * the timeline's scroll box does not share its width: a vertical
   * scrollbar on the lanes narrows them and not it, and the two axes
   * drift apart by the scrollbar's width.
   */
  belowLanes?: (span: TimeSpan | null) => React.ReactNode;
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
  /** Reports this lane's decode failure, and `null` once one succeeds. */
  onLoadErrorChange?: (error: string | null) => void;
  /**
   * The timeline's one axis (#344, #348): the session's length, the
   * density, and the time at the surface's left edge. The lane draws its
   * audio, its selection, its playhead and the time under the pointer on
   * this, and on nothing of its own.
   *
   * Every lane used to be its own axis. It stretched its own file across
   * its own pane and scrolled on its own, so it agreed with the ruler
   * only when its audio started at zero and ran the whole session.
   * Selection was measured against the lane's decoded length and handed
   * to `render_range` as session seconds (#171). Once zoomed, the
   * overlay, the playhead and a drag still mapped pixels as if the pane
   * held the whole session.
   *
   * The lane's audio starts at session zero — `list_tracks` hands over a
   * file on the session's axis — so second *t* of it is session second
   * *t*, and a lane only has to be drawn at the timeline's density and
   * scrolled to its left edge.
   */
  viewport: Viewport;
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
  onLoadErrorChange,
  viewport,
  snapToZero,
  verticalZoom,
  playheadSec,
  trackIndex,
  onRenameTrack,
  onDuplicateTrack,
  onRemoveTrack,
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
  // Where a drag started, as a session time: the time under the pointer
  // at the press, not a pixel, so the drag means the same thing however
  // the view moves under it.
  const dragStateRef = useRef<{
    originSec: number;
    rectLeft: number;
    rectWidth: number;
  } | null>(null);
  const loopRef = useRef(loop);
  const selectionRef = useRef(selection);
  // Read by the load-failure handler below. A ref rather than a
  // dependency because the load effect keys on `audioPath` alone:
  // adding a prop whose identity changes each render would reload the
  // audio on every render.
  const onDurationChangeRef = useRef(onDurationChange);
  useEffect(() => {
    loopRef.current = loop;
  }, [loop]);
  useEffect(() => {
    selectionRef.current = selection;
  }, [selection]);
  useEffect(() => {
    onDurationChangeRef.current = onDurationChange;
  }, [onDurationChange]);

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
      // Drawn at the timeline's density, never stretched to this pane:
      // filling the pane is what put a 1-second track across the same
      // width as a 3-second one (#348).
      fillParent: false,
      // Scrolled by the timeline, never on its own. The timeline has
      // the one scrollbar.
      hideScrollbar: true,
      autoScroll: false,
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

/**
 * Whether a rejection is "we cancelled this", not "this failed".
 *
 * WaveSurfer aborts an in-flight fetch when a new `load()` supersedes
 * it or the element goes away. That surfaces as a `DOMException` named
 * `AbortError` in some paths and as a bare string in others, so both
 * shapes are checked rather than trusting one.
 */
function isAbort(err: unknown): boolean {
  if (err && typeof err === "object" && "name" in err) {
    if ((err as { name?: unknown }).name === "AbortError") return true;
  }
  return /abort/i.test(String(err));
}

  // Report the lane's load state to the parent, which is what the
  // status bar reads. Mirrors `onDurationChange` — lane 0 is the one
  // the parent listens to.
  useEffect(() => {
    onLoadErrorChange?.(loadError);
  }, [loadError, onLoadErrorChange]);

  // Reload when audioPath changes.
  //
  // The rejection has to be tied to the load that produced it. Nothing
  // marked a load as superseded, so aborting one — by switching tabs,
  // or by opening a second file — landed its `AbortError` *after* the
  // next load had already cleared the error, and the stale failure
  // won. The result was a permanent red box reading "AbortError: Fetch
  // is aborted" sitting under a waveform that had decoded perfectly,
  // with no way to dismiss it. Found by opening a file, switching
  // Timeline → Transcript → Graph → Timeline, and opening it again.
  //
  // An abort is also not a user-facing condition in the first place:
  // it means *we* replaced this load, which is exactly when the error
  // must not be shown.
  // A file that is not audio never reaches either `catch` below.
  //
  // `loadAudio` sets the media source and then awaits a promise that
  // only ever resolves from `loadedmetadata`. An undecodable file makes
  // the media element fire `error` instead, so that promise is never
  // settled: `load()` neither resolves nor rejects, and a `.catch` —
  // or any check written to run *after* the load resolves — is dead
  // code on exactly the input it was meant to catch.
  //
  // WaveSurfer does report it, on a channel this lane never opened:
  // `initPlayerEvents` forwards the media element's `error` to its own
  // `error` event. That event is the only signal for this case, so it
  // is what the lane listens to. Both `catch`es stay — a fetch failure
  // rejects *and* emits, and setting the same message twice is a no-op.
  //
  // Subscribed inside this effect rather than at mount so the same
  // `current` flag covers it: an abort emits `error` too (`load()`
  // re-emits what it throws), and an abort belonging to a superseded
  // load must not surface under the load that replaced it.
  useEffect(() => {
    const ws = wsRef.current;
    if (!ws || !audioPath) return;
    let current = true;
    setLoadError(null);

    const fail = (err: unknown) => {
      if (!current || isAbort(err)) return;
      setLoadError(String(err));
      // The ruler, the clip strip and the selection are all drawn on
      // the duration this lane reports. Leaving the previous file's
      // duration standing is what put a 2.5-second scale under a file
      // that never decoded — the picture and the scale both have to
      // stop describing audio that is not there.
      setDuration(0);
      onDurationChangeRef.current?.(0);
    };

    ws.on("error", fail);
    try {
      const url = convertFileSrc(audioPath);
      ws.load(url).catch(fail);
    } catch (err) {
      fail(err);
    }
    return () => {
      current = false;
      ws.un("error", fail);
    };
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

  /**
   * Draw at the timeline's density and show its window.
   *
   * Through WaveSurfer, because WaveSurfer is what draws and what
   * scrolls: it renders into a `.scroll` container of its own, inside a
   * shadow root. The wrapper it draws into is widened to the whole
   * session, so a lane whose audio ends early can still scroll to a time
   * after it — and shows nothing there, which is the truth.
   *
   * One effect, in this order, because each step is measured on the one
   * before: `zoom()` redraws synchronously at the new width, the minimum
   * width lets the container scroll that far, and only then does the
   * scroll land where it should.
   */
  const pxPerSec = viewport.pxPerSec;
  const sessionPx = Math.ceil(viewport.durationSec * pxPerSec);
  const scrollPx = Math.round(viewport.scrollSec * pxPerSec);
  const scrollPxRef = useRef(scrollPx);
  const zoomedToRef = useRef(0);
  useEffect(() => {
    scrollPxRef.current = scrollPx;
    const ws = wsRef.current;
    if (!ws || duration === 0 || !(pxPerSec > 0)) return;
    if (zoomedToRef.current !== pxPerSec) {
      ws.zoom(pxPerSec);
      zoomedToRef.current = pxPerSec;
    }
    ws.getWrapper().style.minWidth = `${sessionPx}px`;
    ws.setScroll(scrollPx);
  }, [pxPerSec, sessionPx, scrollPx, duration]);

  /**
   * Keep the window where the timeline put it.
   *
   * WaveSurfer moves its own scroll after every redraw: back to zero when
   * its audio fits the pane, and by however far the waveform grew
   * otherwise. Either is right for a lone waveform and wrong for a lane
   * on a shared axis, so after each redraw, and each scroll it makes,
   * the lane goes back to the timeline's window. Setting the same value
   * again emits nothing, so this settles at once.
   */
  useEffect(() => {
    const ws = wsRef.current;
    if (!ws) return;
    const hold = () => {
      if (Math.abs(ws.getScroll() - scrollPxRef.current) > 1) {
        ws.setScroll(scrollPxRef.current);
      }
    };
    ws.on("redrawcomplete", hold);
    ws.on("scroll", hold);
    return () => {
      ws.un("redrawcomplete", hold);
      ws.un("scroll", hold);
    };
  }, []);

  const handleDrop = useCallback(
    (e: React.DragEvent<HTMLDivElement>) => {
      e.preventDefault();
      setIsDragging(false);
      const file = e.dataTransfer.files?.[0];
      if (!file) return;
      const path = (file as File & { path?: string }).path;
      if (!path) {
        setLoadError("Could not resolve absolute path for the dropped file.");
        return;
      }
      // Loaded by the app, straight into the session — not sent to the
      // agent as a sentence, which needs a working model to mean
      // anything (#321).
      onFileDropped?.(path);
    },
    [onFileDropped],
  );

  const beginSelection = useCallback(
    (e: React.MouseEvent<HTMLDivElement>) => {
      if (!onSelectionChange || !(viewport.pxPerSec > 0) || !waveformWrapperRef.current) return;
      // Only left-click; Shift is reserved for multi-select later.
      if (e.button !== 0) return;
      const rect = waveformWrapperRef.current.getBoundingClientRect();
      const originSec = pxToSec(e.clientX - rect.left, viewport);
      dragStateRef.current = {
        originSec,
        rectLeft: rect.left,
        rectWidth: rect.width,
      };
      setDraftSelection({ start: originSec, end: originSec });
    },
    [viewport, onSelectionChange],
  );

  /**
   * Move the committed edges onto zero crossings, when asked to and
   * when it is this lane's audio the selection is over.
   *
   * That second condition is not fussiness. Selection is measured on
   * the session axis (#171). This lane's audio starts at session zero,
   * so its samples are the session's for as long as it runs — and past
   * its end there are none. Snapping against a buffer that does not
   * hold the selection would move the boundary to a crossing that is
   * not where the user is cutting — worse than not snapping, and
   * invisible. So a selection that runs past this lane's audio is left
   * alone, which is the behaviour that existed before the toggle.
   */
  const maybeSnap = useCallback(
    (range: Selection): Selection => {
      if (!snapToZero) return range;
      const ws = wsRef.current;
      if (!ws) return range;
      if (!(duration > 0) || range.end > duration + 1e-6) return range;

      const decoded = ws.getDecodedData?.();
      if (!decoded) return range;
      const channel = decoded.getChannelData(0);
      if (!channel?.length) return range;

      return snapRange(channel, decoded.sampleRate, range);
    },
    [snapToZero, duration],
  );

  useEffect(() => {
    if (!draftSelection) return;
    const onMove = (e: MouseEvent) => {
      const drag = dragStateRef.current;
      if (!drag) return;
      const px = clamp(e.clientX - drag.rectLeft, 0, drag.rectWidth);
      const tEnd = pxToSec(px, viewport);
      setDraftSelection({
        start: Math.min(drag.originSec, tEnd),
        end: Math.max(drag.originSec, tEnd),
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
  }, [draftSelection, viewport, onSelectionChange, maybeSnap]);

  /**
   * Where the playhead sits on this lane, in pixels from the surface's
   * left edge, or null when it is off screen or there is nothing to draw.
   *
   * On the timeline's axis, so a seek moves every lane's playhead to the
   * same place — including lanes whose own audio is shorter than the
   * session, which is exactly where WaveSurfer's clamped cursor gave
   * the wrong answer — and a zoomed lane puts it on the audio playing.
   */
  const playhead = useMemo(() => {
    if (playheadSec === undefined || !(viewport.pxPerSec > 0)) return null;
    const x = secToPx(clamp(playheadSec, 0, viewport.durationSec), viewport);
    return x >= 0 && x <= viewport.widthPx ? x : null;
  }, [playheadSec, viewport]);

  /**
   * The selection on screen: clipped to the surface, and null when none
   * of it is in view.
   */
  const overlay = useMemo(() => {
    const range = draftSelection ?? selection ?? null;
    if (!range || !(viewport.pxPerSec > 0)) return null;
    const left = Math.max(0, secToPx(range.start, viewport));
    const right = Math.min(viewport.widthPx, secToPx(range.end, viewport));
    if (right <= left) return null;
    return { left, width: right - left };
  }, [draftSelection, selection, viewport]);

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
        data-testid="timeline-lane-surface"
        onMouseDown={beginSelection}
        style={{
          flex: 1,
          minWidth: 0,
          position: "relative",
          // The lane's time surface: the same left edge and width as
          // every other row's, so one axis maps them all. No horizontal
          // padding, which the ruler and the clip rows never had, and
          // nothing drawn past its edges.
          overflow: "hidden",
          background: isDragging ? "var(--accent-soft)" : "var(--surface-elev)",
          padding: "10px 0",
          boxShadow: isDragging ? "inset 0 0 0 1px var(--accent)" : "none",
          transition: "background 160ms ease, box-shadow 160ms ease",
          cursor: viewport.durationSec > 0 ? "crosshair" : "default",
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
            //
            // A failed load hides it for a second reason. WaveSurfer
            // keeps the last decoded waveform drawn, so the lane went
            // on showing the previous file under the new file's name.
            // `empty()` would clear it properly, but `empty()` is
            // `load('', [[0]], 0.001)` — re-entering `load` from the
            // error handler that `load` just called, which can emit
            // `error` again. Not drawing a picture we know to be of
            // the wrong file is the honest half of that trade.
            visibility:
              spectrogramEnabled || loadError ? "hidden" : "visible",
          }}
        />
        <div
          ref={spectrogramHostRef}
          data-testid="timeline-lane-spectrogram"
          style={{
            position: "absolute",
            top: 10,
            left: 0,
            right: 0,
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
              left: playhead,
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
              left: overlay.left,
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

/**
 * WaveSurfer 7 rejects a load that a newer one superseded. On a rapid
 * A/B toggle that is expected, so it must not reach the user as an
 * error (#246).
 */
function isAbort(err: unknown): boolean {
  if (err instanceof DOMException) return err.name === "AbortError";
  return /abort/i.test(String(err));
}

/**
 * How long an A/B switch crossfades, in milliseconds. Long enough that
 * the cut makes no click; short enough that the ear hears the new side
 * at once, which is what makes it a comparison.
 */
const CROSSFADE_MS = 60;

interface Crossfade {
  /** Jump to the end: the incoming side at full level, the other stopped. */
  finish: () => void;
}

/**
 * Cross `from` out and `to` in over `ms`, then stop `from`.
 *
 * Equal-power (cos/sin), so the pair holds its loudness through the
 * middle of the fade instead of dipping, as a linear fade would. Stepped
 * on a short timer rather than animation frames: a frame is 16 ms, four
 * steps in a 60 ms fade, and a step that coarse is itself audible.
 */
function crossfade(from: WaveSurfer, to: WaveSurfer, ms: number): Crossfade {
  const start = performance.now();
  let timer: ReturnType<typeof setTimeout> | undefined;
  let done = false;
  const finish = () => {
    if (done) return;
    done = true;
    clearTimeout(timer);
    to.setVolume(1);
    from.pause();
    // Back to full level, ready for when it is the incoming side.
    from.setVolume(1);
  };
  const step = () => {
    const p = Math.min(1, (performance.now() - start) / ms);
    from.setVolume(Math.cos((p * Math.PI) / 2));
    to.setVolume(Math.sin((p * Math.PI) / 2));
    if (p >= 1) finish();
    else timer = setTimeout(step, 4);
  };
  step();
  return { finish };
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
      onLoadErrorChange,
      belowLanes,
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
     * The players. Hidden, because they have no waveform to show — the
     * lanes draw the picture and these make the sound.
     *
     * Two, so switching the mix can crossfade (#269 §2). An A/B switch
     * used to reload the one player in place: the old side stopped, the
     * new one loaded, and playback resumed with a gap and a hard cut —
     * the click the comparison plan promised would not be there. Now the
     * incoming side loads on the idle player while the outgoing one plays
     * on, starts at the same moment at silence, and the two cross over
     * `CROSSFADE_MS` before the old one stops.
     *
     * Only the active player drives the transport; the other is either
     * silent or on its way out.
     */
    const mixPlayersRef = useRef<WaveSurfer[]>([]);
    const activeMixRef = useRef(0);
    const activeMix = useCallback(
      (): WaveSurfer | null => mixPlayersRef.current[activeMixRef.current] ?? null,
      [],
    );
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
      const host = mixHostRef.current;
      if (!host) return;
      const unsubscribe: (() => void)[] = [];
      const players = [0, 1].map((side) => {
        // An element of our own, in the page, rather than one WaveSurfer
        // keeps detached: the same audio either way, and something a
        // test in a real browser can find and read.
        const media = document.createElement("audio");
        media.dataset.mixSide = String(side);
        host.appendChild(media);
        const ws = WaveSurfer.create({
          container: host,
          media,
          height: 1,
          cursorWidth: 0,
          // Never drawn, so nothing here is a visual decision.
          waveColor: "transparent",
          progressColor: "transparent",
        });

        const isActive = () => mixPlayersRef.current[activeMixRef.current] === ws;
        const publish = () => {
          if (isActive()) setTransportSec(ws.getCurrentTime());
        };
        // Looping belongs to whatever is actually playing. It used to
        // live on lane 0, which is no longer the thing making sound.
        const onProcess = () => {
          if (!isActive() || !loopRef.current || !selectionRef.current) return;
          if (ws.getCurrentTime() >= selectionRef.current.end) {
            ws.setTime(selectionRef.current.start);
          }
        };
        ws.on("audioprocess", publish);
        ws.on("seeking", publish);
        ws.on("timeupdate", publish);
        ws.on("audioprocess", onProcess);
        unsubscribe.push(() => {
          ws.un("audioprocess", publish);
          ws.un("seeking", publish);
          ws.un("timeupdate", publish);
          ws.un("audioprocess", onProcess);
          ws.destroy();
          media.remove();
        });
        return ws;
      });
      mixPlayersRef.current = players;
      activeMixRef.current = 0;

      return () => {
        for (const u of unsubscribe) u();
        mixPlayersRef.current = [];
      };
    }, []);

    // Load the mix when it changes, onto the idle player, and hand the
    // transport over once it is ready. A null path means there is nothing
    // to play yet — a cold start with no head — and the transport simply
    // does nothing rather than throwing.
    useEffect(() => {
      const players = mixPlayersRef.current;
      if (players.length < 2 || !mixPath) return;
      const fromIndex = activeMixRef.current;
      const toIndex = 1 - fromIndex;
      const from = players[fromIndex];
      const to = players[toIndex];

      setMixError(null);
      let superseded = false;
      let fade: Crossfade | null = null;

      try {
        void to
          .load(convertFileSrc(mixPath))
          .then(() => {
            if (superseded) return;
            // Where the outgoing side is *now*, not where it was when the
            // switch began: it kept playing while this side loaded, and
            // the point of an A/B switch is to hear the same moment on
            // both (#246).
            const at = from.getCurrentTime();
            const duration = to.getDuration() || 0;
            if (at > 0 && duration > 0) to.setTime(Math.min(at, duration));

            activeMixRef.current = toIndex;
            setTransportSec(to.getCurrentTime());
            if (!from.isPlaying()) {
              to.setVolume(1);
              return;
            }
            to.setVolume(0);
            void to.play().catch(() => undefined);
            fade = crossfade(from, to, CROSSFADE_MS);
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
        // A switch that arrives mid-fade finishes this one at once, so
        // the next starts from one player playing at full level.
        fade?.finish();
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

    /**
     * Width of a row's time surface, in pixels: every row's, since they
     * all share one left edge and one width. Measured on the scrollbar
     * row, which is always drawn, and followed as the window and the
     * panel layout change.
     */
    const surfaceRef = useRef<HTMLDivElement>(null);
    const [surfaceWidth, setSurfaceWidth] = useState(0);
    useLayoutEffect(() => {
      const el = surfaceRef.current;
      if (!el) return;
      const measure = () => setSurfaceWidth(el.clientWidth);
      measure();
      if (typeof ResizeObserver === "function") {
        const observer = new ResizeObserver(measure);
        observer.observe(el);
        return () => observer.disconnect();
      }
      window.addEventListener("resize", measure);
      return () => window.removeEventListener("resize", measure);
    }, []);

    /**
     * The session time at the surfaces' left edge, as the user last set
     * it. Kept as a time, not pixels, so a zoom keeps the same moment at
     * the left edge. Clamped where it is read, since what is reachable
     * depends on the zoom and the width.
     */
    const [scrollSecWanted, setScrollSecWanted] = useState(0);

    const viewport = useMemo<Viewport>(() => {
      const pxPerSec = pxPerSecFor(zoom ?? 0, surfaceWidth, timelineDuration);
      const base = { durationSec: timelineDuration, widthPx: surfaceWidth, pxPerSec };
      return { ...base, scrollSec: clampScrollSec(scrollSecWanted, base) };
    }, [zoom, surfaceWidth, timelineDuration, scrollSecWanted]);
    const span = useMemo(() => visibleSpan(viewport), [viewport]);

    /**
     * Pan by `px` pixels. Returns whether there was anywhere to go, so a
     * wheel that cannot pan is left to scroll the page.
     */
    const panBy = useCallback(
      (px: number): boolean => {
        if (!(viewport.pxPerSec > 0) || maxScrollSec(viewport) <= 0) return false;
        setScrollSecWanted(clampScrollSec(viewport.scrollSec + px / viewport.pxPerSec, viewport));
        return true;
      },
      [viewport],
    );

    useEffect(() => {
      const el = rootRef.current;
      if (!el) return;
      const handler = (e: WheelEvent) => {
        if (e.ctrlKey) {
          e.preventDefault();
          const delta = e.deltaY > 0 ? -20 : 20;
          onZoomChange?.(Math.max(0, (zoom ?? 0) + delta));
          return;
        }
        // A horizontal gesture, or Shift with a vertical wheel, pans the
        // whole timeline. Lanes cannot scroll themselves — they are
        // drawn under the pointer, not scrolled by it — so this is the
        // only way a trackpad reaches the timeline's one scroll.
        const horizontal = Math.abs(e.deltaX) > Math.abs(e.deltaY);
        if (!horizontal && !e.shiftKey) return;
        const raw = horizontal ? e.deltaX : e.deltaY;
        const unit =
          e.deltaMode === WheelEvent.DOM_DELTA_LINE
            ? 16
            : e.deltaMode === WheelEvent.DOM_DELTA_PAGE
              ? viewport.widthPx
              : 1;
        if (panBy(raw * unit)) e.preventDefault();
      };
      el.addEventListener("wheel", handler, { passive: false });
      return () => el.removeEventListener("wheel", handler);
    }, [zoom, onZoomChange, panBy, viewport.widthPx]);

    /**
     * The zoom level the ± buttons should step from.
     *
     * `zoom` is 0 while the view is auto-fitted, and 0 is not a
     * pixels-per-second the user is looking at — it means "whatever
     * fills the pane". Stepping from a literal 50 made the first
     * presses land *below* the fitted density on short audio, so the
     * waveform stayed exactly as wide as the pane and the button
     * looked dead for four clicks before anything moved.
     *
     * Reading the real density makes the first press visible, whatever
     * the file length. Falls back to 50 only when there is nothing to
     * measure.
     */
    const effectivePxPerSec = useCallback(
      () => (viewport.pxPerSec > 0 ? viewport.pxPerSec : 50),
      [viewport.pxPerSec],
    );

    /**
     * Fill the pane with the selection. Along with fit-to-window these
     * are the two most-used zoom verbs on any timeline, and until now
     * getting to a selected region meant zooming with ± and then
     * scrolling to find it by hand.
     */
    const zoomToSelection = useCallback(() => {
      if (!selection || !onZoomChange) return;
      const span = selection.end - selection.start;
      const width = surfaceWidth;
      if (span <= 0 || width <= 0) return;

      const pxPerSec = clamp(width / span, MIN_ZOOM_PX_PER_SEC, MAX_ZOOM_PX_PER_SEC);
      onZoomChange(pxPerSec);

      // Centred, which is the same as starting at the selection whenever
      // the selection fills the pane. It differs only when the zoom hit
      // its limit: a selection too short to fill the pane at 2000 px/s
      // would otherwise sit against the left edge.
      //
      // One scroll for the whole timeline, so every lane — including one
      // added afterwards — shows this window until the user moves it.
      const visibleSec = width / pxPerSec;
      setScrollSecWanted(Math.max(0, selection.start + span / 2 - visibleSec / 2));
    }, [selection, onZoomChange, surfaceWidth]);

    /** Zero means auto-fit: the whole session across the pane. */
    const fitToWindow = useCallback(() => {
      onZoomChange?.(0);
      setScrollSecWanted(0);
    }, [onZoomChange]);

    /**
     * The timeline's scrollbar, kept on the viewport. It is also how a
     * user drags the view, so a scroll it reports that the viewport does
     * not already hold is a pan. Setting it to the viewport's own value
     * reports that value back, which changes nothing.
     */
    const hscrollRef = useRef<HTMLDivElement>(null);
    const scrollPx = viewport.scrollSec * viewport.pxPerSec;
    useLayoutEffect(() => {
      const el = hscrollRef.current;
      if (el && Math.abs(el.scrollLeft - scrollPx) > 0.5) el.scrollLeft = scrollPx;
    }, [scrollPx, viewport.pxPerSec, timelineDuration]);
    const onScrollbar = useCallback(
      (e: React.UIEvent<HTMLDivElement>) => {
        const left = e.currentTarget.scrollLeft;
        if (!(viewport.pxPerSec > 0) || Math.abs(left - scrollPx) <= 0.5) return;
        setScrollSecWanted(left / viewport.pxPerSec);
      },
      [viewport.pxPerSec, scrollPx],
    );

    useImperativeHandle(
      ref,
      () => ({
        togglePlay: () => {
          const ws = activeMix();
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
        play: () => reportPlayFailure(activeMix()?.play()),
        pause: () => activeMix()?.pause(),
        seekTo: (sec: number) => {
          const ws = activeMix();
          if (!ws) return;
          const d = ws.getDuration() || 0;
          if (d <= 0) return;
          ws.setTime(clamp(sec, 0, d));
        },
        seekBy: (delta: number) => {
          const ws = activeMix();
          if (!ws) return;
          const d = ws.getDuration() || 0;
          if (d <= 0) return;
          ws.setTime(clamp(ws.getCurrentTime() + delta, 0, d));
        },
        getCurrentTime: () => activeMix()?.getCurrentTime() ?? 0,
        getDuration: () => activeMix()?.getDuration() ?? 0,
        zoomToSelection,
        fitToWindow,
      }),
      [zoomToSelection, fitToWindow, activeMix, reportPlayFailure],
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
          overflow: "hidden",
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
                onZoomChange?.(
                  Math.max(10, Math.round(effectivePxPerSec() / 1.5)),
                )
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
                // This read `zoom ?? 50`, and `??` only falls back on
                // null/undefined — so the auto-fit `0` was kept and
                // `0 * 1.5` is `0`. Zoom-in mapped the default state
                // to itself: the button did nothing at all, forever,
                // unless you first pressed zoom-out (which escapes via
                // `Math.max(10, …)`). Found by clicking `+` in the
                // running app and diffing the waveform — three
                // presses, zero pixels changed.
                onZoomChange?.(
                  Math.min(500, Math.round(effectivePxPerSec() * 1.5)),
                )
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

        {/*
          Every row below shares one time surface: the same left edge,
          after the 132px head, and the same width. So they sit in one
          vertical scroll box — a scrollbar that narrowed the lanes and
          not the ruler would put them on different axes — with the
          ruler and the timeline's scrollbar held at its top and bottom.
        */}
        <div
          data-testid="timeline-body"
          style={{
            flex: 1,
            minHeight: 0,
            display: "flex",
            flexDirection: "column",
            overflowX: "hidden",
            overflowY: "auto",
          }}
        >
          <div style={{ position: "sticky", top: 0, zIndex: 2, flexShrink: 0 }}>
            <Ruler duration={timelineDuration} view={span} onAddMarker={onAddMarker} />
          </div>

          <div style={{ position: "relative", flex: 1 }}>
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
                  onLoadErrorChange={idx === 0 ? onLoadErrorChange : undefined}
                  viewport={viewport}
                  playheadSec={playheadSec}
                  snapToZero={snapToZero}
                  verticalZoom={verticalZoom}
                  trackIndex={trackIndex(idx)}
                  onRenameTrack={onRenameTrack}
                  onDuplicateTrack={onDuplicateTrack}
                  onRemoveTrack={onRemoveTrack}
                  loop={idx === 0 ? loop : undefined}
                />
                {onMoveClip && (track.clips?.length ?? 0) > 0 && (
                  <ClipStrip
                    trackName={track.name}
                    clips={track.clips ?? []}
                    duration={timelineDuration}
                    view={span}
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
                    view={span}
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
                view={span}
                onSeek={(t) => onSeekToMarker?.(t)}
                onRemove={(id) => onRemoveMarker?.(id)}
              />
            )}
          </div>

          {belowLanes?.(span)}

          {/*
            The timeline's one horizontal scrollbar. Its content is the
            session at the current density, so its range is exactly the
            viewport's; every lane, the ruler and the clip rows follow it.
          */}
          <div
            style={{
              position: "sticky",
              bottom: 0,
              zIndex: 2,
              display: "flex",
              flexShrink: 0,
              background: "var(--surface-elev)",
              borderTop: "1px solid var(--border)",
            }}
          >
            <div style={{ width: 132, flexShrink: 0, borderRight: "1px solid var(--border)" }} />
            <div ref={surfaceRef} style={{ flex: 1, minWidth: 0 }}>
              <div
                ref={hscrollRef}
                data-testid="timeline-hscroll"
                aria-label="Scroll the timeline"
                onScroll={onScrollbar}
                style={{ overflowX: "auto", overflowY: "hidden", height: 12 }}
              >
                <div
                  style={{
                    width: Math.ceil(timelineDuration * viewport.pxPerSec),
                    height: 1,
                  }}
                />
              </div>
            </div>
          </div>
        </div>
      </div>
    );
  },
);
