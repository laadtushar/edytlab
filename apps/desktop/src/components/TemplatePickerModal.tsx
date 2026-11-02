import { useEffect, useRef } from "react";
import { usePresence } from "../hooks/usePresence";

export interface TemplateInfo {
  name: string;
  description: string;
}

interface TemplatePickerModalProps {
  open: boolean;
  templates: TemplateInfo[];
  onSelect: (name: string) => void;
  onClose: () => void;
}

export function TemplatePickerModal({
  open,
  templates,
  onSelect,
  onClose,
}: TemplatePickerModalProps) {
  // Held open for one --dur-2 on close so the exit animation can play
  // (#236); `mounted` is still false for a modal that was never open.
  const { mounted, leaving, onAnimationEnd } = usePresence(open);
  const cancelRef = useRef<HTMLButtonElement>(null);
  // Keyboard focus moves into the picker when it opens and goes back to
  // whatever had it when it closes. Opened from closed, the picker mounts a
  // render after `open` turns true (usePresence), so this waits for `mounted`:
  // Cancel does not exist before then.
  useEffect(() => {
    if (!open || !mounted) return;
    const previous = document.activeElement as HTMLElement | null;
    cancelRef.current?.focus();
    return () => previous?.focus();
  }, [open, mounted]);
  // Escape closes it, as it does the shortcuts overlay. App's own Escape
  // (clear the selection) stands down while the picker is open, because
  // window listeners cannot stop one another.
  useEffect(() => {
    if (!open) return;
    function handleKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [open, onClose]);
  if (!mounted) return null;
  return (
    <div
      className={`${leaving ? "backdrop-out" : "backdrop-in"} fixed inset-0 z-50 flex items-center justify-center bg-black/60`}
      // A pointer way out of the picker; Escape and Cancel are the others.
      role="presentation"
      onClick={onClose}
      onAnimationEnd={onAnimationEnd}
    >
      <div
        className={`${leaving ? "overlay-out" : "overlay-in"} bg-[var(--surface-elev)] border border-[var(--border-strong)] rounded-xl shadow-(--shadow-dialog) w-80 p-6`}
        // Keeps a click inside the card from reaching the backdrop's close.
        role="presentation"
        onClick={e => e.stopPropagation()}
      >
        <h2 className="text-sm font-semibold text-[var(--text)] tracking-wide uppercase mb-4">
          Start from template
        </h2>
        <ul className="space-y-2">
          {templates.map(t => (
            <li key={t.name}>
              <button
                type="button"
                className="ring-hover w-full text-left px-3 py-2 rounded-lg border border-[var(--border-strong)] hover:border-[var(--accent-border)] hover:bg-[var(--accent-soft)] transition"
                onClick={() => onSelect(t.name)}
              >
                <div className="text-sm text-[var(--text)] font-medium">{t.name}</div>
                <div className="text-xs text-[var(--text-faint)]">{t.description}</div>
              </button>
            </li>
          ))}
        </ul>
        <div className="mt-4 flex justify-end">
          <button
            ref={cancelRef}
            type="button"
            className="ring-hover px-3 py-1.5 text-xs rounded-lg border border-[var(--border-strong)] text-[var(--text-muted)] hover:text-[var(--text)] transition"
            onClick={onClose}
          >
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}
