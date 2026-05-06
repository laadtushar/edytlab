/**
 * ToolBadge — compact inline pill rendering a tool-call's lifecycle.
 *
 * Visual states:
 *   running → "running <name>…" with a spinner glyph
 *   ok      → "✓ <name>" (or `result` text if provided)
 *   error   → "✗ <name>" (or `result` text if provided)
 *
 * The component is presentational only. State transitions are owned by
 * `useAgentStream`.
 */

import type { ToolStatus } from "../hooks/useAgentStream";

export interface ToolBadgeProps {
  name: string;
  status: ToolStatus;
  result?: string;
}

const STATUS_GLYPH: Record<ToolStatus, string> = {
  running: "⟳",
  ok: "✓",
  error: "✗",
};

const STATUS_CLASS: Record<ToolStatus, string> = {
  running: "bg-yellow-900/40 text-yellow-200 border-yellow-800",
  ok: "bg-green-900/40 text-green-200 border-green-800",
  error: "bg-red-900/40 text-red-200 border-red-800",
};

export function ToolBadge({ name, status, result }: ToolBadgeProps) {
  const label =
    status === "running"
      ? `running ${name}…`
      : (result ?? name);
  return (
    <span
      data-testid="tool-badge"
      data-status={status}
      className={
        "inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-xs font-mono " +
        STATUS_CLASS[status]
      }
    >
      <span
        aria-hidden="true"
        className={status === "running" ? "animate-spin" : ""}
      >
        {STATUS_GLYPH[status]}
      </span>
      <span>{label}</span>
    </span>
  );
}
