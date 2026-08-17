/**
 * The recents list (#156).
 *
 * Launching showed an empty timeline with no way back to yesterday's
 * work except remembering where you put it. The backend has recorded
 * recents since the project object landed; this is the part that shows
 * them, so the tests are about what the list has to get right to be
 * worth looking at: the right project opens, the timestamps read as
 * time, and removing a row is visibly not removing a project.
 */

import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import {
  RecentProjects,
  relativeTime,
  shortPath,
} from "../components/RecentProjects";

const PROJECTS = [
  {
    path: "/Users/alice/Audio/episode-12",
    name: "Episode 12 — mixdown",
    last_opened_at: "2026-08-17T12:00:00Z",
  },
  {
    path: "/Users/alice/Audio/old-thing",
    name: "old-thing",
    last_opened_at: null,
  },
];

describe("RecentProjects", () => {
  it("renders nothing at all when there is nothing to show", () => {
    const { container } = render(
      <RecentProjects projects={[]} onOpen={vi.fn()} onForget={vi.fn()} />,
    );
    // Not an empty "Recent" heading — a first launch should look like a
    // first launch.
    expect(container.firstChild).toBeNull();
  });

  it("opens the project that was clicked", () => {
    const onOpen = vi.fn();
    render(
      <RecentProjects
        projects={PROJECTS}
        onOpen={onOpen}
        onForget={vi.fn()}
      />,
    );
    fireEvent.click(
      screen.getByTestId("recent-open-/Users/alice/Audio/old-thing"),
    );
    expect(onOpen).toHaveBeenCalledWith("/Users/alice/Audio/old-thing");
  });

  /**
   * Forgetting a row is not deleting a project, so the control says so:
   * a labelled remove button, not a destructive-looking one.
   */
  it("forgets a row without touching the project", () => {
    const onForget = vi.fn();
    const onOpen = vi.fn();
    render(
      <RecentProjects
        projects={PROJECTS}
        onOpen={onOpen}
        onForget={onForget}
      />,
    );
    const remove = screen.getByLabelText(
      "Remove Episode 12 — mixdown from recent projects",
    );
    fireEvent.click(remove);
    expect(onForget).toHaveBeenCalledWith("/Users/alice/Audio/episode-12");
    expect(onOpen).not.toHaveBeenCalled();
  });

  /**
   * Two projects can share a name — "episode-1" under two clients — so
   * the path has to be on screen.
   */
  it("shows the path as well as the name", () => {
    render(
      <RecentProjects projects={PROJECTS} onOpen={vi.fn()} onForget={vi.fn()} />,
    );
    // Both the name and the tail of the path end in "old-thing", which
    // is the point: the row carries each separately.
    expect(screen.getAllByText(/old-thing$/).length).toBe(2);
    expect(
      screen.getAllByText(/Users\/alice\/Audio/).length,
    ).toBeGreaterThanOrEqual(1);
  });
});

describe("relativeTime", () => {
  const now = Date.parse("2026-08-17T12:00:00Z");

  it("reads as time, not as a timestamp", () => {
    expect(relativeTime("2026-08-17T11:59:30Z", now)).toBe("just now");
    expect(relativeTime("2026-08-17T11:30:00Z", now)).toBe("30 minutes ago");
    expect(relativeTime("2026-08-17T09:00:00Z", now)).toBe("3 hours ago");
    expect(relativeTime("2026-08-15T12:00:00Z", now)).toBe("2 days ago");
  });

  it("singularises, because '1 days ago' looks like a bug", () => {
    expect(relativeTime("2026-08-16T12:00:00Z", now)).toBe("1 day ago");
    expect(relativeTime("2026-08-17T11:00:00Z", now)).toBe("1 hour ago");
  });

  /** Past a week the count stops being something anyone counts. */
  it("falls back to a date beyond a week", () => {
    const out = relativeTime("2026-07-01T12:00:00Z", now);
    expect(out).not.toContain("ago");
    expect(out.length).toBeGreaterThan(0);
  });

  it("says nothing rather than 'Invalid Date' for missing or broken input", () => {
    expect(relativeTime(null)).toBe("");
    expect(relativeTime(undefined)).toBe("");
    expect(relativeTime("not a date")).toBe("");
  });
});

describe("shortPath", () => {
  it("keeps the end, which is the half that identifies a project", () => {
    const long = `/Users/alice/${"deep/".repeat(20)}episode-12`;
    const out = shortPath(long);
    expect(out.startsWith("…")).toBe(true);
    expect(out.endsWith("episode-12")).toBe(true);
  });

  it("leaves a short path alone", () => {
    expect(shortPath("/a/b")).toBe("/a/b");
  });
});
