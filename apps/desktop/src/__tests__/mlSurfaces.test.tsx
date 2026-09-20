/**
 * UI surfaces that still offered the unimplemented ML features (#233).
 *
 * #317 corrected the errors, the tool schemas, the chat card and the
 * website. Driving the built app turned up three more places the app
 * went on telling the user to transcribe: the command palette, the
 * transcript tab's empty state, and the first-run suggestion list.
 *
 * Its Rust guard could not have caught any of them — it checks
 * dangling scripts, dead env vars, error text and tool schemas, none
 * of which cover a UI affordance that offers a stub as available.
 */

import { render, screen, fireEvent } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { COMMANDS, CommandPalette } from "../components/CommandPalette";
import { ASSISTANT_EXAMPLES } from "../components/EmptyState";
import { TranscriptPane } from "../components/TranscriptPane";

describe("the first-run suggestion list", () => {
  it("does not lead with a feature that cannot run", () => {
    // "transcribe the audio" was the fourth example, and the only one
    // that errors. The first thing the app suggests should not be the
    // thing that fails.
    expect(ASSISTANT_EXAMPLES).not.toContain("transcribe the audio");
    expect(ASSISTANT_EXAMPLES.some((e) => /transcrib/i.test(e))).toBe(false);
    expect(ASSISTANT_EXAMPLES.length).toBeGreaterThan(2);
  });
});

describe("the command palette and unimplemented features (#233)", () => {
  /**
   * The palette offered "Transcribe speech — Speech to text via local
   * Whisper model" with an Enter affordance, while the decoder is a
   * stub. #317 corrected the errors, schemas and chat card; the
   * palette and the transcript tab were two more surfaces still
   * promising the dead end, and its guard could not see either.
   */
  const ML = ["Transcribe speech", "Separate stems"];

  for (const label of ML) {
    it(`marks "${label}" unavailable rather than offering it`, () => {
      const cmd = COMMANDS.find((c) => c.label === label);
      expect(cmd, `${label} is missing from the palette`).toBeTruthy();
      expect(
        cmd!.unavailable,
        `${label} is offered as though it works`,
      ).toBeTruthy();
      expect(cmd!.description.toLowerCase()).toContain("not implemented");
    });
  }

  it("keeps them searchable — a missing entry answers a different question", () => {
    // Removing them would leave someone typing "transcribe" with no
    // result and no explanation. "It's here, it doesn't work yet" is
    // the useful answer.
    for (const label of ML) {
      expect(COMMANDS.some((c) => c.label === label)).toBe(true);
    }
  });

  it("does not fill the chat when an unavailable command is clicked", () => {
    const onSelect = vi.fn();
    render(
      <CommandPalette open onClose={() => undefined} onSelect={onSelect} />,
    );
    fireEvent.change(screen.getByRole("textbox"), {
      target: { value: "transcribe" },
    });
    fireEvent.click(screen.getByText("Transcribe speech"));
    expect(
      onSelect,
      "clicking an unavailable command queued a prompt that only errors",
    ).not.toHaveBeenCalled();
  });

  it("still fills the chat for an available command", () => {
    const onSelect = vi.fn();
    render(
      <CommandPalette open onClose={() => undefined} onSelect={onSelect} />,
    );
    fireEvent.change(screen.getByRole("textbox"), {
      target: { value: "normalize" },
    });
    fireEvent.click(screen.getByText("Normalize"));
    expect(onSelect).toHaveBeenCalled();
  });
});

describe("the transcript tab's empty state", () => {
  it("says the feature is unavailable instead of naming a step", () => {
    render(<TranscriptPane words={[]} />);
    const el = screen.getByTestId("transcript-empty");
    expect(el.textContent).toMatch(/isn['’]t available in this build/i);
    // It used to say "Ask the agent to transcribe a track", which is
    // the dead end itself.
    expect(el.textContent).not.toMatch(/ask the agent to/i);
  });
});

