/**
 * ToolBadge — status transitions render the right glyph + label.
 */

import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";

import { ToolBadge } from "../components/ToolBadge";

describe("ToolBadge", () => {
  it("renders running state with name and ellipsis", () => {
    render(<ToolBadge name="normalize" status="running" />);
    const badge = screen.getByTestId("tool-badge");
    expect(badge).toHaveAttribute("data-status", "running");
    expect(badge.textContent).toContain("running normalize");
  });

  it("renders ok state with check glyph", () => {
    render(<ToolBadge name="normalize" status="ok" />);
    const badge = screen.getByTestId("tool-badge");
    expect(badge).toHaveAttribute("data-status", "ok");
    expect(badge.textContent).toContain("✓");
    expect(badge.textContent).toContain("normalize");
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
});
