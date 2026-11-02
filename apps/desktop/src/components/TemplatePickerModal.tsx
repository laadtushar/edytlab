import { useDialog } from "../hooks/useDialog";
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
  const { dialogProps, headingId } = useDialog(open && mounted, onClose);
  if (!mounted) return null;
  return (
    <div
      className={`${leaving ? "backdrop-out" : "backdrop-in"} fixed inset-0 z-50 flex items-center justify-center bg-black/60`}
      inert={!open}
      aria-hidden={!open ? true : undefined}
      role="presentation"
      onClick={e => { if (e.target === e.currentTarget) onClose(); }}
      onAnimationEnd={onAnimationEnd}
    >
      <div
        {...dialogProps}
        className={`${leaving ? "overlay-out" : "overlay-in"} bg-[var(--surface-elev)] border border-[var(--border-strong)] rounded-xl shadow-(--shadow-dialog) w-80 p-6`}
      >
        <h2 id={headingId} className="text-sm font-semibold text-[var(--text)] tracking-wide uppercase mb-4">
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
          onClick={onClose}
          className="btn-secondary mt-4 w-full rounded-lg px-3 py-2 text-sm"
        >
          Cancel
        </button>
      </div>
    </div>
  );
}
