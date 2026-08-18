/**
 * The progress strip (#169 §1).
 *
 * The point of it is that a long batch stops being indistinguishable
 * from a hang, so the tests are about what a waiting user can see and
 * do — not about the component's internals.
 */

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const handlers: Array<(p: unknown) => void> = [];
const cancel = vi.fn(() => Promise.resolve());

vi.mock("../../lib/tauri-bridge", () => ({
  onToolProgress: (cb: (p: unknown) => void) => {
    handlers.push(cb);
    return Promise.resolve(() => undefined);
  },
  cancelLongRunningTool: () => cancel(),
}));

import { ToolProgressBar } from "../ToolProgressBar";

function emit(p: Record<string, unknown>) {
  handlers.forEach((h) => h(p));
}

const RUNNING = {
  kind: "batch_apply",
  index: 1,
  total: 3,
  file: "/takes/episode-two.wav",
  succeeded: 1,
  refused: 0,
};

describe("the tool progress strip", () => {
  beforeEach(() => {
    handlers.length = 0;
    cancel.mockClear();
  });

  /** Nothing running, nothing shown — it must not be permanent chrome. */
  it("is invisible until something is running", () => {
    render(<ToolProgressBar />);
    expect(screen.queryByTestId("tool-progress")).toBeNull();
  });

  it("says which file, and how far along", async () => {
    render(<ToolProgressBar />);
    emit(RUNNING);
    await waitFor(() => expect(screen.getByTestId("tool-progress")).toBeTruthy());
    // One-based: the second of three is running, so it reads "2 of 3".
    expect(screen.getByText("2 of 3")).toBeTruthy();
    expect(screen.getByTestId("tool-progress-file").textContent).toBe("episode-two.wav");
  });

  it("shows refusals as they happen, not only in the final report", async () => {
    render(<ToolProgressBar />);
    emit({ ...RUNNING, refused: 2 });
    await waitFor(() =>
      expect(screen.getByTestId("tool-progress-refused").textContent).toBe("2 refused"),
    );
  });

  it("clears itself when the run finishes", async () => {
    render(<ToolProgressBar />);
    emit(RUNNING);
    await waitFor(() => expect(screen.getByTestId("tool-progress")).toBeTruthy());
    emit({ kind: "batch_apply", done: true, total: 3, succeeded: 3, refused: 0 });
    await waitFor(() => expect(screen.queryByTestId("tool-progress")).toBeNull());
  });

  it("asks the backend to stop when cancelled", async () => {
    render(<ToolProgressBar />);
    emit(RUNNING);
    await waitFor(() => expect(screen.getByTestId("tool-progress")).toBeTruthy());
    fireEvent.click(screen.getByTestId("tool-progress-cancel"));
    expect(cancel).toHaveBeenCalledTimes(1);
  });

  /**
   * The stop lands at the end of the file in flight, so the button has
   * to stop claiming to be clickable — otherwise the wait reads as the
   * click not having registered.
   */
  it("says it is stopping rather than looking unclicked", async () => {
    render(<ToolProgressBar />);
    emit(RUNNING);
    await waitFor(() => expect(screen.getByTestId("tool-progress")).toBeTruthy());
    const btn = screen.getByTestId("tool-progress-cancel") as HTMLButtonElement;
    fireEvent.click(btn);
    await waitFor(() => expect(btn.textContent).toBe("Stopping…"));
    expect(btn.disabled).toBe(true);
  });
});
