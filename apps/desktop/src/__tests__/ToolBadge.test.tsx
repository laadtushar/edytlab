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

  it("settles its glyph when the tool lands, and spins it only while running", () => {
    // The glyph is keyed by status, so running -> ok is a new element
    // (which mounts and plays `badge-settle`), not the old one restyled.
    // The running glyph keeps `animate-spin` and must not carry the
    // settle: both set `animation`, and the settle would stop the spin.
    const { rerender } = render(<ToolBadge name="normalize" status="running" />);
    const running = screen.getByTestId("tool-badge").querySelector("[aria-hidden]")!;
    expect(running.className).toContain("animate-spin");
    expect(running.className).not.toContain("badge-settle");

    rerender(<ToolBadge name="normalize" status="ok" />);
    const done = screen.getByTestId("tool-badge").querySelector("[aria-hidden]")!;
    expect(done).not.toBe(running);
    expect(done.className).toContain("badge-settle");
    expect(done.className).not.toContain("animate-spin");

    rerender(<ToolBadge name="normalize" status="error" />);
    const failed = screen.getByTestId("tool-badge").querySelector("[aria-hidden]")!;
    expect(failed).not.toBe(done);
    expect(failed.className).toContain("badge-settle");
  });

  it("is the same badge across a status change, so its tint can ease", () => {
    // Only the classes change, so `badge-tone`'s transition has
    // something to transition from.
    const { rerender } = render(<ToolBadge name="normalize" status="running" />);
    const badge = screen.getByTestId("tool-badge");
    expect(badge.className).toContain("badge-tone");

    rerender(<ToolBadge name="normalize" status="ok" />);
    expect(screen.getByTestId("tool-badge")).toBe(badge);
    expect(badge.className).toContain("--success");
    expect(badge.className).toContain("badge-tone");
  });

  it("fades in when it mounts", () => {
    render(<ToolBadge name="normalize" status="running" />);
    expect(screen.getByTestId("tool-badge").className).toContain("app-fade-in");
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
