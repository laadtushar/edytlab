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
import type { Marker, TrackSummary } from "./lib/tauri-bridge";
import {
  addMarker,
  getNode,
  listMarkers,
  listTracks,
  onMarkerChanged,
  removeMarker,
  setHeadTo,
  setSelectionContext,
} from "./lib/tauri-bridge";
import { applyUndo, applyRedo } from "./lib/undoRedo";

import { ABCompareBar } from "./components/ABCompareBar";
import { Chat } from "./components/Chat";
import { EmptyState } from "./components/EmptyState";
import { ErrorBanner } from "./components/ErrorBanner";
import { GraphView } from "./components/GraphView";
import { Settings } from "./components/Settings";
import {
  Timeline,
  type Selection,
  type TimelineHandle,
} from "./components/Timeline";
import { useSession } from "./hooks/useSession";
import {
  hasApiKey,
  onNodeCreated,
  renderPreview as bridgeRenderPreview,
} from "./lib/tauri-bridge";
import { listenToFileDrops, loadAudio, pickAudioFile } from "./lib/file-open";

type LeftView = "timeline" | "graph";

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
  const [audioPath, setAudioPath] = useState<string | null>(null);
  const [rendering, setRendering] = useState(false);
  const [renderError, setRenderError] = useState<string | null>(null);
  const [leftView, setLeftView] = useState<LeftView>("timeline");
  const [graphRefresh, setGraphRefresh] = useState(0);
  const [keyConfigured, setKeyConfigured] = useState<boolean | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [compareMode, setCompareMode] = useState<CompareMode | null>(null);
  const [selection, setSelection] = useState<Selection | null>(null);
  const timelineRef = useRef<TimelineHandle>(null);
  const [markers, setMarkers] = useState<Marker[]>([]);
  const [tracks, setTracks] = useState<TrackSummary[]>([]);
  const selectionTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [zoomPxPerSec, setZoomPxPerSec] = useState(0);
  const [redoStack, setRedoStack] = useState<string[]>([]);

  const handleUndo = useCallback(async () => {
    if (!head) return;
    const node = await getNode(head);
    const result = applyUndo(head, node.parent ?? null, redoStack);
    if (!result) return;
    await setHeadTo(result.head);
    setHeadLocal(result.head);
    setRedoStack(result.redoStack);
    const newTracks = await listTracks();
    setTracks(newTracks);
  }, [head, redoStack, setHeadLocal]);

  const handleRedo = useCallback(async () => {
    const result = applyRedo(redoStack);
    if (!result) return;
    await setHeadTo(result.head);
    setHeadLocal(result.head);
    setRedoStack(result.redoStack);
    const newTracks = await listTracks();
    setTracks(newTracks);
  }, [redoStack, setHeadLocal]);

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
      const t = timelineRef.current;
      if (!t) return;
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
      } else if (e.key === "Escape" && !isTyping && selection) {
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
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [selection, handleUndo, handleRedo]);

  useEffect(() => {
    let cancelled = false;
    hasApiKey()
      .then((ok) => {
        if (!cancelled) setKeyConfigured(ok);
      })
      .catch(() => {
        if (!cancelled) setKeyConfigured(false);
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
    void loadAudio(path, setAudioPath, (err) => setRenderError(err));
  }, []);

  const handleOpenDialog = useCallback(async () => {
    try {
      const path = await pickAudioFile();
      if (path) handleFileSelected(path);
    } catch (err) {
      setRenderError(String(err));
    }
  }, [handleFileSelected]);

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
    listenToFileDrops(handleFileSelected)
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [handleFileSelected]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    onNodeCreated(async (_nodeId: string) => {
      setRedoStack([]); // new branch clears forward history
      setGraphRefresh((n) => n + 1);
      const newTracks = await listTracks();
      setTracks(newTracks);
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

  const handleRenderPreview = useCallback(async () => {
    if (!head || rendering) return;
    setRendering(true);
    setRenderError(null);
    try {
      const path = await renderHead();
      setAudioPath(path);
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
        setAudioPath(path);
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
              onAudioPathChange={setAudioPath}
              onAcceptB={handleAcceptB}
              onClose={() => setCompareMode(null)}
            />
          ) : null}

          <div className="flex-1 min-h-0 overflow-hidden">
            {leftView === "timeline" ? (
              audioPath ? (
                <Timeline
                  ref={timelineRef}
                  audioPath={audioPath}
                  tracks={tracks
                    .filter((t) => t.audio_path)
                    .map((t) => ({
                      name: t.name,
                      audioPath: t.audio_path as string,
                      muted: t.muted,
                    }))}
                  onFileDropped={() => undefined}
                  selection={selection}
                  onSelectionChange={handleSelectionChange}
                  markers={markers}
                  onAddMarker={handleAddMarker}
                  onRemoveMarker={handleRemoveMarker}
                  onSeekToMarker={handleSeekToMarker}
                  zoom={zoomPxPerSec}
                  onZoomChange={setZoomPxPerSec}
                />
              ) : (
                <EmptyState onOpen={handleOpenDialog} />
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
          <Chat
            rendering={rendering}
            onRequestRenderPreview={handleRenderPreview}
            selection={selection}
            onClearSelection={() => {
              setSelection(null);
              void setSelectionContext(null).catch(() => undefined);
            }}
            markers={markers}
          />
        </aside>
      </div>

      <StatusBar
        audioPath={audioPath}
        head={head}
        rendering={rendering}
        selection={selection}
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
    </main>
  );
}

interface AppHeaderProps {
  leftView: LeftView;
  onSelectView: (v: LeftView) => void;
  onOpen: () => void;
  onSettings: () => void;
}

function AppHeader({
  leftView,
  onSelectView,
  onOpen,
  onSettings,
}: AppHeaderProps) {
  return (
    <header
      data-testid="left-pane-tabs"
      className="
        relative z-10 flex shrink-0 items-center gap-4
        border-b border-[var(--border)]
        bg-[var(--surface-elev)]
        px-4 py-2.5
      "
    >
      <Wordmark />
      <div
        className="
          ml-3 flex items-center gap-1
          rounded-md border border-[var(--border)]
          bg-[var(--surface)]
          p-0.5
        "
      >
        <TabButton
          label="Timeline"
          testId="tab-timeline"
          active={leftView === "timeline"}
          onClick={() => onSelectView("timeline")}
        />
        <TabButton
          label="Graph"
          testId="tab-graph"
          active={leftView === "graph"}
          onClick={() => onSelectView("graph")}
        />
      </div>

      <div className="ml-auto flex items-center gap-2">
        <button
          type="button"
          data-testid="open-audio-button"
          onClick={onOpen}
          className="
            inline-flex items-center gap-2
            rounded-md border border-[var(--border-strong)]
            bg-[var(--surface)]
            px-3 py-1.5
            font-mono text-[11px] uppercase tracking-wider text-[var(--text-dim)]
            transition
            hover:border-[var(--accent)]/50 hover:bg-[var(--accent-soft)] hover:text-[var(--accent)]
          "
        >
          <svg
            width="12"
            height="12"
            viewBox="0 0 14 14"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.7"
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
          >
            <path d="M2 3.2C2 2.54 2.54 2 3.2 2h2.6L7 3.5h3.8c.66 0 1.2.54 1.2 1.2v6.1c0 .66-.54 1.2-1.2 1.2H3.2C2.54 12 2 11.46 2 10.8V3.2Z" />
          </svg>
          Open Audio
        </button>

        <button
          type="button"
          onClick={onSettings}
          data-testid="open-settings-button"
          aria-label="Open settings"
          className="
            inline-flex h-8 w-8 items-center justify-center
            rounded-md border border-[var(--border-strong)]
            bg-[var(--surface)]
            text-[var(--text-dim)]
            transition
            hover:border-[var(--accent)]/50 hover:bg-[var(--surface-elev-2)] hover:text-[var(--text)]
          "
        >
          <svg
            width="14"
            height="14"
            viewBox="0 0 14 14"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
          >
            <circle cx="7" cy="7" r="2.2" />
            <path d="M7 1.5v2M7 10.5v2M1.5 7h2M10.5 7h2M3 3l1.4 1.4M9.6 9.6L11 11M3 11l1.4-1.4M9.6 4.4L11 3" />
          </svg>
        </button>
      </div>
    </header>
  );
}

function Wordmark() {
  return (
    <div
      className="
        flex items-baseline gap-1
        text-[15px] font-medium leading-none
        text-[var(--text)]
      "
    >
      <span className="font-[var(--font-serif)] italic text-[var(--accent)] text-[18px]">
        edyt
      </span>
      <span>lab</span>
      <span
        className="
          ml-2 rounded
          bg-[var(--surface-elev-2)]
          px-1.5 py-0.5
          font-mono text-[9px] uppercase tracking-[0.18em] text-[var(--text-faint)]
        "
      >
        studio
      </span>
    </div>
  );
}

interface TabButtonProps {
  label: string;
  testId: string;
  active: boolean;
  onClick: () => void;
}

function TabButton({ label, testId, active, onClick }: TabButtonProps) {
  return (
    <button
      type="button"
      data-testid={testId}
      data-active={active ? "true" : "false"}
      onClick={onClick}
      aria-pressed={active}
      className={
        "rounded px-3 py-1 text-xs font-medium transition " +
        (active
          ? "bg-[var(--surface-elev-2)] text-[var(--text)] shadow-[inset_0_0_0_1px_rgba(255,255,255,0.04)]"
          : "text-[var(--text-faint)] hover:bg-[var(--surface-elev-2)]/60 hover:text-[var(--text-dim)]")
      }
    >
      {label}
    </button>
  );
}

interface StatusBarProps {
  audioPath: string | null;
  head: string | null;
  rendering: boolean;
  selection: Selection | null;
}

function StatusBar({ audioPath, head, rendering, selection }: StatusBarProps) {
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
