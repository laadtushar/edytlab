/**
 * The transcript pane (#157).
 *
 * The ticket calls this the single largest differentiator, and the
 * reason is one sentence: delete a sentence in the text and the audio
 * is cut to match. So the tests are that sentence — plus the selection
 * bridge that makes text and waveform two views of one range, not two
 * notions of "selected".
 */

import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { TranscriptPane } from "../TranscriptPane";
import type { TranscriptWord } from "../../lib/tauri-bridge";

const WORDS: TranscriptWord[] = [
  { text: "the", start_sec: 0.0, end_sec: 0.4, confidence: 0.9 },
  { text: "quick", start_sec: 0.5, end_sec: 0.9, confidence: 0.9 },
  { text: "brown", start_sec: 1.0, end_sec: 1.5, confidence: 0.9 },
  { text: "fox", start_sec: 1.6, end_sec: 2.0, confidence: 0.3 },
];

function setup(over: Partial<React.ComponentProps<typeof TranscriptPane>> = {}) {
  const h = {
    onSelectRange: vi.fn(),
    onCutWords: vi.fn(),
    onSeek: vi.fn(),
  };
  render(<TranscriptPane words={WORDS} {...h} {...over} />);
  return h;
}

/** Drag across words `a`..`b` inclusive. */
function dragWords(a: number, b: number) {
  const els = screen.getAllByTestId("transcript-word");
  fireEvent.mouseDown(els[a], { button: 0 });
  for (let i = a; i <= b; i++) fireEvent.mouseEnter(els[i]);
  fireEvent.mouseUp(window);
}

describe("the transcript pane", () => {
  it("shows every transcribed word", () => {
    setup();
    expect(screen.getAllByTestId("transcript-word")).toHaveLength(4);
    expect(screen.getByText(/quick/)).toBeTruthy();
  });

  /** An ordinary state, not a fault — so it says what to do. */
  it("says how to get a transcript when there is none", () => {
    setup({ words: [] });
    expect(screen.getByTestId("transcript-empty").textContent).toMatch(/transcribe/i);
    expect(screen.queryAllByTestId("transcript-word")).toHaveLength(0);
  });

  it("says so while transcribing rather than looking empty", () => {
    setup({ words: [], busy: true });
    expect(screen.getByTestId("transcript-busy")).toBeTruthy();
  });

  /**
   * The bridge: a text selection is a time range. From the first word's
   * *start* to the last word's *end* — cutting from the first word's end
   * would leave a clipped syllable, which is what `cut_words` reasons
   * about too.
   */
  it("reports a selected span as seconds", () => {
    const h = setup();
    dragWords(1, 2);
    expect(h.onSelectRange).toHaveBeenCalled();
    const calls = h.onSelectRange.mock.calls;
    const last = calls[calls.length - 1][0];
    expect(last).toEqual({ start: 0.5, end: 1.5 });
  });

  it("handles a backwards drag without inverting the range", () => {
    const h = setup();
    const els = screen.getAllByTestId("transcript-word");
    fireEvent.mouseDown(els[2], { button: 0 });
    fireEvent.mouseEnter(els[1]);
    fireEvent.mouseUp(window);
    const calls = h.onSelectRange.mock.calls;
    const last = calls[calls.length - 1][0];
    expect(last.start).toBeLessThan(last.end);
    expect(last).toEqual({ start: 0.5, end: 1.5 });
  });

  /** The other direction: a waveform drag lights up the words. */
  it("highlights the words a timeline selection covers", () => {
    setup({ selection: { start: 0.6, end: 1.4 } });
    const els = screen.getAllByTestId("transcript-word");
    expect(els[0].dataset.selected).toBe("false");
    expect(els[1].dataset.selected).toBe("true");
    expect(els[2].dataset.selected).toBe("true");
    expect(els[3].dataset.selected).toBe("false");
  });

  /** The sentence the whole ticket is about. */
  it("cuts the selected words on Delete", () => {
    const h = setup();
    dragWords(1, 2);
    fireEvent.keyDown(screen.getByTestId("transcript-pane"), { key: "Delete" });
    // Half-open, matching `cut_words`: words 1 and 2 means [1, 3).
    expect(h.onCutWords).toHaveBeenCalledWith(1, 3);
  });

  it("cuts on Backspace too", () => {
    const h = setup();
    dragWords(0, 0);
    fireEvent.keyDown(screen.getByTestId("transcript-pane"), { key: "Backspace" });
    expect(h.onCutWords).toHaveBeenCalledWith(0, 1);
  });

  it("does not cut when nothing is selected", () => {
    const h = setup();
    fireEvent.keyDown(screen.getByTestId("transcript-pane"), { key: "Delete" });
    expect(h.onCutWords).not.toHaveBeenCalled();
  });

  /** Escape has to clear both the pane and the waveform. */
  it("clears the selection on Escape", () => {
    const h = setup();
    dragWords(1, 2);
    fireEvent.keyDown(screen.getByTestId("transcript-pane"), { key: "Escape" });
    expect(h.onSelectRange).toHaveBeenLastCalledWith(null);
    expect(screen.getAllByTestId("transcript-word")[1].dataset.selected).toBe("false");
  });

  it("seeks to a word on double-click", () => {
    const h = setup();
    fireEvent.doubleClick(screen.getAllByTestId("transcript-word")[2]);
    expect(h.onSeek).toHaveBeenCalledWith(1.0);
  });

  /** A word the model was unsure of is worth seeing before cutting around it. */
  it("dims a low-confidence word", () => {
    setup();
    const els = screen.getAllByTestId("transcript-word");
    expect(Number(els[3].style.opacity)).toBeLessThan(1);
    expect(Number(els[0].style.opacity || 1)).toBe(1);
  });
});
