import type { ReactNode } from "react";

/**
 * ErrorBanner — inline, dismissable, optionally actionable error row.
 *
 * Replaces the previous fixed-position toast. Sits at the top of the
 * left pane so the user sees it in their primary work surface, not
 * tucked into a corner. When `action` is set we render a primary CTA
 * (e.g. "Open Settings") so a recoverable error has a one-click path
 * out — most common case is the "no agent configured" / "set_api_key"
 * error that previously left users with a toast and no next step.
 *
 * `tone="warning"` is the same row for something that did not fail but
 * the user should know (an effect the render leaves out, #532): amber
 * rather than red, and a status rather than an alert, so a screen reader
 * does not announce it as an error.
 */

/** Whole class strings, so Tailwind can see them. */
const TONES = {
  error: {
    role: "alert",
    row: "bg-[var(--danger-soft)]",
    badge: "border-[var(--danger-border)] bg-[var(--danger)]/15 text-[var(--danger)]",
  },
  warning: {
    role: "status",
    row: "bg-[var(--warning-soft)]",
    badge: "border-[var(--warning-border)] bg-[var(--warning)]/15 text-[var(--warning)]",
  },
} as const;

interface ErrorBannerProps {
  message: ReactNode;
  tone?: keyof typeof TONES;
  onDismiss?: () => void;
  action?: {
    label: string;
    onClick: () => void;
  };
  testId?: string;
  /** Accessible name of the dismiss button. Defaults to "Dismiss error". */
  dismissLabel?: string;
}

export function ErrorBanner({
  message,
  onDismiss,
  action,
  testId,
  dismissLabel = "Dismiss error",
  tone = "error",
}: ErrorBannerProps) {
  const t = TONES[tone];
  return (
    <div
      role={t.role}
      data-testid={testId}
      className={`
        banner-in
        flex items-center gap-3
        border-b border-[var(--border-strong)]
        ${t.row}
        px-4 py-2.5
        text-sm
      `}
    >
      <span
        aria-hidden="true"
        className={`
          flex h-6 w-6 shrink-0 items-center justify-center
          rounded-full border ${t.badge}
          font-mono text-[11px] font-semibold
        `}
      >
        !
      </span>
      <div className="min-w-0 flex-1 text-[var(--text)]">{message}</div>
      {action ? (
        <button
          type="button"
          onClick={action.onClick}
          className="
            btn-secondary shrink-0 rounded-md
            px-3 py-1
            font-mono text-[11px] uppercase tracking-wider
            text-[var(--accent-text)]
            transition
          "
        >
          {action.label}
        </button>
      ) : null}
      {onDismiss ? (
        <button
          type="button"
          aria-label={dismissLabel}
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
