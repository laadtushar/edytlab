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
import type { EnvelopePoint, Marker, TrackSummary } from "./lib/tauri-bridge";
import {
  addMarker,
  getNode,
  listMarkers,
  listTracks,
  onMarkerChanged,
  removeMarker,
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
import { applyUndo, applyRedo } from "./lib/undoRedo";
import { mixIsStale } from "./lib/mixState";

import { ABCompareBar } from "./components/ABCompareBar";
import { Chat } from "./components/Chat";
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
  renderPreview as bridgeRenderPreview,
} from "./lib/tauri-bridge";
import { listTemplates, applyTemplate, startRecording, stopRecording } from "./lib/tauri-bridge";
import type { TemplateInfo } from "./components/TemplatePickerModal";
import {
  listenToFileDrops,
  loadAudio,
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
import { viewToApply, viewToSave } from "./lib/viewState";

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
  // `sourcePath` is a *source* file — what the user opened, or a track's
  // own flattened WAV. It has no mixer state applied and is only ever
  // right for drawing a lane or naming the session in the status bar.
  //
  // `mixPath` is the output of `render_preview` for a specific node: the
  // mix, with gain, pan, mute, solo, chains, sends and the master chain
  // in it. It is the only thing that should ever be *played*.
  //
  // Merged, `onNodeCreated` overwrote the mix with a raw track path after
  // every agent turn, so the mix was correct for about one render and
  // then quietly was not.
  const [sourcePath, setSourcePath] = useState<string | null>(null);
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
  const selectionTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [zoomPxPerSec, setZoomPxPerSec] = useState(0);
  const [redoStack, setRedoStack] = useState<string[]>([]);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const chatRef = useRef<ChatHandle>(null);
  const [exporting, setExporting] = useState(false);
  const [loopActive, setLoopActive] = useState(false);
  const [spectrogramEnabled, setSpectrogramEnabled] = useState(false);
  // Off by default: snapping changes where an edit lands, and the
  // behaviour that existed is the one a user is not surprised by.
  const [snapToZero, setSnapToZero] = useState(false);
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

  const handleUndo = useCallback(async () => {
    if (!head) return;
    try {
      const node = await getNode(head);
      const result = applyUndo(head, node.parent ?? null, redoStack);
      if (!result) return;
      await setHeadTo(result.head);
      setHeadLocal(result.head);
      setRedoStack(result.redoStack);
      const newTracks = await listTracks();
      setTracks(newTracks);
    } catch (err) {
      setRenderError(String(err));
    }
  }, [head, redoStack, setHeadLocal]);

  const handleRedo = useCallback(async () => {
    if (!head) return;
    try {
      const result = applyRedo(redoStack);
      if (!result) return;
      await setHeadTo(result.head);
      setHeadLocal(result.head);
      setRedoStack(result.redoStack);
      const newTracks = await listTracks();
      setTracks(newTracks);
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
      if (e.ctrlKey && !e.shiftKey && e.key === "z" && !isTyping) {
        e.preventDefault();
        handleUndo();
        return;
      }
      if (
        ((e.ctrlKey && e.key === "y") ||
          (e.ctrlKey && e.shiftKey && e.key === "z")) &&
        !isTyping
      ) {
        e.preventDefault();
        handleRedo();
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

  // Common entry point for "user just supplied a file" — used by the
  // toolbar Open button, the native File > Open menu, and OS-level
  // drag-and-drop.
  const handleFileSelected = useCallback((path: string) => {
    void loadAudio(path, setSourcePath, (err) => setRenderError(err));
  }, []);

  /**
   * Load whatever arrived — from the picker or from a drop.
   *
   * One file keeps the single-file path so the agent gets a "load this
   * file: …" message and the waveform updates the way it always has.
   * Several go through `batch_load`, which adds each as its own track
   * rather than replacing the session.
   */
  const handleFilesSelected = useCallback(
    async (paths: string[]) => {
      if (paths.length === 0) return;
      if (paths.length === 1) {
        handleFileSelected(paths[0]);
        return;
      }
      setSourcePath(paths[0]);
      try {
        await batchLoad(paths);
        setTracks(await listTracks());
      } catch (err) {
        setRenderError(String(err));
      }
    },
    [handleFileSelected],
  );

  const handleOpenDialog = useCallback(async () => {
    try {
      const paths = await pickAudioFiles(true);
      if (!paths || paths.length === 0) return;
      if (paths.length === 1) {
        // Single file — use the existing single-file path so the agent
        // receives a "load this file: …" message and waveform updates normally.
        handleFileSelected(paths[0]);
        return;
      }
      // Multiple files — call batch_load then refresh track list.
      setSourcePath(paths[0]);
      try {
        await batchLoad(paths);
        const newTracks = await listTracks();
        setTracks(newTracks);
      } catch (err) {
        setRenderError(String(err));
      }
    } catch (err) {
      setRenderError(String(err));
    }
  }, [handleFileSelected]);

  useEffect(() => {
    void listTemplates().then(setTemplates).catch(console.error);
  }, []);

  const handleApplyTemplate = useCallback(async (name: string) => {
    setShowTemplatePicker(false);
    try {
      await applyTemplate(name);
      const newTracks = await listTracks();
      setTracks(newTracks);
    } catch (e) {
      setRenderError(String(e));
    }
  }, []);

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
        // That return value used to be discarded, so `head` never moved
        // for a UI-driven edit — `NODE_CREATED` is emitted only from the
        // agent path (commands.rs:1403).
        //
        // A stale head is not cosmetic. `render_preview` names its
        // output after the node id, so re-rendering a stale head hands
        // back the *same path string*; setting it then hits React's
        // useState bailout, the load effect never fires, and nothing
        // reloads — no change, and no error either. Moving a fader and
        // pressing render appeared to work and did nothing.
        const newHead = await apply();
        if (newHead) {
          setHeadLocal(newHead);
          setMixPath(null);
          setMixNodeId(null);
        }
      } catch (e) {
        setRenderError(String(e));
      }
      try {
        setTracks(await listTracks());
      } catch (e) {
        setRenderError(String(e));
      }
    },
    [setHeadLocal],
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
      setTracks(await listTracks());
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
   * Put the user back where they were.
   *
   * Each field is restored only if the file actually had it — an absent
   * zoom must not reset the timeline while claiming to restore it. The
   * head is a *request*: `view.json` can name a node that no longer
   * exists (a folder copied without `.audiograph/`, a rebuilt store),
   * so a failure there leaves the head the store reported and is not an
   * error worth showing.
   */
  const restoreView = useCallback(
    async (fallbackHead: string | null) => {
      const view = viewToApply(await getViewState().catch(() => null));
      if (view.zoomPxPerSec !== undefined) setZoomPxPerSec(view.zoomPxPerSec);
      if (view.selection !== undefined) setSelection(view.selection);
      if (view.playheadSec !== undefined) {
        timelineRef.current?.seekTo(view.playheadSec);
      }
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
    [setHeadLocal],
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
        await restoreView(info.head ?? null);
        setTracks(await listTracks());
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
    if (!head) return;
    // The playhead is read at save time rather than mirrored into
    // state: it changes on every audioprocess tick, and a React state
    // update per tick to feed a debounced disk write would be a lot of
    // machinery to end up in the same place.
    void saveViewState(
      viewToSave({
        head,
        zoomPxPerSec,
        selection,
        playheadSec: timelineRef.current?.getCurrentTime() ?? 0,
      }),
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
      setTracks(await listTracks());
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

  const handleAddMarker = useCallback(async (timeSec: number) => {
    const name = window.prompt("Marker name:", `marker ${markers.length + 1}`) ?? "";
    if (!name.trim()) return;
    try {
      await addMarker(timeSec, name.trim());
      // marker-changed event fires → setMarkers
    } catch (err) {
      setRenderError(String(err));
    }
  }, [markers.length]);

  const handleRemoveMarker = useCallback(async (id: string) => {
    try {
      await removeMarker(id);
    } catch (err) {
      setRenderError(String(err));
    }
  }, []);

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
    listenToFileDrops((paths) => void handleFilesSelected(paths))
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [handleFilesSelected]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    onNodeCreated(async (_nodeId: string) => {
      setRedoStack([]); // new branch clears forward history
      setGraphRefresh((n) => n + 1);
      const newTracks = await listTracks();
      setTracks(newTracks);
      // A track's own audio, for the lane and the status bar. It is
      // NOT the mix, so it must not touch `mixPath` — doing so is what
      // made every edit fall back to unmixed audio.
      const firstPath = newTracks[0]?.audio_path;
      if (firstPath) setSourcePath(firstPath);
      // The session moved, so any previously rendered mix is stale.
      setMixPath(null);
      setMixNodeId(null);
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    // Initial fetch — covers the case where the project already had
    // tracks at startup (auto-init creates a single empty track).
    void listTracks()
      .then(setTracks)
      .catch(() => setTracks([]));
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

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

  const handleStartRecording = useCallback(async () => {
    try {
      await startRecording();
      setIsRecording(true);
    } catch (e) {
      console.error("start_recording failed:", e);
    }
  }, []);

  const handleStopRecording = useCallback(async () => {
    try {
      const result = await stopRecording(
        `recording_${Date.now()}.wav`
      );
      setIsRecording(false);
      await batchLoad([result.path]);
      void listTracks().then(setTracks);
    } catch (e) {
      console.error("stop_recording failed:", e);
      setIsRecording(false);
    }
  }, []);

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

  const showBlocking = keyConfigured === false;

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
        onSaveAs={handleSaveProjectAs}
        hasProject={Boolean(head)}
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
              sourcePath ? (
                <Timeline
                  ref={timelineRef}
                  audioPath={sourcePath}
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
                  onFileDropped={() => undefined}
                  selection={selection}
                  onSelectionChange={handleSelectionChange}
                  markers={markers}
                  onAddMarker={handleAddMarker}
                  onRemoveMarker={handleRemoveMarker}
                  onSeekToMarker={handleSeekToMarker}
                  zoom={zoomPxPerSec}
                  onZoomChange={setZoomPxPerSec}
                  mixPath={mixPath}
                  snapToZero={snapToZero}
                  onSnapToZeroChange={setSnapToZero}
                  verticalZoom={verticalZoom}
                  onVerticalZoomChange={setVerticalZoom}
                  loop={loopActive}
                  onLoopChange={setLoopActive}
                  spectrogramEnabled={spectrogramEnabled}
                  onSpectrogramChange={setSpectrogramEnabled}
                />
              ) : (
                <EmptyState
                  onOpen={handleOpenDialog}
                  onOpenProject={handleOpenProject}
                  onShowTemplates={() => setShowTemplatePicker(true)}
                  recents={recents}
                  onOpenRecent={handleOpenRecent}
                  onForgetRecent={handleForgetRecent}
                />
              )
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
          />
        </aside>
      </div>

      <StatusBar
        audioPath={sourcePath}
        head={head}
        rendering={rendering}
        selection={selection}
        mixStale={mixIsStale({ mixPath, mixNodeId }, head)}
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
}

export function StatusBar({
  audioPath,
  head,
  rendering,
  selection,
  mixStale,
}: StatusBarProps) {
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
              : audioPath
                ? "bg-[var(--success)]"
                : "bg-[var(--text-faint)]")
          }
        />
        {rendering ? "rendering…" : audioPath ? "ready" : "idle"}
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
