/**
 * ToolBadge — status transitions render the right glyph + label.
 */

import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";

import { ToolBadge } from "../components/ToolBadge";

describe("ToolBadge", () => {
  it("renders running state with friendly name and ellipsis", () => {
    render(<ToolBadge name="normalize" status="running" />);
    const badge = screen.getByTestId("tool-badge");
    expect(badge).toHaveAttribute("data-status", "running");
    // ToolBadge maps "normalize" → "Normalize" and appends "…"
    expect(badge.textContent).toContain("Normalize");
  });

  it("renders ok state with check glyph and friendly name", () => {
    render(<ToolBadge name="normalize" status="ok" />);
    const badge = screen.getByTestId("tool-badge");
    expect(badge).toHaveAttribute("data-status", "ok");
    expect(badge.textContent).toContain("✓");
    expect(badge.textContent).toContain("Normalize");
  });

  it("prefers explicit result text over the tool name on completion", () => {
    render(
      <ToolBadge name="normalize" status="ok" result="normalized -1 dBFS" />,
    );
    expect(screen.getByTestId("tool-badge").textContent).toContain(
      "normalized -1 dBFS",
    );
  });

  it("renders error state with cross glyph", () => {
    render(<ToolBadge name="normalize" status="error" />);
    const badge = screen.getByTestId("tool-badge");
    expect(badge).toHaveAttribute("data-status", "error");
    expect(badge.textContent).toContain("✗");
  });

  it("renders not_run as neither success nor failure", () => {
    render(<ToolBadge name="normalize" status="not_run" />);
    const badge = screen.getByTestId("tool-badge");
    expect(badge).toHaveAttribute("data-status", "not_run");
    expect(badge.textContent).toContain("Normalize");
    expect(badge.textContent).toContain("not run");
    expect(badge.textContent).not.toContain("✗");
    expect(badge.textContent).not.toContain("✓");
    // Nothing ran, so it takes no danger or success colour.
    expect(badge.className).not.toContain("--danger");
    expect(badge.className).not.toContain("--success");
  });
});
