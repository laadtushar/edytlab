/**
 * App — top-level layout (Studio Onyx redesign).
 *
 * Three rows:
 *   1. AppHeader   — wordmark, view tabs, primary actions, settings
 *   2. main grid   — 70% Timeline/GraphView · 30% Chat
 *   3. StatusBar   — current head + model hint
 *
 * Cross-pane state stays minimal (audio path, head pointer, compare
 * mode). Errors surface as a structured `ErrorBanner` above the work
 * area instead of a fixed-position toast — when the error mentions
 * a missing API key we attach an "Open Settings" CTA so the user has
 * a one-click recovery path.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type {
  EnvelopePoint,
  Marker,
  TrackSummary,
  TranscriptWord,
} from "./lib/tauri-bridge";
import {
  addMarker,
  getNode,
  listMarkers,
  getTranscript,
  cutTranscriptWords,
  isNoSession,
  listTracks,
  getSyncLock,
  setSyncLock,
  onMarkerChanged,
  removeMarker,
  updateMarker,
  renderRange,
  setHeadTo,
  moveClip,
  removeClip,
  setClipEnvelope,
  setSelectionContext,
  duplicateTrack,
  removeTrack,
  renameTrack,
  setTrackGain,
  setTrackMuted,
  setTrackPan,
  setTrackSoloed,
} from "./lib/tauri-bridge";
import { save } from "@tauri-apps/plugin-dialog";
import {
  applyUndo,
  applyRedo,
  isUndoChord,
  isRedoChord,
} from "./lib/undoRedo";
import { mixIsStale } from "./lib/mixState";
import { scheduledTake, startTake, stopTake } from "./lib/recording";

import { ABCompareBar } from "./components/ABCompareBar";
import { Chat } from "./components/Chat";
import { LabelLane } from "./components/LabelLane";
import { ToolProgressBar } from "./components/ToolProgressBar";
import { TranscriptPane } from "./components/TranscriptPane";
import { TemplatePickerModal } from "./components/TemplatePickerModal";
import type { ChatHandle } from "./components/Chat";
import { CommandPalette } from "./components/CommandPalette";
import { AppHeader } from "./components/AppHeader";
import { EmptyState } from "./components/EmptyState";
import { ErrorBanner } from "./components/ErrorBanner";
import { GraphView } from "./components/GraphView";
import { Settings } from "./components/Settings";
import { ShortcutsOverlay } from "./components/ShortcutsOverlay";
import {
  Timeline,
  type Selection,
  type TimelineHandle,
} from "./components/Timeline";
import { useSession } from "./hooks/useSession";
import {
  hasApiKey,
  installBundledSkills,
  onNodeCreated,
  onToolProgress,
  renderPreview as bridgeRenderPreview,
} from "./lib/tauri-bridge";
import { listTemplates, applyTemplate, startRecording, stopRecording, timerRecord } from "./lib/tauri-bridge";
import type { TemplateInfo } from "./components/TemplatePickerModal";
import {
  listenToFileDrops,
  pickAudioFiles,
  pickProjectDirectory,
} from "./lib/file-open";
import { batchLoad } from "./lib/tauri-bridge";
import {
  forgetRecentProject,
  getViewState,
  listRecentProjects,
  openProject,
  saveProjectAs,
  saveViewState,
  type RecentProject,
} from "./lib/tauri-bridge";
import { type ViewToApply, viewToApply, viewToSave } from "./lib/viewState";

import type { LeftView } from "./lib/views";

interface CompareMode {
  a: string;
  b: string;
}

/**
 * Decide whether an error message should surface an "Open Settings"
 * CTA. The Rust side uses these exact substrings for the
 * agent-not-configured / api-key family of errors; keep the heuristic
 * loose so future variants still trigger the same recovery flow.
 */
function isApiKeyError(message: string): boolean {
  const m = message.toLowerCase();
  return (
    m.includes("set_api_key") ||
    m.includes("api key") ||
    m.includes("agent") ||
    m.includes("no agent")
  );
}

function App() {
  const { renderHead, head, setHeadLocal } = useSession();
  // Two different things used to share one variable, and the collision
  // is why the mixer is inaudible (#155).
  //
  // A *source* file — a track's own audio (`timelineSource`, below) — has
  // no mixer state applied and is only ever right for drawing a lane or
  // naming the session in the status bar.
  //
  // `mixPath` is the output of `render_preview` for a specific node: the
  // mix, with gain, pan, mute, solo, chains, sends and the master chain
  // in it. It is the only thing that should ever be *played*.
  //
  // Merged, `onNodeCreated` overwrote the mix with a raw track path after
  // every agent turn, so the mix was correct for about one render and
  // then quietly was not.
  const [mixPath, setMixPath] = useState<string | null>(null);
  const [mixNodeId, setMixNodeId] = useState<string | null>(null);
  const [rendering, setRendering] = useState(false);
  const [renderError, setRenderError] = useState<string | null>(null);
  const [leftView, setLeftView] = useState<LeftView>("timeline");
  const [graphRefresh, setGraphRefresh] = useState(0);
  const [keyConfigured, setKeyConfigured] = useState<boolean | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [showShortcuts, setShowShortcuts] = useState(false);
  const [compareMode, setCompareMode] = useState<CompareMode | null>(null);
  const [selection, setSelection] = useState<Selection | null>(null);
  const timelineRef = useRef<TimelineHandle>(null);
  const [markers, setMarkers] = useState<Marker[]>([]);
  const [tracks, setTracks] = useState<TrackSummary[]>([]);
  /**
   * The one way the track list is refreshed (#341, #342).
   *
   * - **The latest request wins.** Boot lists the tracks, and so does
   *   every edit; nothing ordered the replies, so a boot reply arriving
   *   after an edit's replaced the newer list with the older one. Each
   *   request is numbered, and only the newest one's reply is applied.
   * - **`NoSession` is an empty list**, not an error: it is what a new
   *   project with no history answers. "New project…" on an empty folder
   *   showed it as a failure.
   *
   * Any other failure is thrown to the caller, which reports it — unless
   * a newer request has been made since, in which case it is moot.
   */
  const trackRequestRef = useRef(0);
  const refreshTracks = useCallback(async (): Promise<void> => {
    const request = ++trackRequestRef.current;
    let next: TrackSummary[];
    try {
      next = await listTracks();
    } catch (err) {
      if (request !== trackRequestRef.current) return;
      if (!isNoSession(err)) throw err;
      next = [];
    }
    if (request !== trackRequestRef.current) return;
    setTracks(next);
  }, []);
  /**
   * What the timeline draws and the status bar names: the session's own
   * audio, whichever track holds it.
   *
   * Only ever the session's. It also used to fall back to a file the user
   * had just picked, drawn before anything had loaded it — so with no
   * working model the waveform and "ready" showed over an empty session
   * (#321). Opening a file now loads it before anything is drawn.
   *
   * Derived on every render rather than copied at each place tracks
   * arrive. A copy has to be remembered, and it was forgotten: boot and
   * opening a project listed their tracks and left "Drop a file or pick
   * one to begin" over them (#332), and so did recording into a fresh
   * project, because a helper that three call sites used was bypassed
   * by the rest. It also read only track 0, so a project whose first
   * track is empty hid the audio on its second. A value computed from
   * `tracks` cannot be skipped by any path that sets them.
   */
  const timelineSource = useMemo(
    () => tracks.find((t) => t.audio_path)?.audio_path ?? null,
    [tracks],
  );
  const selectionTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [zoomPxPerSec, setZoomPxPerSec] = useState(0);
  // Whether the head lane's audio actually decoded. The status bar
  // used to infer "ready" from a path alone — from a path having been
  // *chosen* — so it reported ready for a file that 404'd.
  const [audioLoadError, setAudioLoadError] = useState<string | null>(null);
  const [redoStack, setRedoStack] = useState<string[]>([]);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const chatRef = useRef<ChatHandle>(null);
  const [exporting, setExporting] = useState(false);
  const [loopActive, setLoopActive] = useState(false);
  const [spectrogramEnabled, setSpectrogramEnabled] = useState(false);
  // Off by default: snapping changes where an edit lands, and the
  // behaviour that existed is the one a user is not surprised by.
  const [snapToZero, setSnapToZero] = useState(false);
  // Sync-lock is session state, not a UI preference: undo can turn it
  // back off and a project can open with it already on, so this mirror
  // is re-read from the session rather than owned here.
  const [syncLock, setSyncLockState] = useState(false);
  // 1 = the samples at their real amplitude, which is where the lanes
  // have always been.
  const [verticalZoom, setVerticalZoom] = useState(1);
  const [templates, setTemplates] = useState<TemplateInfo[]>([]);
  const [showTemplatePicker, setShowTemplatePicker] = useState(false);
  const [isRecording, setIsRecording] = useState(false);
  // Projects this machine has opened. Empty on a first launch, and the
  // empty state hides the list entirely rather than showing a heading
  // with nothing under it.
  const [recents, setRecents] = useState<RecentProject[]>([]);
  const viewSaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // False until the saved view has been read at boot. Nothing may write
  // the view before that: once a returning user has a head, the save
  // fired 500 ms after launch and wrote zoom 0, no selection and
  // playhead 0 over the view they left, before anything had read it.
  const viewRestoredRef = useRef(false);
  // The playhead as last read from, or written to, `view.json`. The live
  // playhead belongs to the mix player, which holds nothing until a
  // preview is rendered, so `getCurrentTime()` answers 0 there. Saving
  // that would write a position nobody chose over one somebody did.
  const lastPlayheadRef = useRef(0);

  const handleUndo = useCallback(async () => {
    if (!head) return;
    try {
      const node = await getNode(head);
      const result = applyUndo(head, node.parent ?? null, redoStack);
      if (!result) return;
      await setHeadTo(result.head);
      setHeadLocal(result.head);
      setRedoStack(result.redoStack);
      await refreshTracks();
    } catch (err) {
      setRenderError(String(err));
    }
  }, [head, redoStack, setHeadLocal]);

  // Whenever the head moves — an edit, an undo, a project opening — the
  // toggle re-reads the session rather than trusting what it last set.
  // Undo past a `set_sync_lock` node is exactly the case a local guess
  // gets wrong, and it gets it wrong silently.
  useEffect(() => {
    let cancelled = false;
    getSyncLock()
      .then((v) => {
        if (!cancelled) setSyncLockState(v);
      })
      .catch(() => {
        // No session open yet; the default stands.
      });
    return () => {
      cancelled = true;
    };
  }, [head]);

  const handleSyncLockChange = useCallback(
    async (enabled: boolean) => {
      try {
        const newHead = await setSyncLock(enabled);
        setHeadLocal(newHead);
        setSyncLockState(enabled);
      } catch (err) {
        setRenderError(String(err));
      }
    },
    [setHeadLocal],
  );

  const handleRedo = useCallback(async () => {
    if (!head) return;
    try {
      const result = applyRedo(redoStack);
      if (!result) return;
      await setHeadTo(result.head);
      setHeadLocal(result.head);
      setRedoStack(result.redoStack);
      await refreshTracks();
    } catch (err) {
      setRenderError(String(err));
    }
  }, [head, redoStack, setHeadLocal]);

  // Window-level keyboard transport. Active whenever the user isn't
  // typing into a chat input / settings field. Space toggles
  // play/pause; Home/End jump to start/end; ←/→ seek 5 s; Shift+←/→
  // seek 1 s.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      const tag = target?.tagName ?? "";
      const isTyping =
        tag === "INPUT" || tag === "TEXTAREA" || target?.isContentEditable;
      if (isUndoChord(e) && !isTyping) {
        e.preventDefault();
        handleUndo();
        return;
      }
      if (isRedoChord(e) && !isTyping) {
        e.preventDefault();
        handleRedo();
        return;
      }
      // Ctrl/Cmd+K opens the command palette.
      //
      // Nothing opened it before. `paletteOpen` was initialised to
      // `false` and the only other reference set it back to `false`, so
      // the component rendered `null` for the entire life of the app —
      // 87 commands behind a door with no handle.
      //
      // Deliberately runs while `isTyping`: the palette's whole purpose
      // is to reach a command without leaving the keyboard, and the
      // chat box is exactly where a hand already is. ⌘K is not a
      // text-editing key, so intercepting it there costs nothing.
      if ((e.metaKey || e.ctrlKey) && (e.key === "k" || e.key === "K")) {
        e.preventDefault();
        setPaletteOpen((open) => !open);
        return;
      }
      if (e.key === "?" && !e.ctrlKey && !e.altKey && !e.metaKey && !isTyping) {
        e.preventDefault();
        if (!showShortcuts) setShowShortcuts(true);
        return;
      }
      if ((e.key === "l" || e.key === "L") && !isTyping) {
        e.preventDefault();
        setLoopActive((v) => !v);
        return;
      }
      const t = timelineRef.current;
      if (!t) return;
      if (e.key === " " && !isTyping) {
        e.preventDefault();
        t.togglePlay();
      } else if (e.key === "Home" && !isTyping) {
        e.preventDefault();
        t.seekTo(0);
      } else if (e.key === "End" && !isTyping) {
        e.preventDefault();
        t.seekTo(t.getDuration());
      } else if (e.key === "ArrowLeft" && !isTyping) {
        e.preventDefault();
        t.seekBy(e.shiftKey ? -1 : -5);
      } else if (e.key === "ArrowRight" && !isTyping) {
        e.preventDefault();
        t.seekBy(e.shiftKey ? 1 : 5);
      } else if (e.key === "Escape" && !isTyping && selection && !showShortcuts) {
        e.preventDefault();
        setSelection(null);
      } else if ((e.key === "+" || e.key === "=") && !isTyping) {
        e.preventDefault();
        setZoomPxPerSec((z) => Math.min(z + 40, 2000));
      } else if (e.key === "-" && !isTyping) {
        e.preventDefault();
        setZoomPxPerSec((z) => Math.max(z - 40, 0));
      } else if (e.key === "0" && !isTyping) {
        e.preventDefault();
        setZoomPxPerSec(0);
      } else if (
        (e.key === "e" || e.key === "E") &&
        (e.metaKey || e.ctrlKey) &&
        !isTyping
      ) {
        // Audacity's Ctrl+E. Frames the selection rather than leaving
        // the user to zoom and then hunt for it.
        e.preventDefault();
        t.zoomToSelection();
      } else if (
        (e.key === "f" || e.key === "F") &&
        (e.metaKey || e.ctrlKey) &&
        !isTyping
      ) {
        e.preventDefault();
        t.fitToWindow();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [selection, showShortcuts, handleUndo, handleRedo]);

  useEffect(() => {
    let cancelled = false;
    hasApiKey()
      .then((ok) => {
        if (!cancelled) setKeyConfigured(ok);
      })
      .catch(() => {
        if (!cancelled) setKeyConfigured(false);
      });

    // Install the 8 bundled skill files to ~/.edytlab/skills/ on first
    // launch. Fire-and-forget — non-fatal if the resource dir is absent
    // (dev mode) or skills already exist.
    installBundledSkills().catch(() => {
      // Non-fatal: bundled skills may not be available in dev mode
    });

    return () => {
      cancelled = true;
    };
  }, []);

  // Marker init + subscription.
  useEffect(() => {
    void listMarkers().then(setMarkers).catch(() => setMarkers([]));
    let unlisten: (() => void) | null = null;
    onMarkerChanged(() => {
      void listMarkers().then(setMarkers).catch(() => setMarkers([]));
    })
      .then((fn) => { unlisten = fn; })
      .catch(() => undefined);
    return () => { unlisten?.(); };
  }, []);


  /**
   * Adopt the node a command just appended (#232).
   *
   * Every session-mutating command returns its new head, and only the
   * mixer commands were reading it. `agent://node-created` fires from
   * the agent path alone (commands.rs:1403), so for a UI-driven edit
   * the frontend head simply stopped moving.
   *
   * That is data loss, not staleness. `persistView` writes this value
   * into `view.json`, and `restoreView` feeds it to `set_head_to`,
   * which rewinds the store head *durably* on the next open — so every
   * label typed since the last agent edit is gone from `list_markers`,
   * while the nodes themselves sit intact and unreachable in the graph.
   * Undo reads the same value, so Ctrl+Z after naming a label reverts
   * the edit before it instead.
   *
   * Clearing the mix is part of adopting the head: `render_preview`
   * names its output after the node id, so re-rendering a stale head
   * hands back the same path string, React's useState bails out, the
   * load effect never fires and nothing reloads — no change and no
   * error either.
   */
  const applyNewHead = useCallback(
    (newHead: string | null | undefined) => {
      if (!newHead) return;
      setHeadLocal(newHead);
      setMixPath(null);
      setMixNodeId(null);
    },
    [setHeadLocal],
  );

  /**
   * Load whatever arrived — from the picker, the menu or a drop — one
   * file or several, each as its own track.
   *
   * Straight into the session, through `batch_load`: the same `load`
   * tool the agent uses, with no model involved (#321). A single file
   * used to be sent to the agent as "load this file: …" and drawn at
   * once, so with no working model — offline, no key yet, a model that
   * cannot call tools — the waveform and "ready" showed over a session
   * that stayed empty, and every edit after failed with nothing on
   * screen saying why. Loading a file has one correct outcome; it is not
   * a judgement for a model to make.
   *
   * Nothing is drawn until the load succeeds: the timeline follows the
   * tracks, and a failure names the file. The agent still learns of it —
   * its next turn reads the session, where the load is a node like any
   * other.
   */
  const loadFiles = useCallback(
    async (paths: string[]) => {
      if (paths.length === 0) return;
      try {
        applyNewHead((await batchLoad(paths)).last_node_id);
        await refreshTracks();
      } catch (err) {
        const what = paths.length === 1 ? trimPath(paths[0]) : `${paths.length} files`;
        setRenderError(`Could not load ${what}: ${String(err)}`);
      }
    },
    [applyNewHead],
  );

  const handleOpenDialog = useCallback(async () => {
    try {
      const paths = await pickAudioFiles(true);
      if (paths) await loadFiles(paths);
    } catch (err) {
      setRenderError(String(err));
    }
  }, [loadFiles]);

  useEffect(() => {
    void listTemplates().then(setTemplates).catch(console.error);
  }, []);

  const handleApplyTemplate = useCallback(async (name: string) => {
    setShowTemplatePicker(false);
    try {
      applyNewHead(await applyTemplate(name));
      await refreshTracks();
    } catch (e) {
      setRenderError(String(e));
    }
  }, [applyNewHead]);

  // Mixer commits. Each command appends one session node, so the head
  // moves and the track list has to be re-read: the Timeline shows the
  // value optimistically, and this is what confirms or corrects it.
  //
  // A rejected value (out of range, track gone) surfaces in the error
  // banner and the refresh puts the control back where the session
  // actually is, rather than leaving the fader lying.
  const commitTrackChange = useCallback(
    async (apply: () => Promise<string>) => {
      try {
        // Every one of these commands appends a node and returns its id.
        // `applyNewHead` is what stops that value being discarded; see
        // its comment for why a stale head loses data rather than just
        // looking wrong.
        applyNewHead(await apply());
      } catch (e) {
        setRenderError(String(e));
      }
      try {
        await refreshTracks();
      } catch (e) {
        setRenderError(String(e));
      }
    },
    [applyNewHead],
  );

  const handleTrackGainChange = useCallback(
    (index: number, gainDb: number) =>
      void commitTrackChange(() => setTrackGain(index, gainDb)),
    [commitTrackChange],
  );
  const handleTrackPanChange = useCallback(
    (index: number, pan: number) =>
      void commitTrackChange(() => setTrackPan(index, pan)),
    [commitTrackChange],
  );
  const handleTrackMuteChange = useCallback(
    (index: number, muted: boolean) =>
      void commitTrackChange(() => setTrackMuted(index, muted)),
    [commitTrackChange],
  );
  const handleClipEnvelopeChange = useCallback(
    (trackIndex: number, clipIndex: number, points: EnvelopePoint[]) =>
      void commitTrackChange(() =>
        setClipEnvelope(trackIndex, clipIndex, points),
      ),
    [commitTrackChange],
  );

  const handleMoveClip = useCallback(
    (trackIndex: number, clipIndex: number, startSec: number) =>
      void commitTrackChange(() => moveClip(trackIndex, clipIndex, startSec)),
    [commitTrackChange],
  );

  const handleRemoveClip = useCallback(
    (trackIndex: number, clipIndex: number) =>
      void commitTrackChange(() => removeClip(trackIndex, clipIndex)),
    [commitTrackChange],
  );

  const handleTrackSoloChange = useCallback(
    (index: number, soloed: boolean) =>
      void commitTrackChange(() => setTrackSoloed(index, soloed)),
    [commitTrackChange],
  );

  // Track-head actions (#161). Each is one existing tool, one node, and
  // undoes like any other edit — which is why removing does not stop to
  // ask. `listTracks` is refreshed after, since these change the list
  // itself rather than a value on a track.
  const afterTrackListChange = useCallback(async () => {
    try {
      await refreshTracks();
    } catch (e) {
      setRenderError(String(e));
    }
  }, []);

  const handleRenameTrack = useCallback(
    (index: number, name: string) =>
      void commitTrackChange(() => renameTrack(index, name)).then(
        afterTrackListChange,
      ),
    [commitTrackChange, afterTrackListChange],
  );

  const handleDuplicateTrack = useCallback(
    (index: number) =>
      void commitTrackChange(() => duplicateTrack(index)).then(
        afterTrackListChange,
      ),
    [commitTrackChange, afterTrackListChange],
  );

  const handleRemoveTrack = useCallback(
    (index: number) =>
      void commitTrackChange(() => removeTrack(index)).then(
        afterTrackListChange,
      ),
    [commitTrackChange, afterTrackListChange],
  );

  /**
   * Zoom, selection and playhead from a saved view.
   *
   * Each field is restored only if the file actually had it — an absent
   * zoom must not reset the timeline while claiming to restore it.
   *
   * `fill` sets only what is still at its default: auto-fit zoom and no
   * selection. At launch the view is read while the timeline is already
   * usable, and replacing whatever is on screen when the read comes back
   * undid a zoom the user had just made. Opening a project `replace`s:
   * there the whole view is being switched, and nothing on screen
   * belongs to the project being opened.
   */
  const applyView = useCallback(
    (view: ViewToApply, mode: "fill" | "replace") => {
      const zoom = view.zoomPxPerSec;
      if (zoom !== undefined) {
        setZoomPxPerSec((cur) => (mode === "fill" && cur !== 0 ? cur : zoom));
      }
      const sel = view.selection;
      if (sel !== undefined) {
        setSelection((cur) => (mode === "fill" && cur !== null ? cur : sel));
      }
      if (view.playheadSec !== undefined) {
        lastPlayheadRef.current = view.playheadSec;
        timelineRef.current?.seekTo(view.playheadSec);
      }
    },
    [],
  );

  /**
   * Put the user back where they were in a project they just opened.
   *
   * The head is a *request*: `view.json` can name a node that no longer
   * exists (a folder copied without `.audiograph/`, a rebuilt store),
   * so a failure there leaves the head the store reported and is not an
   * error worth showing.
   */
  const restoreView = useCallback(
    async (fallbackHead: string | null) => {
      const view = viewToApply(await getViewState().catch(() => null));
      applyView(view, "replace");
      if (view.head) {
        try {
          await setHeadTo(view.head);
          setHeadLocal(view.head);
          return;
        } catch {
          // Stale head: fall through to whatever the store reported.
        }
      }
      if (fallbackHead) setHeadLocal(fallbackHead);
    },
    [applyView, setHeadLocal],
  );

  /**
   * Reopening a project is opening it: same command, so the recents
   * row moves to the top and `project.json` records the visit exactly
   * as it would from the file dialog.
   */
  const handleOpenRecent = useCallback(
    async (path: string) => {
      try {
        const info = await openProject(path);
        // Whatever the last project had on screen is not this one's. A
        // rendered mix outlives a project change otherwise.
        setMixPath(null);
        setMixNodeId(null);
        await restoreView(info.head ?? null);
        await refreshTracks();
        setRecents(await listRecentProjects());
      } catch (e) {
        setRenderError(String(e));
      }
    },
    [restoreView],
  );

  /**
   * Remember the view, 500 ms after it stops changing.
   *
   * Debounced because zoom and selection change continuously while a
   * gesture is in flight, and a file write per pixel is absurd. Losing
   * the last half-second of a scroll position on a hard kill is not a
   * loss worth defending against.
   */
  const persistView = useCallback(() => {
    if (!head || !viewRestoredRef.current) return;
    // The playhead is read at save time rather than mirrored into
    // state: it changes on every audioprocess tick, and a React state
    // update per tick to feed a debounced disk write would be a lot of
    // machinery to end up in the same place. With no mix loaded there is
    // no playhead to read, so the last known one is kept instead.
    const timeline = timelineRef.current;
    const playheadSec =
      timeline && timeline.getDuration() > 0
        ? timeline.getCurrentTime()
        : lastPlayheadRef.current;
    lastPlayheadRef.current = playheadSec;
    void saveViewState(
      viewToSave({ head, zoomPxPerSec, selection, playheadSec }),
    ).catch(() => undefined);
  }, [head, zoomPxPerSec, selection]);

  useEffect(() => {
    if (!head) return;
    if (viewSaveTimerRef.current) clearTimeout(viewSaveTimerRef.current);
    viewSaveTimerRef.current = setTimeout(persistView, 500);
    return () => {
      if (viewSaveTimerRef.current) clearTimeout(viewSaveTimerRef.current);
    };
  }, [head, persistView]);

  // A session that was only *played* changes none of the state above,
  // so without this the playhead would never be written for it. Closing
  // the window is the one moment that is guaranteed to matter.
  useEffect(() => {
    window.addEventListener("beforeunload", persistView);
    return () => window.removeEventListener("beforeunload", persistView);
  }, [persistView]);

  /**
   * Open a project by folder — the verb that was missing entirely.
   * Until now the only way in was to open an audio *file*, which
   * created a project as a side effect and never said so.
   */
  const handleOpenProject = useCallback(async () => {
    try {
      const dir = await pickProjectDirectory();
      if (!dir) return;
      await handleOpenRecent(dir);
    } catch (e) {
      setRenderError(String(e));
    }
  }, [handleOpenRecent]);

  /**
   * Start a new project.
   *
   * Mechanically identical to opening one: `open_project` creates the
   * store when the folder does not already contain one. That is the
   * whole reason "New" did not exist — it was already possible, just
   * never named, so the only way to discover it was to point "Open
   * project…" at an empty folder and notice that it worked.
   *
   * Naming it is the fix. The two entries do the same call and differ
   * in what they promise, which is what a user is actually choosing
   * between.
   */
  const handleNewProject = useCallback(async () => {
    try {
      const dir = await pickProjectDirectory();
      if (!dir) return;
      await handleOpenRecent(dir);
    } catch (e) {
      setRenderError(String(e));
    }
  }, [handleOpenRecent]);

  /**
   * Save As: copy the project somewhere new and carry on there.
   *
   * The view is flushed first. It is normally written 500 ms after the
   * last change, and a copy taken inside that window would land at a
   * different scroll position than the one being left behind.
   */
  const handleSaveProjectAs = useCallback(async () => {
    try {
      const dir = await pickProjectDirectory();
      if (!dir) return;
      persistView();
      const report = await saveProjectAs(dir);
      setRecents(await listRecentProjects());
      await refreshTracks();
      // Not an error, so it does not go through the error banner — but
      // the numbers are worth seeing, since a copy that skipped the
      // cache is smaller than the folder it came from and that would
      // otherwise look like data loss.
      // eslint-disable-next-line no-console
      console.info(
        `Saved a copy to ${dir}: ${report.files} files, ` +
          `${(report.bytes / 1e6).toFixed(1)} MB, ` +
          `${report.skipped_previews} cached preview(s) left behind.`,
      );
    } catch (e) {
      setRenderError(String(e));
    }
  }, [persistView]);

  /** Forget the row, not the project. */
  const handleForgetRecent = useCallback(async (path: string) => {
    try {
      setRecents(await forgetRecentProject(path));
    } catch (e) {
      setRenderError(String(e));
    }
  }, []);

  // Load the recents list once at startup. A failure here is not worth
  // an error banner — the list is a convenience, and the Open button
  // still works without it.
  useEffect(() => {
    let cancelled = false;
    listRecentProjects()
      .then((list) => {
        if (!cancelled) setRecents(list);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);

  // Debounced selection IPC — push the selection to Rust 250 ms after
  // the last change so rapid drags don't flood the backend.
  const handleSelectionChange = useCallback((sel: Selection | null) => {
    setSelection(sel);
    if (selectionTimerRef.current) clearTimeout(selectionTimerRef.current);
    selectionTimerRef.current = setTimeout(() => {
      const range = sel ? { start_sec: sel.start, end_sec: sel.end } : null;
      void setSelectionContext(range).catch(() => undefined);
    }, 250);
  }, []);

  // `select_region` reports the region it matched, "for the user to
  // check" — and nothing consumed it (#252). The whole safety argument
  // for the tool is seeing a described region *before* pointing a
  // destructive tool at it, so a report nobody applies delivers none of
  // it.
  //
  // Routed through `handleSelectionChange` rather than `setSelection` so
  // the backend's selection context is updated too: the next turn's
  // "[apply to …]" prefix should agree with what is highlighted.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;

    void onToolProgress((p) => {
      if (p.kind !== "selection") return;
      if (typeof p.start_sec !== "number" || typeof p.end_sec !== "number") {
        return;
      }
      if (p.end_sec <= p.start_sec) return;
      handleSelectionChange({ start: p.start_sec, end: p.end_sec });
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [handleSelectionChange]);

  const handleAddMarker = useCallback(async (timeSec: number) => {
    const name = window.prompt("Marker name:", `marker ${markers.length + 1}`) ?? "";
    if (!name.trim()) return;
    try {
      applyNewHead(await addMarker(timeSec, name.trim()));
      // marker-changed event fires → setMarkers
    } catch (err) {
      setRenderError(String(err));
    }
  }, [markers.length, applyNewHead]);

  const handleRemoveMarker = useCallback(async (id: string) => {
    try {
      applyNewHead(await removeMarker(id));
    } catch (err) {
      setRenderError(String(err));
    }
  }, [applyNewHead]);

  // The lane's three edits. Each is one call and therefore one undoable
  // node — a rename is not a delete plus an add, and a drag is not a
  // sequence of moves (#203 §1).
  // The session axis: the furthest point any clip on any track reaches.
  // The timeline computes the same number internally for its own lanes,
  // but the label lane sits outside it and has to agree, or a label at
  // 30s lands somewhere other than 30s on the ruler above it.
  const sessionDuration = useMemo(
    () =>
      tracks.reduce(
        (max, t) =>
          t.clips.reduce((m, c) => Math.max(m, c.start_sec + c.length_sec), max),
        0,
      ),
    [tracks],
  );

  // The transcript at the current head. Re-read whenever the head moves
  // — a cut shifts every later word, and the pane must not go on
  // showing the timings from before the edit.
  const [transcript, setTranscript] = useState<TranscriptWord[]>([]);

  useEffect(() => {
    let cancelled = false;
    getTranscript()
      .then((w) => {
        if (!cancelled) setTranscript(w);
      })
      .catch(() => {
        // No session yet; the pane's empty state is the right answer.
      });
    return () => {
      cancelled = true;
    };
  }, [head]);

  const handleCutWords = useCallback(
    async (from: number, to: number) => {
      try {
        // Track 0: the transcript is a session-level record and the
        // tool cuts the track it is told to. One voice track is the
        // case this ships for; per-track transcripts are #168.
        const newHead = await cutTranscriptWords(0, from, to);
        setHeadLocal(newHead);
        await refreshTracks();
        setSelection(null);
      } catch (err) {
        setRenderError(String(err));
      }
    },
    [setHeadLocal],
  );

  const handleRenameMarker = useCallback(async (id: string, name: string) => {
    try {
      applyNewHead(await updateMarker(id, { name }));
    } catch (err) {
      setRenderError(String(err));
    }
  }, [applyNewHead]);

  const handleMoveMarker = useCallback(async (id: string, timeSec: number) => {
    try {
      applyNewHead(await updateMarker(id, { time: timeSec }));
    } catch (err) {
      setRenderError(String(err));
    }
  }, [applyNewHead]);

  const handleSeekToMarker = useCallback((timeSec: number) => {
    timelineRef.current?.seekTo(timeSec);
  }, []);

  // Native menu (`File > Open Audio…`) emits `menu://open-file`.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    listen("menu://open-file", () => {
      void handleOpenDialog();
    })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [handleOpenDialog]);

  // OS-level drag-and-drop. Tauri 2's webview intercepts native file
  // drops, so HTML5 onDrop never fires for them — we bind at the
  // webview level instead.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    listenToFileDrops((paths) => void loadFiles(paths))
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [loadFiles]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    onNodeCreated(async (_nodeId: string) => {
      setRedoStack([]); // new branch clears forward history
      setGraphRefresh((n) => n + 1);
      await refreshTracks();
      // The session moved, so any previously rendered mix is stale.
      setMixPath(null);
      setMixNodeId(null);
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    // Initial fetch. The backend reopens the default project at every
    // launch and restores its `HEAD`, so a returning user's tracks are
    // already there; a fresh project has no head yet and answers
    // `NoSession`, which leaves the list empty.
    void refreshTracks().catch(() => setTracks([]));
    // Then the view they left. Saving stays off until this has been
    // read, whether or not there was anything to read.
    //
    // Everything but its head. The backend has already restored `HEAD`,
    // and `useSession` reads it; the saved view's head can only be as new
    // or older. It is written 500 ms after the view settles, so an edit
    // in the last half-second before quitting leaves it one behind — and
    // obeying it moved `HEAD` back on disk, taking that edit off the
    // timeline. An edit landing while this read was in flight went the
    // same way.
    void getViewState()
      .catch(() => null)
      .then((saved) => applyView(viewToApply(saved), "fill"))
      .finally(() => {
        viewRestoredRef.current = true;
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [applyView]);

  const handleExportSelection = useCallback(async () => {
    if (!head || !selection || exporting) return;
    setExporting(true);
    setRenderError(null);
    try {
      const outPath = await save({
        title: "Export Selection",
        filters: [{ name: "WAV", extensions: ["wav"] }],
        defaultPath: "export.wav",
      });
      if (!outPath) { setExporting(false); return; }
      await renderRange(head, selection.start, selection.end, outPath);
    } catch (e) {
      setRenderError(String(e));
    } finally {
      setExporting(false);
    }
  }, [head, selection, exporting]);

  // Recording was the only pair of handlers reporting to the console
  // (#248). Every other one routes to the error banner, which is the
  // app's single error surface — and a packaged desktop build has no
  // console to read, so a failed Record was indistinguishable from a
  // dead button.
  //
  // The outcome logic lives in `lib/recording.ts` so it can be tested;
  // what stays here is the mapping onto state.
  const handleStartRecording = useCallback(async () => {
    setRenderError(null);
    const outcome = await startTake(startRecording);
    if (outcome.kind === "recording") {
      setIsRecording(true);
    } else {
      setRenderError(outcome.message);
    }
  }, []);

  const handleStopRecording = useCallback(async () => {
    setRenderError(null);
    const outcome = await stopTake(
      () => stopRecording(`recording_${Date.now()}.wav`),
      batchLoad,
    );

    // The button flips back whatever happened — the recorder has
    // stopped either way. What changed is that a lost take no longer
    // presents exactly like a successful stop.
    setIsRecording(false);

    if (outcome.kind === "loaded") {
      applyNewHead(outcome.nodeId);
      void refreshTracks().catch((err) => setRenderError(String(err)));
    } else {
      setRenderError(outcome.message);
    }
  }, [applyNewHead]);

  /**
   * Arm an unattended take (#225 §4).
   *
   * `timer_record` resolves only when the take is done — minutes
   * later, by design — so this is deliberately not awaited into a
   * spinner. The countdown and Cancel are the progress strip's, on the
   * channel the backend already reports to.
   *
   * `isRecording` is held for the whole schedule, countdown included.
   * The recorder refuses a second take while one is armed, and a Stop
   * button that does nothing is worse than one that is not offered.
   */
  const handleTimerRecord = useCallback(
    (schedule: { startAfterSec?: number; durationSec?: number }) => {
      setRenderError(null);
      setIsRecording(true);
      void scheduledTake(
        () => timerRecord(`recording_${Date.now()}.wav`, schedule),
        batchLoad,
      ).then((outcome) => {
        setIsRecording(false);
        if (outcome.kind === "loaded") {
          applyNewHead(outcome.nodeId);
          void refreshTracks().catch((err) => setRenderError(String(err)));
        } else if (outcome.kind !== "cancelled") {
          // Cancelling is the user's own doing and needs no banner.
          setRenderError(outcome.message);
        }
      });
    },
    [applyNewHead],
  );

  const handleRenderPreview = useCallback(async () => {
    if (!head || rendering) return;
    setRendering(true);
    setRenderError(null);
    try {
      const path = await renderHead();
      setMixPath(path);
      setMixNodeId(head);
    } catch (err) {
      setRenderError(String(err));
    } finally {
      setRendering(false);
    }
  }, [head, rendering, renderHead]);

  const handleSelectGraphNode = useCallback(
    async (nodeId: string) => {
      setHeadLocal(nodeId);
      setRendering(true);
      setRenderError(null);
      try {
        const path = await bridgeRenderPreview(nodeId);
        setMixPath(path);
        setMixNodeId(nodeId);
      } catch (err) {
        setRenderError(String(err));
      } finally {
        setRendering(false);
      }
    },
    [setHeadLocal],
  );

  const handleCompareNodes = useCallback(
    (bNodeId: string) => {
      if (!head) return;
      setCompareMode({ a: head, b: bNodeId });
    },
    [head],
  );

  const handleAcceptB = useCallback(() => {
    if (!compareMode) return;
    setHeadLocal(compareMode.b);
    setCompareMode(null);
  }, [compareMode, setHeadLocal]);

  const handleCloseShortcuts = useCallback(() => setShowShortcuts(false), []);

  // Not while the panel is open (#250). `keyConfigured` now updates on a
  // provider switch, and without this guard flipping it false would
  // replace the panel the user is standing in — losing their tab and
  // anything typed — with the blocking prompt. They are already in the
  // right place; the panel says what happened. The prompt appears when
  // they close it and the app is still keyless.
  const showBlocking = keyConfigured === false && !settingsOpen;

  const errorAction = useMemo(() => {
    if (!renderError) return undefined;
    if (!isApiKeyError(renderError)) return undefined;
    return {
      label: "Open Settings",
      onClick: () => {
        setRenderError(null);
        setSettingsOpen(true);
      },
    };
  }, [renderError]);

  return (
    <main className="grid h-screen w-screen grid-rows-[auto_1fr_auto] bg-[var(--bg)] text-[var(--text)] app-fade-in">
      <AppHeader
        leftView={leftView}
        onSelectView={setLeftView}
        onOpen={handleOpenDialog}
        onSettings={() => setSettingsOpen(true)}
        isRecording={isRecording}
        onRecord={isRecording ? handleStopRecording : handleStartRecording}
        onTimerRecord={handleTimerRecord}
        onSaveAs={handleSaveProjectAs}
        hasProject={Boolean(head)}
        onNewProject={handleNewProject}
        onOpenProject={handleOpenProject}
      />

      <div className="grid min-h-0 grid-cols-[minmax(0,1fr)_360px] gap-px bg-[var(--border)]">
        <section className="flex h-full min-h-0 flex-col bg-[var(--surface)]">
          {renderError ? (
            <ErrorBanner
              testId="render-error"
              message={renderError}
              action={errorAction}
              onDismiss={() => setRenderError(null)}
            />
          ) : null}

          {/* Above the timeline so a long run is visible wherever the
              user is looking, and gone again the moment it finishes. */}
          <ToolProgressBar />

          {compareMode ? (
            <ABCompareBar
              aNodeId={compareMode.a}
              bNodeId={compareMode.b}
              onAudioPathChange={setMixPath}
              onAcceptB={handleAcceptB}
              onClose={() => setCompareMode(null)}
            />
          ) : null}

          <div className="flex-1 min-h-0 overflow-hidden">
            {leftView === "timeline" ? (
              timelineSource ? (
                <Timeline
                  ref={timelineRef}
                  audioPath={timelineSource}
                  tracks={tracks
                    // `index` is captured before the filter: a track
                    // with no audio is not drawn but still occupies a
                    // slot the mixer commands address by.
                    .map((t, index) => ({ t, index }))
                    .filter(({ t }) => t.audio_path)
                    .map(({ t, index }) => ({
                      index,
                      name: t.name,
                      audioPath: t.audio_path as string,
                      muted: t.muted,
                      gainDb: t.gain_db,
                      pan: t.pan,
                      soloed: t.soloed,
                      clips: t.clips,
                    }))}
                  onTrackGainChange={handleTrackGainChange}
                  onTrackPanChange={handleTrackPanChange}
                  onTrackMuteChange={handleTrackMuteChange}
                  onTrackSoloChange={handleTrackSoloChange}
                  onRenameTrack={handleRenameTrack}
                  onDuplicateTrack={handleDuplicateTrack}
                  onRemoveTrack={handleRemoveTrack}
                  onClipEnvelopeChange={handleClipEnvelopeChange}
                  onMoveClip={handleMoveClip}
                  onRemoveClip={handleRemoveClip}
                  onFileDropped={(path) => void loadFiles([path])}
                  selection={selection}
                  onSelectionChange={handleSelectionChange}
                  markers={markers}
                  onAddMarker={handleAddMarker}
                  onRemoveMarker={handleRemoveMarker}
                  onSeekToMarker={handleSeekToMarker}
                  zoom={zoomPxPerSec}
                  onZoomChange={setZoomPxPerSec}
                  onLoadErrorChange={setAudioLoadError}
                  mixPath={mixPath}
                  snapToZero={snapToZero}
                  onSnapToZeroChange={setSnapToZero}
                  syncLock={syncLock}
                  onSyncLockChange={handleSyncLockChange}
                  verticalZoom={verticalZoom}
                  onVerticalZoomChange={setVerticalZoom}
                  loop={loopActive}
                  onLoopChange={setLoopActive}
                  spectrogramEnabled={spectrogramEnabled}
                  onSpectrogramChange={setSpectrogramEnabled}
                  // Inside the timeline, on its axis: zoomed, a label
                  // has to stay over the audio it names.
                  belowLanes={(view) => (
                    <LabelLane
                      labels={markers}
                      duration={sessionDuration}
                      view={view}
                      onAdd={handleAddMarker}
                      onRename={handleRenameMarker}
                      onMove={handleMoveMarker}
                      onRemove={handleRemoveMarker}
                      onSeek={handleSeekToMarker}
                    />
                  )}
                />
              ) : (
                <EmptyState
                  onOpen={handleOpenDialog}
                  onOpenProject={handleOpenProject}
                  onNewProject={handleNewProject}
                  onShowTemplates={() => setShowTemplatePicker(true)}
                  recents={recents}
                  onOpenRecent={handleOpenRecent}
                  onForgetRecent={handleForgetRecent}
                />
              )
            ) : leftView === "transcript" ? (
              <TranscriptPane
                words={transcript}
                selection={selection}
                onSelectRange={handleSelectionChange}
                onCutWords={handleCutWords}
                onSeek={handleSeekToMarker}
              />
            ) : (
              <GraphView
                head={head}
                onSelectNode={handleSelectGraphNode}
                onCompareNodes={handleCompareNodes}
                refreshKey={graphRefresh}
              />
            )}
          </div>
        </section>

        <aside className="h-full min-h-0 bg-[var(--surface)]">
          <Chat ref={chatRef} 
            rendering={rendering}
            onRequestRenderPreview={handleRenderPreview}
            selection={selection}
            onClearSelection={() => {
              setSelection(null);
              void setSelectionContext(null).catch(() => undefined);
            }}
            markers={markers}
            onExportSelection={handleExportSelection}
            exporting={exporting}
            onOpenSettings={() => setSettingsOpen(true)}
          />
        </aside>
      </div>

      <StatusBar
        audioPath={timelineSource}
        head={head}
        rendering={rendering}
        selection={selection}
        mixStale={mixIsStale({ mixPath, mixNodeId }, head)}
        loadError={audioLoadError}
      />

      {showBlocking ? (
        <Settings
          mode="blocking"
          onSaved={() => {
            setKeyConfigured(true);
            // First-launch save: clear any stale error from the
            // pre-key state (e.g. an automatic Render Preview that
            // hit "no agent configured").
            setRenderError(null);
          }}
          onProviderChanged={setKeyConfigured}
        />
      ) : null}
      {!showBlocking && settingsOpen ? (
        <Settings
          mode="panel"
          onClose={() => setSettingsOpen(false)}
          onSaved={() => {
            setSettingsOpen(false);
            // The Rust side rebuilds the agent inside set_api_key_for,
            // so any "no agent configured" banner left over from the
            // failed action that prompted the user to open settings is
            // now stale — drop it.
            setRenderError(null);
          }}
          onProviderChanged={setKeyConfigured}
          onCleared={() => {
            setKeyConfigured(false);
            setSettingsOpen(false);
          }}
        />
      ) : null}
      <CommandPalette open={paletteOpen} onClose={() => setPaletteOpen(false)} onSelect={(prompt) => { setPaletteOpen(false); chatRef.current?.fillInput(prompt); }} />
      <ShortcutsOverlay open={showShortcuts} onClose={handleCloseShortcuts} />
      <TemplatePickerModal
        open={showTemplatePicker}
        templates={templates}
        onSelect={handleApplyTemplate}
        onClose={() => setShowTemplatePicker(false)}
      />
    </main>
  );
}

interface StatusBarProps {
  audioPath: string | null;
  head: string | null;
  rendering: boolean;
  selection: Selection | null;
  /**
   * True when a mix has been rendered but the session has moved on since.
   * Without this there is no way to tell whether what you would hear
   * matches what you are looking at — the preview is named after the node
   * it came from, so a stale one is indistinguishable from a current one.
   */
  mixStale?: boolean;
  /**
   * Set when the chosen audio failed to decode.
   *
   * Without it this bar reports state from `audioPath` alone, which
   * only says a path was picked. A file that 404s left `ready` and the
   * filename sitting directly under the timeline's own error box —
   * two claims about the same file, and the one the user reads first
   * was the wrong one.
   */
  loadError?: string | null;
}

export function StatusBar({
  audioPath,
  head,
  rendering,
  selection,
  mixStale,
  loadError,
}: StatusBarProps) {
  const failed = Boolean(audioPath) && Boolean(loadError);
  const fileLabel = audioPath ? trimPath(audioPath) : "no file loaded";
  const headLabel = head ? `head ${head.slice(0, 7)}` : "no head";
  return (
    <footer
      data-testid="status-bar"
      className="
        flex shrink-0 items-center gap-4
        border-t border-[var(--border)]
        bg-[var(--surface-elev)]
        px-4 py-1.5
        font-mono text-[10px] uppercase tracking-[0.18em] text-[var(--text-faint)]
      "
    >
      <span className="flex items-center gap-1.5">
        <span
          aria-hidden="true"
          className={
            "h-1.5 w-1.5 rounded-full " +
            (rendering
              ? "bg-[var(--warning)] animate-pulse"
              : failed
                ? "bg-[var(--danger)]"
                : audioPath
                  ? "bg-[var(--success)]"
                  : "bg-[var(--text-faint)]")
          }
        />
        {rendering
          ? "rendering…"
          : failed
            ? "load failed"
            : audioPath
              ? "ready"
              : "idle"}
      </span>
      {mixStale ? (
        <>
          <span className="text-[var(--text-faint)]/80">·</span>
          <span
            data-testid="status-bar-mix-stale"
            className="text-[var(--warning)]"
            title="The session has changed since the last preview render"
          >
            preview out of date
          </span>
        </>
      ) : null}
      <span className="text-[var(--text-faint)]/80">·</span>
      <span data-testid="status-bar-file" title={audioPath ?? undefined}>
        {fileLabel}
      </span>
      <span className="text-[var(--text-faint)]/80">·</span>
      <span data-testid="status-bar-head">{headLabel}</span>
      {selection ? (
        <>
          <span className="text-[var(--text-faint)]/80">·</span>
          <span
            data-testid="status-bar-selection"
            className="text-[var(--accent)]"
          >
            sel {fmtTime(selection.start)} → {fmtTime(selection.end)} (
            {fmtDuration(selection.end - selection.start)})
          </span>
        </>
      ) : null}
      <span className="ml-auto text-[var(--text-faint)]">v0.1.0</span>
    </footer>
  );
}

function fmtTime(sec: number): string {
  const m = Math.floor(sec / 60);
  const s = sec - m * 60;
  return `${m}:${s.toFixed(2).padStart(5, "0")}`;
}

function fmtDuration(sec: number): string {
  return `${sec.toFixed(2)}s`;
}

function trimPath(path: string): string {
  const sep = path.includes("\\") ? "\\" : "/";
  const parts = path.split(sep);
  return parts[parts.length - 1] || path;
}

export default App;
