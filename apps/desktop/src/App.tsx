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

import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { EnvelopePoint, RenderWarning } from "./lib/tauri-bridge";
import { getSyncLock, renderRange, setSelectionContext } from "./lib/tauri-bridge";
import { save } from "@tauri-apps/plugin-dialog";
import { isUndoChord, isRedoChord, isTextEntry } from "./lib/undoRedo";
import { isDerivedAudioPath } from "./lib/derivedAudio";
import { mixIsCurrent, mixIsMissing, mixIsStale } from "./lib/mixState";
import { scheduledTake, startTake, stopTake } from "./lib/recording";
import { appliedText, isUncalibratedMessage, punchOutcome } from "./lib/punch";

import { ABCompareBar } from "./components/ABCompareBar";
import { Chat } from "./components/Chat";
import { LabelLane } from "./components/LabelLane";
import { ToolProgressBar } from "./components/ToolProgressBar";
import { TranscriptPane } from "./components/TranscriptPane";
import { TemplatePickerModal } from "./components/TemplatePickerModal";
import type { ChatHandle } from "./components/Chat";
import { hasOpenDialog } from "./hooks/useDialog";
import { CommandPalette } from "./components/CommandPalette";
import { AppHeader } from "./components/AppHeader";
import { EmptyState } from "./components/EmptyState";
import { ErrorBanner } from "./components/ErrorBanner";
import { RenderWarningsNotice } from "./components/RenderWarningsNotice";
import { GraphView } from "./components/GraphView";
import { Settings } from "./components/Settings";
import { ShortcutsOverlay } from "./components/ShortcutsOverlay";
import {
  Timeline,
  type Selection,
  type TimelineHandle,
} from "./components/Timeline";
import { useMarkers } from "./hooks/useMarkers";
import { useSession, type EditArgs, type EditName, type HeldInput } from "./hooks/useSession";
import { useTranscript } from "./hooks/useTranscript";
import { SessionQueryProvider } from "./lib/sessionQueries";
import {
  getKeychainPersistence,
  hasApiKey,
  installBundledSkills,
  onToolProgress,
  renderPreview as bridgeRenderPreview,
  type KeychainPersistence,
} from "./lib/tauri-bridge";
import {
  keychainNotice,
  readSessionOnlyDismissed,
  writeSessionOnlyDismissed,
} from "./lib/keychainNotice";
import { KeyringWaiting } from "./components/KeyringWaiting";
import { listTemplates, punchRecord, startRecording, stopRecording, timerRecord } from "./lib/tauri-bridge";
import type { TemplateInfo } from "./components/TemplatePickerModal";
import {
  listenToFileDrops,
  pickAudioFiles,
  pickProjectDirectory,
} from "./lib/file-open";
import { isSessionValue } from "./lib/controlValue";
import { cutBlockedReason, cutTrack } from "./lib/transcriptSource";
import { describeLoadFailures } from "./lib/load-failures";
import {
  forgetRecentProject,
  getHeadTrail,
  getViewState,
  listRecentProjects,
  saveViewState,
  type RecentProject,
} from "./lib/tauri-bridge";
import { type ViewToApply, viewToApply, viewToSave } from "./lib/viewState";
import { readSavedTrail } from "./lib/headTrail";

import type { LeftView } from "./lib/views";
import { useHeadMove } from "./hooks/useHeadMove";
import { useGraphPreview } from "./hooks/useGraphPreview";
import { useRenderWarnings } from "./hooks/useRenderWarnings";
import { exportNotice as exportWarningNotice, mixNotice } from "./lib/renderWarnings";

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
  return (
    <SessionQueryProvider>
      <AppSession />
    </SessionQueryProvider>
  );
}

function AppSession() {
  const [renderError, setRenderError] = useState<string | null>(null);
  const {
    tracks: heldTracks,
    edition,
    sessionTracks,
    renderHead,
    head,
    trail,
    edit,
    move,
    holdInput,
    dropInput,
    sentValue,
  } = useSession({ onReadError: (err) => setRenderError(String(err)) });
  // The node the history graph is previewing, apart from `head` (#522): a
  // click there shows a node, and only "Set as head" moves the head, so the
  // undo path and the file it is saved to follow real moves alone.
  const { previewId, preview: previewNode, clearPreview } = useGraphPreview(head);
  // The node the loaded mix should be a render of: the one being
  // previewed, else the head. Everything that shows or plays the mix asks
  // about this; everything that is about the session asks about `head`.
  const shownNode = previewId ?? head;
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
  //
  // A render is of the session as it was when it was made: it carries the
  // session's `edition` and is no mix at all once an edit, the assistant, a
  // project opening or a punch has changed the session since, so nothing
  // clears it by hand. Undo, redo and Set as head leave it: it is the mix of
  // the node they left, and `mixIsStale` compares that node with the one shown.
  const [rendered, setRendered] = useState({ mixPath: null, mixNodeId: null, edition: 0 } as {
    mixPath: string | null;
    mixNodeId: string | null;
    edition: number;
  });
  const editionRef = useRef(edition);
  useLayoutEffect(() => {
    editionRef.current = edition;
  }, [edition]);
  const current = rendered.edition === edition;
  const mixPath = current ? rendered.mixPath : null;
  const mixNodeId = current ? rendered.mixNodeId : null;
  /** The mix of `nodeId` is at `path`; `nodeId` is not known for compare's renders. */
  const setMix = useCallback((path: string | null, nodeId?: string | null) => {
    setRendered((cur) => ({
      mixPath: path,
      mixNodeId: nodeId !== undefined ? nodeId : cur.edition === editionRef.current ? cur.mixNodeId : null,
      edition: editionRef.current,
    }));
  }, []);
  const [rendering, setRendering] = useState(false);
  const [leftView, setLeftView] = useState<LeftView>("timeline");
  const [keyConfigured, setKeyConfigured] = useState<boolean | null>(null);
  // Whether the launch-time `has_api_key` is still pending. It is while
  // the backend reads the keychain, which can be as long as a locked
  // keyring's unlock prompt waits on the user (#497). Not derived from
  // `keyConfigured` and `keychainError`: dismissing the error banner
  // clears the second, and that is not the read starting again.
  const [keyCheckPending, setKeyCheckPending] = useState(true);
  // Why the keychain could not be read at launch, if it could not. Not
  // the same as `keyConfigured === false`: a key may well be stored, and
  // asking for one again would be wrong (#394).
  const [keychainError, setKeychainError] = useState<string | null>(null);
  // Whether saved settings survive a reboot; null until the backend says.
  const [keychainPersistence, setKeychainPersistence] =
    useState<KeychainPersistence | null>(null);
  const [sessionOnlyDismissed, setSessionOnlyDismissed] = useState(
    readSessionOnlyDismissed,
  );
  const [settingsOpen, setSettingsOpen] = useState(false);
  // Moves when the active provider may have changed (Settings opened or
  // closed, a provider chosen there), for Chat's "Using ChatGPT plan" line.
  const [providerEpoch, setProviderEpoch] = useState(0);
  useEffect(() => {
    setProviderEpoch((n) => n + 1);
  }, [settingsOpen, keyConfigured]);
  // Which part of Settings' Account tab to scroll to on opening, for a caller
  // that sends the user to press a button there (the speech-setup card, #384).
  const [settingsReveal, setSettingsReveal] = useState<"speech-model" | "recording-latency" | null>(null);
  // Every other way in (the gear, an error's button) opens at the top, so a
  // request to reveal ends with the dialog it was for.
  useEffect(() => {
    if (!settingsOpen) setSettingsReveal(null);
  }, [settingsOpen]);
  const [showShortcuts, setShowShortcuts] = useState(false);
  const [compareMode, setCompareMode] = useState<CompareMode | null>(null);
  const [selection, setSelection] = useState<Selection | null>(null);
  const timelineRef = useRef<TimelineHandle>(null);
  // The label lane's labels: re-read when a label changes and whenever
  // the head moves, since undo does not announce itself (#453).
  const markers = useMarkers();
  // What a render of the node shown leaves out: a stored effect this build
  // cannot run (#532). Read off the node, so Play and Export need not
  // render first to learn of it, and it clears when a head move lands on a
  // state without the effect. A/B compare plays renders of its own two
  // nodes and says what they leave out itself.
  const renderWarnings = useRenderWarnings(compareMode === null ? shownNode : null);
  // What the last export was written without, until dismissed or the head
  // moves on (#532): it is about a file made from the head it was exported
  // at, and means nothing next to another project's.
  const [exportedWithout, setExportedWithout] = useState<{
    file: string;
    warnings: readonly RenderWarning[];
  } | null>(null);
  useEffect(() => {
    setExportedWithout(null);
  }, [head]);
  // The latest list, for handlers that must compare against what the
  // session holds now rather than what they closed over.
  const tracks = sessionTracks;
  const tracksRef = useRef(sessionTracks);
  useEffect(() => {
    tracksRef.current = sessionTracks;
  }, [sessionTracks]);
  /**
   * What the timeline draws and the status bar names: the session's own
   * audio, whichever track holds it. The track itself is kept for its
   * name, which the status bar shows when that audio is an edit's
   * output rather than a file the user brought (#416).
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
  const sourceTrack = useMemo(
    () => tracks.find((t) => t.audio_path) ?? null,
    [tracks],
  );
  const timelineSource = sourceTrack?.audio_path ?? null;
  /**
   * What the timeline draws its lanes from: the session's list with the
   * controls the user holds laid over it (#596).
   *
   * Memoised, so the timeline is handed a new list only when the session's
   * list changed or a control was handed back. It used to be handed a new
   * array on every render of the app, and the timeline copies what it is
   * handed into its lanes each time.
   */
  const laneTracks = useMemo(
    () =>
      heldTracks
        // A track with no audio still gets a lane (#742): a template's
        // tracks, and one the assistant has added, hold no clips yet. The
        // lane's `index` is the track's place in the session's list, which
        // the mixer commands address it by.
        .map((t, index) => ({
          index,
          name: t.name,
          audioPath: t.audio_path ?? "",
          muted: t.muted,
          gainDb: t.gain_db,
          pan: t.pan,
          soloed: t.soloed,
          clips: t.clips,
        })),
    [heldTracks],
  );
  const selectionTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [zoomPxPerSec, setZoomPxPerSec] = useState(0);
  // Whether the head lane's audio actually decoded. The status bar
  // used to infer "ready" from a path alone — from a path having been
  // *chosen* — so it reported ready for a file that 404'd.
  const [audioLoadError, setAudioLoadError] = useState<string | null>(null);
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
  // A punch-in is running (#203): the microphone and the speakers are its.
  const [punching, setPunching] = useState(false);
  // A punch runs outside the edit queue. A successful open or Save As may
  // therefore have installed another project before its answer arrives.
  const projectGenerationRef = useRef(0);
  // A scheduled recording is armed (waiting or recording), which `isRecording`
  // also covers; named apart so the Punch control can say which it is.
  const [timerArmed, setTimerArmed] = useState(false);
  // What the last punch-in was placed with, said once.
  const [punchNotice, setPunchNotice] = useState<string | null>(null);
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

  // One undo or redo at a time: moving onto history whose audio was
  // swept rebuilds it first, which can take seconds (#373).
  const headMove = useHeadMove();
  const runHeadMove = headMove.run;

  // Undo and redo are moves: in the session's line behind any edit still on
  // its way, decided only when it is their turn (#498), and one at a time here.
  const handleUndo = useCallback(async () => {
    await runHeadMove(async () => {
      try {
        await move({ kind: "undo" });
      } catch (err) {
        setRenderError(String(err));
      }
    });
  }, [runHeadMove, move]);

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
        await edit("setSyncLock", [enabled]);
        setSyncLockState(enabled);
      } catch (err) {
        setRenderError(String(err));
      }
    },
    [edit],
  );

  const handleRedo = useCallback(async () => {
    await runHeadMove(async () => {
      try {
        await move({ kind: "redo" });
      } catch (err) {
        setRenderError(String(err));
      }
    });
  }, [runHeadMove, move]);

  // Window-level keyboard transport. Active whenever the user isn't
  // typing into a chat input / settings field. Space toggles
  // play/pause; Home/End jump to start/end; ←/→ seek 5 s; Shift+←/→
  // seek 1 s.
  useEffect(() => {
    const handleTimelineKey = (e: KeyboardEvent) => {
      const t = timelineRef.current;
      if (!t) return;
      if (e.key === " ") {
        e.preventDefault();
        // One press toggles once, even when the key is held (#431).
        if (!e.repeat) t.togglePlay();
      } else if (e.key === "Home") {
        e.preventDefault(); t.seekTo(0);
      } else if (e.key === "End") {
        e.preventDefault(); t.seekTo(t.getDuration());
      } else if (e.key === "ArrowLeft") {
        e.preventDefault(); t.seekBy(e.shiftKey ? -1 : -5);
      } else if (e.key === "ArrowRight") {
        e.preventDefault(); t.seekBy(e.shiftKey ? 1 : 5);
      } else {
        handleViewKey(e, t);
      }
    };
    const handleViewKey = (e: KeyboardEvent, t: NonNullable<typeof timelineRef.current>) => {
      if (e.key === "Escape" && selection && !showShortcuts && !showTemplatePicker) {
        e.preventDefault(); setSelection(null);
      } else if (e.key === "+" || e.key === "=") {
        e.preventDefault(); setZoomPxPerSec((z) => Math.min(z + 40, 2000));
      } else if (e.key === "-") {
        e.preventDefault(); setZoomPxPerSec((z) => Math.max(z - 40, 0));
      } else if (e.key === "0") {
        e.preventDefault(); setZoomPxPerSec(0);
      } else if ((e.key === "e" || e.key === "E") && (e.metaKey || e.ctrlKey)) {
        e.preventDefault(); t.zoomToSelection();
      } else if ((e.key === "f" || e.key === "F") && (e.metaKey || e.ctrlKey)) {
        e.preventDefault(); t.fitToWindow();
      }
    };
    const onKey = (e: KeyboardEvent) => {
      // Ctrl/Cmd+K closes the palette as well as opening it, so it is
      // answered before the open-dialog check below stands the app down.
      if ((e.metaKey || e.ctrlKey) && (e.key === "k" || e.key === "K") && paletteOpen) {
        e.preventDefault();
        setPaletteOpen(false);
        return;
      }
      if (hasOpenDialog()) return;
      const target = e.target as HTMLElement | null;
      const tag = target?.tagName ?? "";
      const isTyping =
        tag === "INPUT" || tag === "TEXTAREA" || target?.isContentEditable;
      // Undo and redo ask a narrower question than the keys below: a
      // focused slider is not text entry, so Ctrl+Z still undoes the
      // fader edit that put focus there. Space, Home, End and the
      // arrows stay with `isTyping`, because on a slider they move it.
      const inTextField = isTextEntry(target);
      if (isUndoChord(e) && !inTextField) {
        e.preventDefault();
        handleUndo();
        return;
      }
      if (isRedoChord(e) && !inTextField) {
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
        setPaletteOpen(true);
        return;
      }
      // The picker can be opening before its dialog has mounted.
      if (e.key === "?" && !e.ctrlKey && !e.altKey && !e.metaKey && !isTyping && !showTemplatePicker) {
        e.preventDefault();
        setShowShortcuts(true);
        return;
      }
      if ((e.key === "l" || e.key === "L") && !isTyping) {
        e.preventDefault();
        setLoopActive((v) => !v);
        return;
      }
      if (!isTyping) handleTimelineKey(e);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [selection, showShortcuts, showTemplatePicker, paletteOpen, handleUndo, handleRedo]);

  useEffect(() => {
    let cancelled = false;
    hasApiKey()
      .then((ok) => {
        if (cancelled) return;
        setKeyConfigured(ok);
        setKeyCheckPending(false);
      })
      .catch((err) => {
        // The keychain could not be read. That is not "no key": the old
        // `setKeyConfigured(false)` put the first-run Welcome over a key
        // that was stored, and entering a new one overwrote it (#394).
        // `keyConfigured` stays null, so no blocking prompt mounts, and
        // the banner says what happened and offers Settings.
        if (cancelled) return;
        setKeychainError(String(err));
        setKeyCheckPending(false);
      });

    // Whether what gets saved will survive a reboot. A failure to ask is
    // not worth a banner of its own.
    getKeychainPersistence()
      .then((p) => {
        if (cancelled) return;
        setKeychainPersistence(p);
        // Persistent again (a Secret Service was started): forget the
        // dismissal, so the warning comes back if it ever stops being.
        if (p.persistent) {
          writeSessionOnlyDismissed(false);
          setSessionOnlyDismissed(false);
        }
      })
      .catch(() => undefined);

    // Keep the 8 bundled skill files current in ~/.edytlab/skills/: install
    // the missing ones, update those the user has not edited. Fire-and-forget
    // — non-fatal if the resource dir is absent (dev mode).
    installBundledSkills().catch(() => {
      // Non-fatal: bundled skills may not be available in dev mode
    });

    return () => {
      cancelled = true;
    };
  }, []);

  // No handler adopts a head, clears the mix or reads the tracks again after
  // an edit (#232, #498): `edit` and `move` do, from the table in
  // `lib/sessionCommands.ts`, and a mix rendered before the session changed
  // is no mix (`edition`, above).

  /** `batch_load` as one turn in the session's line, its head adopted inside the turn. */
  const loadInOrder = useCallback((paths: string[]) => edit("batchLoad", [paths]), [edit]);

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
        const result = await loadInOrder(paths);
        setRenderError(describeLoadFailures(result.failures));
      } catch (err) {
        const what = paths.length === 1 ? trimPath(paths[0]) : `${paths.length} files`;
        setRenderError(`Could not load ${what}: ${String(err)}`);
      }
    },
    [loadInOrder],
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
      await edit("applyTemplate", [name]);
    } catch (e) {
      setRenderError(String(e));
    }
  }, [edit]);

  // Mixer commits. Each command appends one session node, so the head
  // moves and the track list is read again: the Timeline shows the value
  // optimistically, and this is what confirms or corrects it.
  //
  // A rejected value (out of range, track gone) surfaces in the error
  // banner and the read puts the control back where the session actually
  // is, rather than leaving the fader lying.
  //
  // Until that read has landed the control is the user's (#596): a track
  // list read before the commit was answered, landing meanwhile, would put
  // the older value back under it. `hold` names the control the edit commits
  // (`useSession`, `lib/heldControls.ts`); the control goes back to the
  // session once the list read after the answer is in. That read hands out
  // fresh track and clip references even for identical data, so a refused
  // edit still redraws the lane from the session's value.
  //
  // In line (#498): two commits made in quick succession reach the
  // session in the order they were made, so the last value set is the one
  // it keeps.
  const commit = useCallback(
    async <K extends EditName>(name: K, args: EditArgs<K>, hold?: HeldInput) => {
      try {
        await edit(name, args, { hold });
      } catch (e) {
        setRenderError(String(e));
      }
    },
    [edit],
  );

  // Release, key-up and blur all commit a slider, and most carry the value
  // the session already holds: see `isSessionValue` for the undo it broke.
  //
  // "Already what the session holds" means what it will hold once the
  // commits ahead of this one in the line have been applied, not what the
  // last read showed (#498). Against the read alone, 0 -> -3 -> 0 made in
  // one window drops the final 0, because it matches the 0 on screen, and
  // the session keeps -3. The last value sent for a control is kept by
  // `useSession` until a track list read after that commit has landed, so
  // the track list is the truth again.
  //
  // A slider is the user's from its first input event (#596), not from its
  // commit: key-up comes after the input, and a track list landing in
  // between used to put the slider back, so the key-up committed the value
  // the session held and nothing was sent.
  const commitSlider = useCallback(
    <K extends "setTrackGain" | "setTrackPan">(
      index: number,
      control: "gain" | "pan",
      shown: number | undefined,
      value: number,
      name: K,
      args: EditArgs<K>,
    ) => {
      const sent = sentValue(index, control);
      if (isSessionValue(typeof sent === "number" ? sent : shown, value)) {
        dropInput(index, control);
        return;
      }
      void commit(name, args, { index, control, value });
    },
    [commit, sentValue, dropInput],
  );
  const handleTrackGainChange = useCallback(
    (index: number, gainDb: number) =>
      commitSlider(index, "gain", tracksRef.current[index]?.gain_db, gainDb, "setTrackGain", [
        index,
        gainDb,
      ]),
    [commitSlider],
  );
  const handleTrackPanChange = useCallback(
    (index: number, pan: number) =>
      commitSlider(index, "pan", tracksRef.current[index]?.pan, pan, "setTrackPan", [index, pan]),
    [commitSlider],
  );
  // A slider's input event: the value is the user's from here until the
  // session confirms it.
  const handleTrackGainInput = useCallback(
    (index: number, gainDb: number) => holdInput({ index, control: "gain", value: gainDb }),
    [holdInput],
  );
  const handleTrackPanInput = useCallback(
    (index: number, pan: number) => holdInput({ index, control: "pan", value: pan }),
    [holdInput],
  );
  const handleTrackMuteChange = useCallback(
    (index: number, muted: boolean) =>
      void commit("setTrackMuted", [index, muted], { index, control: "muted", value: muted }),
    [commit],
  );
  const handleClipEnvelopeChange = useCallback(
    (trackIndex: number, clipIndex: number, points: EnvelopePoint[]) =>
      void commit("setClipEnvelope", [trackIndex, clipIndex, points]),
    [commit],
  );

  const handleMoveClip = useCallback(
    (trackIndex: number, clipIndex: number, startSec: number) =>
      void commit("moveClip", [trackIndex, clipIndex, startSec]),
    [commit],
  );

  const handleRemoveClip = useCallback(
    (trackIndex: number, clipIndex: number) =>
      void commit("removeClip", [trackIndex, clipIndex]),
    [commit],
  );

  const handleTrackSoloChange = useCallback(
    (index: number, soloed: boolean) =>
      void commit("setTrackSoloed", [index, soloed], { index, control: "soloed", value: soloed }),
    [commit],
  );

  // Track-head actions (#161). Each is one existing tool, one node, and
  // undoes like any other edit — which is why removing does not stop to
  // ask. The list is read again like any edit's, since these change the
  // list itself rather than a value on a track.
  const handleRenameTrack = useCallback(
    (index: number, name: string) => void commit("renameTrack", [index, name]),
    [commit],
  );

  const handleDuplicateTrack = useCallback(
    (index: number) => void commit("duplicateTrack", [index]),
    [commit],
  );

  const handleRemoveTrack = useCallback(
    (index: number) => void commit("removeTrack", [index]),
    [commit],
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
   * Put the user back where they were in a project they just opened:
   * zoom, selection and playhead from its saved view, and the head the
   * store itself reported (`ProjectInfo.head`, its own `HEAD`).
   *
   * Everything but the saved view's head, as at launch. The view is
   * written 500 ms after it settles, so its head is the store's `HEAD` or
   * older: older when an edit landed in the last half-second before the
   * project was closed, or the head moved in a way the app did not adopt.
   * Obeying it meant `set_head_to`, which moves `HEAD` back *on disk* and
   * takes the newer edits off the timeline (#507). The saved head is not
   * even looked at: a node `view.json` names that the store lacks (a
   * folder copied without `.audiograph/`) is no longer a case to
   * tolerate, because nothing is asked of the store.
   *
   * The path the user took through the project is read here too (#453),
   * beside the view, and answered to `move`, which resets the head with it.
   * It applies only if it ends at the store's head: a path that ends
   * elsewhere (the last save was lost to a crash, another client moved the
   * store) is not the way to this head, and undo then asks the node for its
   * parent. A path that cannot be read is no path; it never stops a project
   * opening.
   */
  const restoreView = useCallback(
    async () => {
      const [view, path] = await Promise.all([
        getViewState().catch(() => null),
        Promise.resolve()
          .then(() => getHeadTrail())
          .then(readSavedTrail)
          .catch(() => null),
      ]);
      applyView(viewToApply(view), "replace");
      // A preview of the project that was left is nothing in this one, even
      // when both happen to be on the same head (#522).
      clearPreview();
      return path;
    },
    [applyView, clearPreview],
  );

  /**
   * Reopening a project is opening it: same command, so the recents
   * row moves to the top and `project.json` records the visit exactly
   * as it would from the file dialog.
   */
  const handleOpenRecent = useCallback(
    async (path: string) => {
      try {
        // In line: an edit made before this goes to the project being
        // left, and one made after it to the project being opened (#498).
        // The held controls of the project that was left are let go of, and
        // every read is made again.
        await move({
          kind: "open",
          path,
          restore: async () => {
            projectGenerationRef.current += 1;
            return restoreView();
          },
        });
        setRecents(await listRecentProjects());
      } catch (e) {
        setRenderError(String(e));
      }
    },
    [restoreView, move],
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
      // In line: it moves the session to the copy, as opening does (#498).
      const report = await move({
        kind: "saveAs",
        dest: dir,
        adopted: () => {
          projectGenerationRef.current += 1;
        },
      });
      setRecents(await listRecentProjects());
      // Not an error, so it does not go through the error banner — but
      // the numbers are worth seeing, since a copy that skipped the
      // caches is smaller than the folder it came from and that would
      // otherwise look like data loss.
      // eslint-disable-next-line no-console
      console.info(
        `Saved a copy to ${dir}: ${report.files} files, ` +
          `${(report.bytes / 1e6).toFixed(1)} MB, ` +
          `${report.skipped_cache_files} cached preview and audition file(s) left behind.`,
      );
    } catch (e) {
      setRenderError(String(e));
    }
  }, [persistView, move]);

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

  // A label with its name already typed: what the lane's box hands over.
  const handleAddLabel = useCallback(
    async (timeSec: number, name: string) => {
      try {
        await edit("addMarker", [timeSec, name]);
        // The lane reads again when the edit settles (`useMarkers`).
      } catch (err) {
        setRenderError(String(err));
      }
    },
    [edit],
  );

  // The ruler hands over a time and no name. The name is typed into the
  // label lane's own box, opened at that time, and comes back through
  // `handleAddLabel` like any label's. It was asked for with the
  // browser's prompt dialog, which a macOS webview answers with `null`,
  // so there a click on the ruler did nothing (#546).
  const [nameRequest, setNameRequest] = useState<{ time: number } | null>(null);
  const handleRulerAdd = useCallback((timeSec: number) => {
    setNameRequest({ time: timeSec });
  }, []);
  const handleNameRequestHandled = useCallback(() => setNameRequest(null), []);

  const handleRemoveMarker = useCallback(async (id: string) => {
    try {
      await edit("removeMarker", [id]);
    } catch (err) {
      setRenderError(String(err));
    }
  }, [edit]);

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

  // The transcript at the current head, and where it came from.
  const transcript = useTranscript();

  // The track the transcript's words are of, or none. A word's time is
  // seconds of the audio it was made from, so cutting by it cuts that
  // track and no other (#384): the pane's Delete does nothing for a
  // transcript of a file, or of a track that has been removed, and says why.
  const transcriptCutTrack = cutTrack(transcript.source);

  const handleCutWords = useCallback(
    async (from: number, to: number) => {
      if (transcriptCutTrack === null) return;
      try {
        await edit("cutTranscriptWords", [transcriptCutTrack, from, to]);
        setSelection(null);
      } catch (err) {
        setRenderError(String(err));
      }
    },
    [edit, transcriptCutTrack],
  );

  const handleRenameMarker = useCallback(async (id: string, name: string) => {
    try {
      await edit("updateMarker", [id, { name }]);
    } catch (err) {
      setRenderError(String(err));
    }
  }, [edit]);

  const handleMoveMarker = useCallback(async (id: string, timeSec: number) => {
    try {
      await edit("updateMarker", [id, { time: timeSec }]);
    } catch (err) {
      setRenderError(String(err));
    }
  }, [edit]);

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
    // The tracks are read by `useSession`: the backend reopens the default
    // project at every launch and restores its `HEAD`, so a returning
    // user's tracks are already there; a fresh project has no head yet and
    // answers `NoSession`, which leaves the list empty. And the assistant's
    // nodes (`agent://node-created`) are heard there too, as moves.
    //
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
  }, [applyView]);

  const handleExportSelection = useCallback(async () => {
    if (!head || !selection || exporting) return;
    setExporting(true);
    setRenderError(null);
    setExportedWithout(null);
    try {
      const outPath = await save({
        title: "Export Selection",
        filters: [{ name: "WAV", extensions: ["wav"] }],
        defaultPath: "export.wav",
      });
      if (!outPath) { setExporting(false); return; }
      const { warnings } = await renderRange(head, selection.start, selection.end, outPath);
      // The file is written either way; when it is missing something the
      // user stored, the one place they look for the result says so.
      if (warnings.length > 0) setExportedWithout({ file: trimPath(outPath), warnings });
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
      loadInOrder,
    );

    // The button flips back whatever happened — the recorder has
    // stopped either way. What changed is that a lost take no longer
    // presents exactly like a successful stop.
    setIsRecording(false);

    // The head of the take is adopted by `loadInOrder`, inside its turn.
    // Adopting it again here would be outside any turn, and could put the
    // head back to a node a later edit has left.
    if (outcome.kind !== "loaded") setRenderError(outcome.message);
  }, [loadInOrder]);

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
      setTimerArmed(true);
      void scheduledTake(
        () => timerRecord(`recording_${Date.now()}.wav`, schedule),
        loadInOrder,
      ).then((outcome) => {
        setIsRecording(false);
        setTimerArmed(false);
        // Its head is adopted by `loadInOrder`, in its turn.
        if (outcome.kind !== "loaded" && outcome.kind !== "cancelled") {
          // Cancelling is the user's own doing and needs no banner.
          setRenderError(outcome.message);
        }
      });
    },
    [loadInOrder],
  );

  /**
   * Punch in (#203): replace the selected region of a track with a take
   * recorded while the rest of the mix plays.
   *
   * The transport is paused first: the backend plays the mix itself, through
   * the speakers, on a clock it shares with the microphone, and a second
   * player from the webview would be in the take. `punch_record` resolves
   * when the take is placed, tens of seconds later at most, and is not
   * inside the edit line: that would hold every other edit for
   * its length. Only adopting the head is in the line, as for any command
   * that changes the session. The backend refuses a take if its project or
   * head changed before application. A later edit or project switch can
   * still beat the reply, so adoption re-reads the current head in the line
   * and lets a newer agent event win while that read is in flight.
   */
  const handlePunch = useCallback(
    async ({ track, prerollSec }: { track: number; prerollSec: number }) => {
      if (!selection || punching) return;
      setRenderError(null);
      setPunchNotice(null);
      timelineRef.current?.pause();
      setPunching(true);
      const projectGeneration = projectGenerationRef.current;
      let applied = false;
      try {
        const outcome = await punchOutcome(() =>
          punchRecord(track, selection.start, selection.end, prerollSec),
        );
        if (outcome.kind === "applied") {
          applied = true;
          // In line, and read when it is its turn. The response may arrive
          // after a later edit or project switch: the head is read in the
          // line and never adopted over a newer one, and the lane controls
          // of another project are not let go of.
          await move({
            kind: "reconcile",
            isCurrent: () => projectGenerationRef.current === projectGeneration,
          });
          // Even if an agent event won the head read, the punch was applied
          // in this project. Keep newer lane input but report the success.
          if (projectGenerationRef.current === projectGeneration) {
            setPunchNotice(appliedText(outcome));
          }
        } else if (outcome.kind === "failed") {
          // A refused take can be saved in the project the user just left;
          // the backend's message is the only place its recovery path appears.
          const leftProject = projectGenerationRef.current !== projectGeneration;
          setRenderError(leftProject ? `In the project you left: ${outcome.message}` : outcome.message);
        } else if (outcome.kind === "uncalibrated" && projectGenerationRef.current === projectGeneration) {
          // A refusal or a failure says what to try; "not calibrated" also
          // gets the button that goes there.
          setRenderError(outcome.message);
        }
      } catch (error) {
        setRenderError(applied
          ? `The punch was applied, but the current project could not be refreshed: ${String(error)}. Reopen the project before retrying.`
          : String(error));
      } finally {
        setPunching(false);
      }
    },
    [selection, punching, move],
  );

  // Renders what is shown: the previewed node when the graph has one, else
  // the head. Play asks for this when the mix is not a render of it.
  //
  // Answers the path it rendered, or null when it did not render (a render
  // already under way, nothing shown, or a failure, which it reports): the
  // timeline asks for the mix again with this when the file has gone (#607).
  const handleRenderPreview = useCallback(async (): Promise<string | null> => {
    if (!shownNode || rendering) return null;
    setRendering(true);
    setRenderError(null);
    try {
      const path = previewId ? await bridgeRenderPreview(previewId) : await renderHead();
      setMix(path, shownNode);
      return path;
    } catch (err) {
      setRenderError(String(err));
      return null;
    } finally {
      setRendering(false);
    }
  }, [shownNode, previewId, rendering, renderHead, setMix]);

  /**
   * A click on a bubble in the history graph (#522): look at that node, do
   * not go there. It renders the node's mix and marks it as the preview;
   * the head, the undo path and the saved path stay where they were, and a
   * real head move (an edit, undo, "Set as head") ends the preview.
   */
  const handleSelectGraphNode = useCallback(
    async (nodeId: string) => {
      previewNode(nodeId);
      setRendering(true);
      setRenderError(null);
      try {
        const path = await bridgeRenderPreview(nodeId);
        setMix(path, nodeId);
      } catch (err) {
        setRenderError(String(err));
      } finally {
        setRendering(false);
      }
    },
    [previewNode, setMix],
  );

  /**
   * "Set as head" in the history graph (#453): a head move like undo's —
   * one at a time, the backend first, then the step on the path, so the
   * next undo comes back from it. Errors propagate to GraphView's
   * banner, as before.
   *
   * The move records the step on the path only once `set_head_to` has
   * resolved, so a refused move is not recorded. It is a no-op when the head already is
   * `nodeId`, which is right. This is the only way the graph moves the
   * head: a click on a bubble is a preview (`handleSelectGraphNode`, #522),
   * and the head move ends it. The mix needs no clearing: `mixIsCurrent` /
   * `mixIsStale` compare `mixNodeId` with the node shown.
   */
  const handleGraphSetHead = useCallback(
    async (nodeId: string): Promise<void> => {
      const moved = await runHeadMove(async () => {
        // In line, like undo: it moves the head the edits before it made.
        await move({ kind: "setHead", node: nodeId });
        return true;
      });
      // One head move at a time: an undo or redo that is still waiting its
      // turn behind an edit holds the slot, and the click would be dropped
      // in silence. Said in the graph's own banner, like a refused move,
      // so the user knows to click again.
      if (!moved) {
        throw new Error(
          "An undo, redo or head move is still running. Set the head again once it has finished.",
        );
      }
    },
    [runHeadMove, move],
  );

  /**
   * Naming a node in the history graph writes to the store, so it goes in
   * line with the edits (#498): two renames of one node cannot land the
   * older name last.
   */
  const handleGraphRenameNode = useCallback(
    (nodeId: string, label: string): Promise<void> =>
      edit("renameNode", [nodeId, label]),
    [edit],
  );

  /**
   * A/B compare's "Accept B" moves the session head, so it is a move in the
   * line, and the head it names is adopted inside the turn. The bar calls
   * `onAcceptB` only after the turn has ended, which is too late to adopt
   * anything (#498).
   *
   * It ends as Set as head does: the controls the user holds are let go of
   * and every read is made again (#596). Nothing else reads the tracks on a
   * head move, so the lanes went on showing side A's tracks, with the user's
   * values over them.
   */
  const handleCompareAcceptB = useCallback(
    // A read that fails after the move is reported (`onReadError`), not
    // taken for a refused accept.
    (b: string): Promise<string> => move({ kind: "acceptB", b }),
    [move],
  );

  const handleCompareNodes = useCallback(
    (bNodeId: string) => {
      if (!head) return;
      setCompareMode({ a: head, b: bNodeId });
    },
    [head],
  );

  // The head moved in `handleCompareAcceptB`'s turn; this only leaves the
  // comparison.
  const handleAcceptB = useCallback(() => {
    setCompareMode(null);
  }, []);

  // The path taken, for the history graph to draw (#398).
  const historyPath = useMemo(() => (head ? [...trail, head] : trail), [trail, head]);

  const handleCloseShortcuts = useCallback(() => setShowShortcuts(false), []);

  // Not while the panel is open (#250). `keyConfigured` now updates on a
  // provider switch, and without this guard flipping it false would
  // replace the panel the user is standing in — losing their tab and
  // anything typed — with the blocking prompt. They are already in the
  // right place; the panel says what happened. The prompt appears when
  // they close it and the app is still keyless.
  const showBlocking = keyConfigured === false && !settingsOpen;

  // Whether Play can start what is loaded as it is. A/B compare puts its
  // own renders in `mixPath` — they belong to no head — and Play there
  // plays the side that is chosen, never a render of the head. A node the
  // graph is previewing is what is shown, so its render is the current one
  // (#522).
  const mixMatchesShown =
    compareMode !== null || mixIsCurrent({ mixPath, mixNodeId }, shownNode);

  const notice = keychainNotice(keychainError, keychainPersistence);
  // The same words in Settings: the blocking dialog covers the banner,
  // and it is where a user who lost their key to a reboot lands.
  const storageWarning =
    keychainNotice(null, keychainPersistence)?.message ?? null;

  const errorAction = useMemo(() => {
    if (!renderError) return undefined;
    // A punch-in with no delay kept for the devices (#203): the way out is
    // the Audio tab, where it is measured or typed.
    if (isUncalibratedMessage(renderError)) {
      return {
        label: "Open Settings → Audio",
        onClick: () => {
          setRenderError(null);
          setSettingsReveal("recording-latency");
          setSettingsOpen(true);
        },
      };
    }
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
    // No entry animation on `<main>`, or on anything else above the
    // timeline: it is an ancestor of every WaveSurfer canvas, and an
    // animated ancestor re-composites them (docs/motion-audit.md).
    <main className="grid h-screen w-screen grid-rows-[auto_1fr_auto] bg-[var(--bg)] text-[var(--text)]">
      <AppHeader
        leftView={leftView}
        onSelectView={setLeftView}
        onOpen={handleOpenDialog}
        onSettings={() => setSettingsOpen(true)}
        isRecording={isRecording}
        onRecord={isRecording ? handleStopRecording : handleStartRecording}
        onTimerRecord={handleTimerRecord}
        punch={{
          tracks,
          selection,
          punching,
          timerArmed,
          onPunch: handlePunch,
          onOpenAudioSettings: () => {
            setSettingsReveal("recording-latency");
            setSettingsOpen(true);
          },
        }}
        onSaveAs={handleSaveProjectAs}
        hasProject={Boolean(head)}
        onNewProject={handleNewProject}
        onOpenProject={handleOpenProject}
      />

      <div className="grid min-h-0 grid-cols-[minmax(0,1fr)_360px] gap-px bg-[var(--border)]">
        <section className="flex h-full min-h-0 flex-col bg-[var(--surface)]">
          <KeyringWaiting pending={keyCheckPending} />
          {notice?.kind === "read-error" ? (
            <ErrorBanner
              testId="keychain-error"
              message={notice.message}
              action={{
                label: "Open Settings",
                onClick: () => setSettingsOpen(true),
              }}
              onDismiss={() => setKeychainError(null)}
            />
          ) : null}
          {notice?.kind === "session-only" && !sessionOnlyDismissed ? (
            <ErrorBanner
              testId="keychain-session-only"
              message={notice.message}
              dismissLabel="Dismiss warning"
              onDismiss={() => {
                writeSessionOnlyDismissed(true);
                setSessionOnlyDismissed(true);
              }}
            />
          ) : null}
          {renderError ? (
            <ErrorBanner
              testId="render-error"
              message={renderError}
              action={errorAction}
              onDismiss={() => setRenderError(null)}
            />
          ) : null}
          {punchNotice ? (
            <ErrorBanner
              tone="info"
              testId="punch-notice"
              message={punchNotice}
              dismissLabel="Dismiss"
              onDismiss={() => setPunchNotice(null)}
            />
          ) : null}
          <RenderWarningsNotice notice={mixNotice(renderWarnings)} />
          {exportedWithout ? (
            <RenderWarningsNotice
              testId="export-warnings"
              notice={exportWarningNotice(exportedWithout.file, exportedWithout.warnings)}
              onDismiss={() => setExportedWithout(null)}
            />
          ) : null}

          {/* Above the timeline so a long run is visible wherever the
              user is looking, and gone again the moment it finishes. */}
          <ToolProgressBar />

          {compareMode ? (
            <ABCompareBar
              aNodeId={compareMode.a}
              bNodeId={compareMode.b}
              onAudioPathChange={setMix}
              acceptB={handleCompareAcceptB}
              onAcceptB={handleAcceptB}
              onClose={() => setCompareMode(null)}
            />
          ) : null}

          <div className="flex-1 min-h-0 overflow-hidden">
            {leftView === "timeline" ? (
              timelineSource || tracks.length > 0 ? (
                <Timeline
                  ref={timelineRef}
                  audioPath={timelineSource}
                  tracks={laneTracks}
                  onTrackGainInput={handleTrackGainInput}
                  onTrackPanInput={handleTrackPanInput}
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
                  onAddMarker={handleRulerAdd}
                  onRemoveMarker={handleRemoveMarker}
                  onSeekToMarker={handleSeekToMarker}
                  zoom={zoomPxPerSec}
                  onZoomChange={setZoomPxPerSec}
                  onLoadErrorChange={setAudioLoadError}
                  mixPath={mixPath}
                  // Play renders the shown node's preview (the graph's
                  // preview, else the head) when this is false, then plays
                  // it (#431). A/B compare supplies its own mixes, which
                  // belong to no head, so it never asks.
                  mixCurrent={mixMatchesShown}
                  rendering={rendering}
                  onRequestRender={handleRenderPreview}
                  // The mix's file was evicted from the preview cache while
                  // the timeline was away: render it again, once (#607). Not
                  // during a compare, whose sides belong to no head and are
                  // held by the backend; its renders are not Play's to make.
                  onMixMissing={compareMode === null ? handleRenderPreview : undefined}
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
                      onAdd={handleAddLabel}
                      nameRequest={nameRequest}
                      onNameRequestHandled={handleNameRequestHandled}
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
                words={transcript.words}
                cutBlockedReason={cutBlockedReason(transcript.source)}
                selection={selection}
                onSelectRange={handleSelectionChange}
                onCutWords={handleCutWords}
                onSeek={handleSeekToMarker}
              />
            ) : (
              <GraphView
                head={head}
                preview={previewId}
                onSelectNode={handleSelectGraphNode}
                onCompareNodes={handleCompareNodes}
                onSetHead={handleGraphSetHead}
                onRenameNode={handleGraphRenameNode}
                path={historyPath}
              />
            )}
          </div>
        </section>

        <aside className="h-full min-h-0 bg-[var(--surface)]">
          <Chat ref={chatRef} 
            providerEpoch={providerEpoch}
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
            onOpenSettings={(reveal) => {
              setSettingsReveal(reveal ?? null);
              setSettingsOpen(true);
            }}
          />
        </aside>
      </div>

      <StatusBar
        audioPath={timelineSource}
        trackName={sourceTrack?.name}
        head={head}
        previewId={previewId}
        rendering={rendering}
        selection={selection}
        mixStale={mixIsStale({ mixPath, mixNodeId }, shownNode)}
        mixMissing={compareMode === null && mixIsMissing({ mixPath, mixNodeId })}
        loadError={audioLoadError}
        restoringHistory={headMove.pending}
      />

      {showBlocking ? (
        <Settings
          mode="blocking"
          storageWarning={storageWarning}
          onSaved={() => {
            setKeyConfigured(true);
            setKeychainError(null);
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
          reveal={settingsReveal}
          storageWarning={storageWarning}
          onClose={() => setSettingsOpen(false)}
          onSaved={() => {
            setSettingsOpen(false);
            // A save went through, so the keychain is readable now.
            setKeychainError(null);
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
  /**
   * The name of the track `audioPath` belongs to. Shown in its place when
   * the path is derived audio, whose file name is a content hash (#416).
   */
  trackName?: string | null;
  head: string | null;
  /**
   * The node the history graph is previewing, when it is not the head
   * (#522). The head label keeps saying where the session is; this says
   * that what is loaded to play is a render of another node.
   */
  previewId?: string | null;
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
   * True when no mix has been rendered at all — after opening a file or
   * any edit. `mixStale` is false then (there is nothing to be out of
   * date), so without this the bar said "ready" over a session that
   * would play nothing, and nothing said why (#431). It tells the user
   * how to hear it; Play renders first.
   *
   * Not shown while a render is under way — one is already doing what
   * the hint asks for, and a hint flashing up between a click and its
   * result reads as the click having failed.
   */
  mixMissing?: boolean;
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
  /**
   * An undo or redo has been moving the head for a while — rebuilding
   * history's audio that was swept to keep the project under its cap
   * (#373). Shown in place of the state word, which would otherwise say
   * `ready` while the key looks like it did nothing.
   */
  restoringHistory?: boolean;
}

export function StatusBar({
  audioPath,
  trackName,
  head,
  previewId = null,
  rendering,
  selection,
  mixStale,
  mixMissing = false,
  loadError,
  restoringHistory = false,
}: StatusBarProps) {
  const failed = Boolean(audioPath) && Boolean(loadError);
  // Only over audio that loaded: a file that failed to decode has bigger
  // news, and with no file there is nothing to render.
  const showMixMissing =
    mixMissing && Boolean(audioPath) && !failed && !rendering && !restoringHistory;
  // A file the user loaded is named by its file name, with the whole path
  // on hover. After a destructive edit the track reads a derived file
  // named by its hash, so the bar names the track instead — the name the
  // lane header shows — and leaves the hash out of the tooltip too.
  const derived = audioPath !== null && isDerivedAudioPath(audioPath);
  const fileLabel = !audioPath
    ? "no file loaded"
    : derived
      ? trackName?.trim() || "edited audio"
      : trimPath(audioPath);
  const fileTitle = derived ? fileLabel : (audioPath ?? undefined);
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
      <span
        className="flex items-center gap-1.5"
        data-testid="status-bar-state"
        role="status"
        aria-live="polite"
      >
        <span
          aria-hidden="true"
          className={
            "h-1.5 w-1.5 rounded-full " +
            (rendering || restoringHistory
              ? "bg-[var(--warning)] animate-pulse"
              : failed
                ? "bg-[var(--danger)]"
                : audioPath
                  ? "bg-[var(--success)]"
                  : "bg-[var(--text-faint)]")
          }
        />
        {restoringHistory
          ? "restoring history…"
          : rendering
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
      {showMixMissing ? (
        <>
          <span className="text-[var(--text-faint)]/80">·</span>
          <span
            data-testid="status-bar-mix-missing"
            className="text-[var(--warning)]"
            title="Nothing has been rendered for this session yet, so there is nothing to hear. Play renders it first."
          >
            preview not rendered — press Play or Preview
          </span>
        </>
      ) : null}
      <span className="text-[var(--text-faint)]/80">·</span>
      <span data-testid="status-bar-file" title={fileTitle}>
        {fileLabel}
      </span>
      <span className="text-[var(--text-faint)]/80">·</span>
      <span data-testid="status-bar-head">{headLabel}</span>
      {previewId ? (
        <>
          <span className="text-[var(--text-faint)]/80">·</span>
          <span
            data-testid="status-bar-preview"
            className="text-[var(--accent)]"
            title="The graph is previewing this node. The head has not moved; Set as head in the graph moves it."
          >
            previewing {previewId.slice(0, 7)}
          </span>
        </>
      ) : null}
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
      <span data-testid="status-bar-version" className="ml-auto text-[var(--text-faint)]">
        v{__APP_VERSION__}
      </span>
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
