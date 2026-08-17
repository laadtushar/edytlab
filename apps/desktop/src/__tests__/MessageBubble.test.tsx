/**
 * The bubble renders what the model streamed, not the blank lines it
 * happened to start with.
 *
 * The bubble is `whitespace-pre-wrap` — correct in the middle of a
 * message, wrong at its edges. Models routinely open a reply with
 * newlines, more so once a thinking block has been stripped, and every
 * one of them was rendered: the bubble grew to paragraph height with
 * nothing in it and the streaming caret stranded at the bottom. That is
 * the "weird orange thing floating in whitespace" it looked like.
 *
 * These tests pin the two halves of the fix — the trim, and the caret
 * being a caret rather than a glyph in a box too narrow to hold it.
 */

import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { MessageBubble } from "../components/MessageBubble";

describe("MessageBubble", () => {
  it("drops leading newlines the model opened with", () => {
    render(<MessageBubble role="assistant" text={"\n\n\n\nDone! Speed is 2x."} />);
    const bubble = screen.getByTestId("message-bubble");
    expect(bubble.textContent).toBe("Done! Speed is 2x.");
  });

  it("drops trailing whitespace too, so the box does not overhang", () => {
    render(<MessageBubble role="assistant" text={"Done.\n\n\n"} />);
    expect(screen.getByTestId("message-bubble").textContent).toBe("Done.");
  });

  it("keeps blank lines *inside* a message, which are real formatting", () => {
    const text = "Would you like to:\n\n- Speed it up?\n- Slow it down?";
    render(<MessageBubble role="assistant" text={text} />);
    expect(screen.getByTestId("message-bubble").textContent).toBe(text);
  });

  /**
   * The bug as seen: nothing has streamed yet, so there is no text — and
   * the bubble should be the size of a caret rather than the size of a
   * paragraph.
   */
  it("renders no text at all when only whitespace has arrived", () => {
    render(<MessageBubble role="assistant" text={"\n\n\n"} pending />);
    const bubble = screen.getByTestId("message-bubble");
    expect(bubble.textContent).toBe("");
    expect(screen.getByTestId("caret")).toBeInTheDocument();
  });

  it("shows the caret only while streaming", () => {
    const { rerender } = render(
      <MessageBubble role="assistant" text="typing" pending />,
    );
    expect(screen.getByTestId("caret")).toBeInTheDocument();
    rerender(<MessageBubble role="assistant" text="typing" />);
    expect(screen.queryByTestId("caret")).not.toBeInTheDocument();
  });

  /**
   * It was a `▍` glyph inside a `w-1.5` inline-block — narrower than the
   * character, so it clipped, and it read as a stray orange mark rather
   * than a cursor. A drawn bar has no font dependency and cannot clip.
   */
  it("draws the caret rather than typing a block character", () => {
    render(<MessageBubble role="assistant" text="x" pending />);
    const caret = screen.getByTestId("caret");
    expect(caret.textContent).toBe("");
    expect(caret).toHaveAttribute("aria-hidden", "true");
  });

  it("still distinguishes user from assistant", () => {
    render(<MessageBubble role="user" text="increase speed 2x" />);
    expect(screen.getByTestId("message-bubble")).toHaveAttribute(
      "data-role",
      "user",
    );
  });
});
