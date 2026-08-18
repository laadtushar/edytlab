/**
 * Save As appears only when there is a project to copy (#156).
 *
 * The button is drawn from `App`'s header, so the test renders the
 * header's own contract: a handler and whether a project is open. A
 * button that copies nothing would be worse than no button, and "no
 * project open" is the state a first launch sits in.
 */

import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { AppHeader } from "../components/AppHeader";

function renderHeader(props: Partial<Parameters<typeof AppHeader>[0]> = {}) {
  const onSaveAs = vi.fn();
  render(
    <AppHeader
      leftView="timeline"
      onSelectView={vi.fn()}
      onOpen={vi.fn()}
      onSettings={vi.fn()}
      isRecording={false}
      onRecord={vi.fn()}
      onSaveAs={onSaveAs}
      hasProject
      {...props}
    />,
  );
  return { onSaveAs };
}

describe("Save As in the header", () => {
  it("copies the project when clicked", () => {
    const { onSaveAs } = renderHeader();
    fireEvent.click(screen.getByTestId("save-as-button"));
    expect(onSaveAs).toHaveBeenCalled();
  });

  it("is absent with no project open", () => {
    renderHeader({ hasProject: false });
    expect(screen.queryByTestId("save-as-button")).toBeNull();
  });

  it("is absent when the caller cannot copy", () => {
    renderHeader({ onSaveAs: undefined });
    expect(screen.queryByTestId("save-as-button")).toBeNull();
  });
});
