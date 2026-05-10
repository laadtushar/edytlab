/**
 * ToolBadge — compact pill rendering a tool-call's lifecycle (Studio Onyx).
 *
 * Visual states use semantic accent tokens (warning/success/danger)
 * mapped to the design system, so the dark theme stays cohesive
 * across the whole app rather than each component picking its own
 * red/green/yellow.
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
  running:
    "border-[var(--warning)]/35 bg-[var(--warning)]/10 text-[var(--warning)]",
  ok: "border-[var(--success)]/35 bg-[var(--success)]/10 text-[var(--success)]",
  error:
    "border-[var(--danger)]/40 bg-[var(--danger)]/10 text-[var(--danger)]",
};

export function ToolBadge({ name, status, result }: ToolBadgeProps) {
  const label =
    status === "running" ? `running ${name}…` : (result ?? name);
  return (
    <span
      data-testid="tool-badge"
      data-status={status}
      className={
        "inline-flex items-center gap-1.5 rounded-full border px-2.5 py-0.5 font-mono text-[10px] uppercase tracking-wider " +
        STATUS_CLASS[status]
      }
    >
      <span
        aria-hidden="true"
        className={status === "running" ? "animate-spin" : ""}
      >
        {STATUS_GLYPH[status]}
      </span>
      <span className="normal-case tracking-normal">{label}</span>
    </span>
  );
}
