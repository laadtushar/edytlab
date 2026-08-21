import { useEffect } from "react";

export interface Shortcut {
  keys: string;
  description: string;
}

export const SHORTCUTS: Shortcut[] = [
  { keys: "Space", description: "Play / Pause" },
  { keys: "Home", description: "Seek to start" },
  { keys: "End", description: "Seek to end" },
  { keys: "← →", description: "Seek ±5 seconds" },
  { keys: "Shift+← →", description: "Seek ±1 second" },
  { keys: "Escape", description: "Clear selection" },
  { keys: "Ctrl+K", description: "Command palette (all tools)" },
  { keys: "Ctrl+Z", description: "Undo" },
  { keys: "Ctrl+Y / Ctrl+Shift+Z", description: "Redo" },
  { keys: "+ / =", description: "Zoom in" },
  { keys: "-", description: "Zoom out" },
  { keys: "0", description: "Reset zoom" },
  { keys: "Ctrl/Cmd + E", description: "Zoom to selection" },
  { keys: "Ctrl/Cmd + F", description: "Fit to window" },
  { keys: "↕+ / ↕−", description: "Vertical zoom (toolbar)" },
  { keys: "L", description: "Toggle loop playback" },
  { keys: "?", description: "Show this overlay" },
];

interface ShortcutsOverlayProps {
  open: boolean;
  onClose: () => void;
}

export function ShortcutsOverlay({ open, onClose }: ShortcutsOverlayProps) {
  useEffect(() => {
    if (!open) return;
    function handleKey(e: KeyboardEvent) {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    }
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div
      className="backdrop-in fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      onClick={onClose}
      data-testid="shortcuts-overlay"
      role="dialog"
      aria-modal="true"
      aria-labelledby="shortcuts-heading"
    >
      <div
        className="overlay-in bg-neutral-900 border border-neutral-700 rounded-xl shadow-2xl w-96 p-6"
        onClick={e => e.stopPropagation()}
      >
        <div className="flex items-center justify-between mb-4">
          <h2 id="shortcuts-heading" className="text-sm font-semibold text-neutral-200 tracking-wide uppercase">
            Keyboard Shortcuts
          </h2>
          <button
            type="button"
            onClick={onClose}
            className="text-neutral-500 hover:text-neutral-300 text-lg leading-none"
            aria-label="Close"
          >
            ×
          </button>
        </div>
        <table className="w-full text-sm">
          <tbody>
            {SHORTCUTS.map(({ keys, description }) => (
              <tr key={keys} className="border-b border-neutral-800 last:border-0">
                <td className="py-2 pr-4 font-mono text-amber-400 whitespace-nowrap">
                  {keys}
                </td>
                <td className="py-2 text-neutral-300">{description}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
