/**
 * ErrorBanner — inline, dismissable, optionally actionable error row.
 *
 * Replaces the previous fixed-position toast. Sits at the top of the
 * left pane so the user sees it in their primary work surface, not
 * tucked into a corner. When `action` is set we render a primary CTA
 * (e.g. "Open Settings") so a recoverable error has a one-click path
 * out — most common case is the "no agent configured" / "set_api_key"
 * error that previously left users with a toast and no next step.
 */

interface ErrorBannerProps {
  message: string;
  onDismiss?: () => void;
  action?: {
    label: string;
    onClick: () => void;
  };
  testId?: string;
}

export function ErrorBanner({
  message,
  onDismiss,
  action,
  testId,
}: ErrorBannerProps) {
  return (
    <div
      role="alert"
      data-testid={testId}
      className="
        app-fade-in
        flex items-center gap-3
        border-b border-[var(--border-strong)]
        bg-[linear-gradient(180deg,rgba(239,111,114,0.10),rgba(239,111,114,0.04))]
        px-4 py-2.5
        text-sm
      "
    >
      <span
        aria-hidden="true"
        className="
          flex h-6 w-6 shrink-0 items-center justify-center
          rounded-full border border-[var(--danger)]/40 bg-[var(--danger)]/15
          font-mono text-[var(--danger)] text-[11px] font-semibold
        "
      >
        !
      </span>
      <span className="flex-1 text-[var(--text)]">{message}</span>
      {action ? (
        <button
          type="button"
          onClick={action.onClick}
          className="
            shrink-0 rounded-md
            border border-[var(--accent)]/45
            bg-[var(--accent-soft)]
            px-3 py-1
            font-mono text-[11px] uppercase tracking-wider
            text-[var(--accent)]
            transition
            hover:bg-[var(--accent)]/22 hover:border-[var(--accent)]/70
          "
        >
          {action.label}
        </button>
      ) : null}
      {onDismiss ? (
        <button
          type="button"
          aria-label="Dismiss error"
          onClick={onDismiss}
          className="
            shrink-0 rounded-md p-1
            text-[var(--text-dim)]
            transition
            hover:bg-white/5 hover:text-[var(--text)]
          "
        >
          <svg
            width="14"
            height="14"
            viewBox="0 0 14 14"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.6"
            strokeLinecap="round"
          >
            <path d="M3 3l8 8M11 3l-8 8" />
          </svg>
        </button>
      ) : null}
    </div>
  );
}
