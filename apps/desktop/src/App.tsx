/**
 * App — top-level layout.
 *
 * Two-pane shell: a "left" pane (Canvas or GraphView, switched by a
 * Timeline/Graph tab toggle) takes 70% and Chat takes 30%. Cross-pane
 * state is minimal: the parent owns the currently-displayed audio
 * path so Render Preview (in Chat) can hand a freshly rendered WAV
 * to Canvas, and it owns the head-pointer so the M25 GraphView can
 * select-by-click and re-render the canvas.
 *
 * App also owns the M13 first-launch flow: on mount we check whether
 * the OS keychain has an Anthropic API key. If not, we render a
 * blocking <Settings mode="blocking"> over everything until the user
 * provides one. A small gear button in the corner opens the same
 * component in panel mode for later edits / "Clear key".
 */

import { useCallback, useEffect, useState } from "react";

import { Canvas } from "./components/Canvas";
import { Chat } from "./components/Chat";
import { GraphView } from "./components/GraphView";
import { Settings } from "./components/Settings";
import { useSession } from "./hooks/useSession";
import {
  hasApiKey,
  onNodeCreated,
  renderPreview as bridgeRenderPreview,
} from "./lib/tauri-bridge";

type LeftView = "timeline" | "graph";

function App() {
  const { renderHead, head, setHeadLocal } = useSession();
  const [audioPath, setAudioPath] = useState<string | null>(null);
  const [rendering, setRendering] = useState(false);
  const [renderError, setRenderError] = useState<string | null>(null);
  const [leftView, setLeftView] = useState<LeftView>("timeline");
  const [graphRefresh, setGraphRefresh] = useState(0);
  // `null` means "we haven't checked yet"; we hold off on rendering
  // chat-bound bridge calls until we know whether a key exists.
  const [keyConfigured, setKeyConfigured] = useState<boolean | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);

  useEffect(() => {
    let cancelled = false;
    hasApiKey()
      .then((ok) => {
        if (!cancelled) setKeyConfigured(ok);
      })
      .catch(() => {
        // If the keychain probe fails (e.g. running outside Tauri in
        // tests), assume no key — the blocking modal is the safe
        // default.
        if (!cancelled) setKeyConfigured(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // Bump the graph-view refresh key whenever the agent emits
  // `node-created`, so a graph open in the right pane stays in sync
  // with the agent's edits without the user having to switch tabs.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    onNodeCreated(() => {
      setGraphRefresh((n) => n + 1);
    }).then((fn) => {
      if (cancelled) {
        fn();
      } else {
        unlisten = fn;
      }
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

  // GraphView click → update the local head pointer + render that
  // node into the canvas pane. Acceptance criterion #2: clicking a
  // node fires the head-update flow and the Canvas re-renders.
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

  const showBlocking = keyConfigured === false;

  return (
    <main className="grid h-screen w-screen grid-cols-[70%_30%]">
      <div className="flex h-full w-full flex-col">
        <div
          data-testid="left-pane-tabs"
          className="flex shrink-0 items-center gap-1 border-b border-neutral-800 bg-neutral-950 px-2 py-1"
        >
          <TabButton
            label="Timeline"
            testId="tab-timeline"
            active={leftView === "timeline"}
            onClick={() => setLeftView("timeline")}
          />
          <TabButton
            label="Graph"
            testId="tab-graph"
            active={leftView === "graph"}
            onClick={() => setLeftView("graph")}
          />
        </div>
        <div className="flex-1 min-h-0">
          {leftView === "timeline" ? (
            <Canvas audioPath={audioPath} onFileDropped={() => undefined} />
          ) : (
            <GraphView
              head={head}
              onSelectNode={handleSelectGraphNode}
              refreshKey={graphRefresh}
            />
          )}
        </div>
      </div>
      <Chat
        rendering={rendering}
        onRequestRenderPreview={handleRenderPreview}
      />
      <button
        type="button"
        onClick={() => setSettingsOpen(true)}
        data-testid="open-settings-button"
        aria-label="Open settings"
        className="fixed right-3 top-3 z-30 rounded-md border border-zinc-700 bg-zinc-900/80 px-2 py-1 text-xs text-zinc-200 hover:bg-zinc-800"
      >
        Settings
      </button>
      {renderError ? (
        <div
          role="alert"
          data-testid="render-error"
          className="fixed bottom-3 left-3 z-50 rounded-md border border-red-800 bg-red-900/80 px-3 py-2 text-xs text-red-100"
        >
          Could not render: {renderError}
        </div>
      ) : null}
      {showBlocking ? (
        <Settings
          mode="blocking"
          onSaved={() => setKeyConfigured(true)}
        />
      ) : null}
      {!showBlocking && settingsOpen ? (
        <Settings
          mode="panel"
          onClose={() => setSettingsOpen(false)}
          onSaved={() => setSettingsOpen(false)}
          onCleared={() => {
            // Acceptance criterion #3: clearing returns to first-launch
            // state without restart. Flip the flag so the blocking modal
            // takes over; close the panel.
            setKeyConfigured(false);
            setSettingsOpen(false);
          }}
        />
      ) : null}
    </main>
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
        "rounded-md px-3 py-1 text-xs font-medium " +
        (active
          ? "bg-neutral-800 text-neutral-100"
          : "text-neutral-400 hover:bg-neutral-800 hover:text-neutral-100")
      }
    >
      {label}
    </button>
  );
}

export default App;
