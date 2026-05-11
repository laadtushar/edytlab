/**
 * MemoryEditor — verifies the round-trip through the read/write bridge:
 *   1. Mount reads both scopes; both panes render with the loaded value.
 *   2. Save is disabled until the textarea diverges from the loaded value.
 *   3. Save calls writeMemory and clears the dirty state.
 *   4. The project pane gracefully degrades to a disabled textarea when
 *      no project is open (read rejects with "no project").
 */

import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const readMemoryMock = vi.fn();
const writeMemoryMock = vi.fn();

vi.mock("../../lib/tauri-bridge", () => ({
  readMemory: (scope: string) => readMemoryMock(scope),
  writeMemory: (scope: string, contents: string) =>
    writeMemoryMock(scope, contents),
}));

import { MemoryEditor } from "../MemoryEditor";

describe("MemoryEditor", () => {
  beforeEach(() => {
    readMemoryMock.mockReset();
    writeMemoryMock.mockReset().mockResolvedValue(undefined);
  });

  it("loads both panes from the backend on mount", async () => {
    readMemoryMock.mockImplementation((scope: string) =>
      Promise.resolve(scope === "global" ? "global-text" : "project-text"),
    );

    render(<MemoryEditor />);

    await waitFor(() => {
      expect(readMemoryMock).toHaveBeenCalledWith("global");
      expect(readMemoryMock).toHaveBeenCalledWith("project");
    });

    const globalTa = screen.getByTestId(
      "memory-textarea-global",
    ) as HTMLTextAreaElement;
    const projectTa = screen.getByTestId(
      "memory-textarea-project",
    ) as HTMLTextAreaElement;
    await waitFor(() => expect(globalTa.value).toBe("global-text"));
    expect(projectTa.value).toBe("project-text");
  });

  it("disables Save until the textarea diverges, then writes on click", async () => {
    readMemoryMock.mockResolvedValue("");
    const user = userEvent.setup();
    render(<MemoryEditor />);

    await waitFor(() => expect(readMemoryMock).toHaveBeenCalledTimes(2));

    const save = screen.getByTestId("memory-save-global");
    expect(save).toBeDisabled();

    const ta = screen.getByTestId("memory-textarea-global");
    await user.type(ta, "hello");

    expect(save).not.toBeDisabled();
    await user.click(save);

    await waitFor(() => {
      expect(writeMemoryMock).toHaveBeenCalledWith("global", "hello");
    });
    // After save, the baseline updates so Save goes back to disabled.
    expect(save).toBeDisabled();
    expect(screen.getByTestId("memory-status-global").textContent).toContain(
      "Saved",
    );
  });

  it("disables the project pane when no project is open", async () => {
    readMemoryMock.mockImplementation((scope: string) =>
      scope === "global"
        ? Promise.resolve("")
        : Promise.reject("project scope requested but no project is open"),
    );

    render(<MemoryEditor />);

    const projectTa = (await screen.findByTestId(
      "memory-textarea-project",
    )) as HTMLTextAreaElement;
    await waitFor(() => expect(projectTa).toBeDisabled());
    // Global pane stays editable.
    expect(screen.getByTestId("memory-textarea-global")).not.toBeDisabled();
  });

  it("surfaces a write error in the status line", async () => {
    readMemoryMock.mockResolvedValue("");
    writeMemoryMock.mockRejectedValueOnce("disk full");

    const user = userEvent.setup();
    render(<MemoryEditor />);
    await waitFor(() => expect(readMemoryMock).toHaveBeenCalledTimes(2));

    const ta = screen.getByTestId("memory-textarea-global");
    await user.type(ta, "x");
    await act(async () => {
      await user.click(screen.getByTestId("memory-save-global"));
    });

    await waitFor(() => {
      expect(screen.getByTestId("memory-status-global").textContent).toContain(
        "disk full",
      );
    });
  });
});
