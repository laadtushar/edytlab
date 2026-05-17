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
  if (!open) return null;
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      onClick={onClose}
    >
      <div
        className="bg-[var(--surface-elev)] border border-[var(--border-strong)] rounded-xl shadow-2xl w-80 p-6"
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
                className="w-full text-left px-3 py-2 rounded-lg border border-[var(--border-strong)] hover:border-[var(--accent)]/60 hover:bg-[var(--accent-soft)] transition-colors"
                onClick={() => onSelect(t.name)}
              >
                <div className="text-sm text-[var(--text)] font-medium">{t.name}</div>
                <div className="text-xs text-[var(--text-faint)]">{t.description}</div>
              </button>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
