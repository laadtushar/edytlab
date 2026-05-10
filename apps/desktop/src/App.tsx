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

import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { ABCompareBar } from "./components/ABCompareBar";
import { Chat } from "./components/Chat";
import { EmptyState } from "./components/EmptyState";
import { ErrorBanner } from "./components/ErrorBanner";
import { GraphView } from "./components/GraphView";
import { Settings } from "./components/Settings";
import { Timeline } from "./components/Timeline";
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
    onNodeCreated(() => {
      setGraphRefresh((n) => n + 1);
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
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
                  audioPath={audioPath}
                  onFileDropped={() => undefined}
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
          />
        </aside>
      </div>

      <StatusBar audioPath={audioPath} head={head} rendering={rendering} />

      {showBlocking ? (
        <Settings mode="blocking" onSaved={() => setKeyConfigured(true)} />
      ) : null}
      {!showBlocking && settingsOpen ? (
        <Settings
          mode="panel"
          onClose={() => setSettingsOpen(false)}
          onSaved={() => setSettingsOpen(false)}
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
}

function StatusBar({ audioPath, head, rendering }: StatusBarProps) {
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
      <span className="ml-auto text-[var(--text-faint)]">v0.1.0</span>
    </footer>
  );
}

function trimPath(path: string): string {
  const sep = path.includes("\\") ? "\\" : "/";
  const parts = path.split(sep);
  return parts[parts.length - 1] || path;
}

export default App;
