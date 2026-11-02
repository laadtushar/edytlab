import { OverlayDialog } from "./OverlayDialog";
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
  if (!mounted) return null;
  return (
    <div
      data-overlay-layer
      className={`${leaving ? "backdrop-out" : "backdrop-in"} fixed inset-0 z-50 flex items-center justify-center bg-black/60`}
      inert={!open}
      aria-hidden={!open || undefined}
      onClick={onClose}
      onAnimationEnd={onAnimationEnd}
    >
      <OverlayDialog
        active={open}
        onClose={onClose}
        aria-labelledby="template-picker-heading"
        className={`${leaving ? "overlay-out" : "overlay-in"} bg-[var(--surface-elev)] border border-[var(--border-strong)] rounded-xl shadow-(--shadow-dialog) w-80 p-6`}
      >
        <h2 id="template-picker-heading" className="text-sm font-semibold text-[var(--text)] tracking-wide uppercase mb-4">
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
        <button
          type="button"
          data-dialog-initial-focus
          className="btn-secondary mt-4 w-full rounded px-3 py-2"
          onClick={onClose}
        >
          Cancel
        </button>
      </OverlayDialog>
    </div>
  );
}
