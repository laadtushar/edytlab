import { useDialog } from "../hooks/useDialog";
import { usePresence } from "../hooks/usePresence";

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
  { keys: "Ctrl/Cmd + Z", description: "Undo" },
  { keys: "Ctrl/Cmd + Shift + Z", description: "Redo" },
  { keys: "Ctrl/Cmd + Y", description: "Redo (alternative)" },
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
  // Held open for one --dur-2 on close so the exit animation can play
  // (#236); `mounted` is still false for an overlay never opened.
  const { mounted, leaving, onAnimationEnd } = usePresence(open);

  const { dialogProps, headingId } = useDialog(open && mounted, onClose);

  if (!mounted) return null;

  return (
    <div
      className={`${leaving ? "backdrop-out" : "backdrop-in"} fixed inset-0 z-50 flex items-center justify-center bg-black/60`}
      onClick={e => { if (e.target === e.currentTarget) onClose(); }}
      onAnimationEnd={onAnimationEnd}
      data-testid="shortcuts-overlay"
      inert={!open}
      aria-hidden={!open ? true : undefined}
      role="presentation"
    >
      <div
        {...dialogProps}
        className={`${leaving ? "overlay-out" : "overlay-in"} bg-[var(--surface-elev)] border border-[var(--border-strong)] rounded-xl shadow-(--shadow-dialog) w-96 p-6`}
      >
        <div className="flex items-center justify-between mb-4">
          <h2 id={headingId} className="text-sm font-semibold text-[var(--text)] tracking-wide uppercase">
            Keyboard Shortcuts
          </h2>
          <button
            type="button"
            onClick={onClose}
            className="text-[var(--text-dim)] hover:text-[var(--text)] text-lg leading-none"
            aria-label="Close"
          >
            ×
          </button>
        </div>
        <table className="w-full text-sm">
          <tbody>
            {SHORTCUTS.map(({ keys, description }) => (
              <tr key={keys} className="border-b border-[var(--border)] last:border-0">
                <td className="py-2 pr-4 font-mono text-[var(--accent-text)] whitespace-nowrap">
                  {keys}
                </td>
                <td className="py-2 text-[var(--text-dim)]">{description}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
