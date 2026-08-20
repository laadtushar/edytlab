/**
 * Getting into a project at all (#189 follow-up).
 *
 * The report was "I still don't see create or open project", and both
 * halves of that turned out to be true in different ways.
 *
 * There was no **create** anywhere — no button, no menu entry, no
 * backend command. Making a project was already possible, because
 * `open_project` builds the store when the folder has none, but nothing
 * said so: you had to point "Open project…" at an empty folder and
 * notice that it worked.
 *
 * And **open** existed in exactly one place — the empty state, which
 * stops rendering the moment audio loads. So mid-session there was no
 * way to reach it, and no way to leave the current project for another.
 *
 * These tests pin both: the actions exist, and they are reachable from
 * the header rather than only from a screen that disappears.
 */

import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { AppHeader } from "../AppHeader";
import { EmptyState } from "../EmptyState";

function header(extra: Record<string, unknown> = {}) {
  const props = {
    leftView: "timeline" as const,
    onSelectView: vi.fn(),
    onOpen: vi.fn(),
    onSettings: vi.fn(),
    isRecording: false,
    onRecord: vi.fn(),
    ...extra,
  };
  render(<AppHeader {...props} />);
  return props;
}

describe("the header can start and open a project", () => {
  it("offers New project…", () => {
    const onNewProject = vi.fn();
    header({ onNewProject });
    fireEvent.click(screen.getByTestId("new-project-button"));
    expect(onNewProject).toHaveBeenCalledOnce();
  });

  it("offers Open project…", () => {
    const onOpenProject = vi.fn();
    header({ onOpenProject });
    fireEvent.click(screen.getByTestId("open-project-button"));
    expect(onOpenProject).toHaveBeenCalledOnce();
  });

  it("offers both while a project is already open", () => {
    // The actual bug: these used to live only on the empty state, so
    // once audio loaded there was no route to another project at all.
    header({
      onNewProject: vi.fn(),
      onOpenProject: vi.fn(),
      onSaveAs: vi.fn(),
      hasProject: true,
    });
    expect(screen.getByTestId("new-project-button")).toBeTruthy();
    expect(screen.getByTestId("open-project-button")).toBeTruthy();
    expect(screen.getByTestId("save-as-button")).toBeTruthy();
  });

  it("draws neither when the handlers are absent", () => {
    // They are optional props; a header without them must not render
    // dead buttons.
    header();
    expect(screen.queryByTestId("new-project-button")).toBeNull();
    expect(screen.queryByTestId("open-project-button")).toBeNull();
  });
});

describe("the empty state names both routes in", () => {
  it("shows New project… alongside Open project…", () => {
    render(
      <EmptyState
        onOpen={vi.fn()}
        onOpenProject={vi.fn()}
        onNewProject={vi.fn()}
      />,
    );
    expect(screen.getByTestId("empty-state-new-project-button")).toBeTruthy();
    expect(screen.getByTestId("empty-state-open-project-button")).toBeTruthy();
  });

  it("says what folder each one wants", () => {
    // The two do the same call and differ only in what they promise,
    // which is the thing the user is actually choosing between — so the
    // promise has to be written down.
    render(
      <EmptyState
        onOpen={vi.fn()}
        onOpenProject={vi.fn()}
        onNewProject={vi.fn()}
      />,
    );
    expect(
      screen.getByTestId("empty-state-new-project-button").getAttribute("title"),
    ).toMatch(/empty folder/i);
    expect(
      screen
        .getByTestId("empty-state-open-project-button")
        .getAttribute("title"),
    ).toMatch(/already contains/i);
  });
});
