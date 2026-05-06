/**
 * App — top-level layout.
 *
 * Two-pane shell: Canvas occupies 70% on the left, Chat 30% on the
 * right. Cross-pane state is minimal: the parent owns the
 * currently-displayed audio path so Render Preview (in Chat) can hand
 * a freshly rendered WAV to Canvas.
 */

import { useCallback, useState } from "react";

import { Canvas } from "./components/Canvas";
import { Chat } from "./components/Chat";
import { useSession } from "./hooks/useSession";

function App() {
  const { renderHead, head } = useSession();
  const [audioPath, setAudioPath] = useState<string | null>(null);
  const [rendering, setRendering] = useState(false);
  const [renderError, setRenderError] = useState<string | null>(null);

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

  return (
    <main className="grid h-screen w-screen grid-cols-[70%_30%]">
      <Canvas audioPath={audioPath} onFileDropped={() => undefined} />
      <Chat
        rendering={rendering}
        onRequestRenderPreview={handleRenderPreview}
      />
      {renderError ? (
        <div
          role="alert"
          data-testid="render-error"
          className="fixed bottom-3 left-3 z-50 rounded-md border border-red-800 bg-red-900/80 px-3 py-2 text-xs text-red-100"
        >
          Could not render: {renderError}
        </div>
      ) : null}
    </main>
  );
}

export default App;
