/**
 * App — top-level layout.
 *
 * Two-pane shell: Canvas occupies 70% on the left, Chat 30% on the
 * right. Cross-pane state is minimal: the parent owns the
 * currently-displayed audio path so Render Preview (in Chat) can hand
 * a freshly rendered WAV to Canvas.
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
import { Settings } from "./components/Settings";
import { useSession } from "./hooks/useSession";
import { hasApiKey } from "./lib/tauri-bridge";

function App() {
  const { renderHead, head } = useSession();
  const [audioPath, setAudioPath] = useState<string | null>(null);
  const [rendering, setRendering] = useState(false);
  const [renderError, setRenderError] = useState<string | null>(null);
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

  const showBlocking = keyConfigured === false;

  return (
    <main className="grid h-screen w-screen grid-cols-[70%_30%]">
      <Canvas audioPath={audioPath} onFileDropped={() => undefined} />
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

export default App;
