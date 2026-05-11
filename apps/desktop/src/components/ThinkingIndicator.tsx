/**
 * ThinkingIndicator — pill shown while the agent has work pending but
 * hasn't streamed anything back yet.
 *
 * Two states:
 *
 * - `awaiting` (no text yet): "Thinking…" with three pulsing dots. This
 *   is what shows between the user's submit and the first agent event.
 * - `streaming` (text deltas arriving): falls through to render
 *   `null` — the streaming bubble in `Chat` already shows the live
 *   cursor, so a second indicator would be noise.
 *
 * The design follows the pattern documented in
 * `docs/specs/agentic-chat-ui.md`: a one-line progress pulse, no full
 * reasoning dump. We reserve room for an "expand reasoning" affordance
 * (`<details>`) once the backend emits thinking-delta events.
 */

export interface ThinkingIndicatorProps {
  /** True if the agent is awaiting the first response event. */
  awaiting: boolean;
}

export function ThinkingIndicator({ awaiting }: ThinkingIndicatorProps) {
  if (!awaiting) return null;
  return (
    <div
      data-testid="thinking-indicator"
      role="status"
      aria-live="polite"
      className="
        inline-flex items-center gap-2
        rounded-full
        border border-[var(--border)]
        bg-[var(--surface-elev)]
        px-3 py-1
        text-xs text-[var(--text-dim)]
      "
    >
      <span className="flex items-center gap-0.5" aria-hidden="true">
        <Dot delay="0ms" />
        <Dot delay="160ms" />
        <Dot delay="320ms" />
      </span>
      <span className="font-mono text-[10px] uppercase tracking-[0.18em]">
        thinking
      </span>
    </div>
  );
}

function Dot({ delay }: { delay: string }) {
  return (
    <span
      className="inline-block h-1 w-1 animate-pulse rounded-full bg-[var(--accent)]"
      style={{ animationDelay: delay }}
    />
  );
}
