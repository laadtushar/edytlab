/**
 * The palette's search has to be tested through the palette (#262).
 *
 * `CommandPalette.test.tsx` checks the *contents* of `COMMANDS` — that
 * every entry has a prompt, that categories stay contiguous. What it
 * cannot check is the thing the user actually does: type, and see the
 * list narrow. That filter lived only inside the component, and the one
 * test that claimed to cover it defined its own `search()` helper and
 * asserted against that — including a clause matching `c.prompt`, which
 * the real filter has never read.
 *
 * The measure of this file: replacing the component's `filtered` with
 * `return []` — a palette that matches nothing for every query — must
 * fail it. Nothing before did.
 *
 * The tags clause is the reason this matters in practice. "sidechain"
 * and "diarize" appear in no label, description or category; they are
 * reachable only through `tags`. Dropping that one clause would make
 * those commands unfindable while every other test stayed green.
 */

import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { COMMANDS, CommandPalette } from "../components/CommandPalette";

function open(onSelect = vi.fn()) {
  const onClose = vi.fn();
  const view = render(
    <CommandPalette open onClose={onClose} onSelect={onSelect} />,
  );
  const input = screen.getByLabelText("Search commands");
  const type = (q: string) => fireEvent.change(input, { target: { value: q } });
  return { view, input, type, onSelect, onClose };
}

/**
 * The command labels currently rendered, in rendered order.
 *
 * A row is `<button><span><span>{label}</span><span>{description}</span>`,
 * so the label is the first nested span — the outer one's text is the
 * label and description run together.
 */
function visibleLabels(): string[] {
  return screen
    .queryAllByRole("button")
    .map((b) => b.querySelector("span span")?.textContent ?? "")
    .filter((t) => t.length > 0);
}

describe("typing in the palette", () => {
  it("shows everything before a query is typed", () => {
    open();
    expect(visibleLabels().length).toBe(COMMANDS.length);
  });

  it("narrows to the commands that match", () => {
    const { type } = open();
    type("normalize");

    const labels = visibleLabels();
    expect(labels.length).toBeGreaterThan(0);
    expect(labels.length).toBeLessThan(COMMANDS.length);
    for (const label of labels) {
      const cmd = COMMANDS.find((c) => c.label === label)!;
      expect(
        `${cmd.label} ${cmd.description} ${cmd.category} ${(cmd.tags ?? []).join(" ")}`.toLowerCase(),
        `"${label}" matched "normalize" on nothing the filter reads`,
      ).toContain("normalize");
    }
  });

  /**
   * The clause most likely to be dropped in a refactor, and the only
   * route to these commands.
   */
  it.each([
    ["sidechain", "Duck music under speech"],
    ["denoise", "Reduce noise"],
  ])("finds %s, which only a tag matches", (query, expected) => {
    const cmd = COMMANDS.find((c) => c.label === expected)!;
    expect(
      `${cmd.label} ${cmd.description} ${cmd.category}`.toLowerCase(),
      `"${query}" is no longer tag-only for "${expected}"; pick another query`,
    ).not.toContain(query);

    const { type } = open();
    type(query);
    expect(visibleLabels()).toContain(expected);
  });

  it("is case-insensitive and ignores surrounding space", () => {
    const { type } = open();
    type("  NORMALIZE  ");
    expect(visibleLabels().length).toBeGreaterThan(0);
  });

  it("says so when nothing matches", () => {
    const { type } = open();
    type("zzzzz-no-such-command");
    expect(visibleLabels()).toEqual([]);
    expect(screen.getByText(/No commands match/)).toBeInTheDocument();
  });
});

describe("choosing a command", () => {
  it("hands the chat the prompt of the row that was clicked", () => {
    const { type, onSelect } = open();
    type("sidechain");

    const label = visibleLabels()[0];
    fireEvent.click(screen.getByText(label));

    expect(onSelect).toHaveBeenCalledWith(
      COMMANDS.find((c) => c.label === label)!.prompt,
    );
  });

  /**
   * Arrow keys index the flat filtered list while the eye reads the
   * grouped one. If those two orders drift, Enter sends a prompt other
   * than the highlighted row's — the failure is silent, because the
   * palette closes and a plausible prompt lands in the chat.
   */
  it("Enter sends the highlighted row, not a different one", () => {
    const { type, onSelect } = open();
    type("fade");

    const labels = visibleLabels();
    expect(labels.length).toBeGreaterThan(1);

    fireEvent.keyDown(document, { key: "ArrowDown" });
    fireEvent.keyDown(document, { key: "Enter" });

    expect(onSelect).toHaveBeenCalledWith(
      COMMANDS.find((c) => c.label === labels[1])!.prompt,
    );
  });
});
