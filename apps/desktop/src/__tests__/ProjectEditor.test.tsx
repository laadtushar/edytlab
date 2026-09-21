/**
 * Naming a project, and setting what it exports with (#225 §5).
 *
 * `get_project_meta` and `set_project_meta` shipped with no UI
 * reference at all, so the only way to name a project was to ask the
 * agent in a sentence — which needs a working LLM (#321) to do
 * something entirely deterministic.
 */

import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const getProjectMeta = vi.fn();
const setProjectMeta = vi.fn();

vi.mock("../lib/tauri-bridge", () => ({
  getProjectMeta: () => getProjectMeta(),
  setProjectMeta: (
    name: string,
    notes?: string,
    artist?: string,
    album?: string,
    year?: string,
  ) => setProjectMeta(name, notes, artist, album, year),
}));

import { ProjectEditor } from "../components/ProjectEditor";

const META = {
  name: "Episode 12",
  notes: "rough cut",
  artist: "A Podcast",
  album: "Season 2",
  year: "2026",
};

describe("the project editor", () => {
  beforeEach(() => {
    getProjectMeta.mockReset().mockResolvedValue(META);
    setProjectMeta.mockReset().mockImplementation((name: string) =>
      Promise.resolve({ ...META, name }),
    );
  });

  it("shows what the project already says", async () => {
    render(<ProjectEditor />);
    await waitFor(() =>
      expect(screen.getByTestId("project-name")).toHaveValue("Episode 12"),
    );
    expect(screen.getByTestId("project-artist")).toHaveValue("A Podcast");
    expect(screen.getByTestId("project-album")).toHaveValue("Season 2");
    expect(screen.getByTestId("project-year")).toHaveValue("2026");
    expect(screen.getByTestId("project-notes")).toHaveValue("rough cut");
  });

  it("saves every field, so a rename does not clear the tags", async () => {
    // Each argument left out means "leave that one alone" on the Rust
    // side, so sending a partial set here would silently blank the
    // rest.
    render(<ProjectEditor />);
    await waitFor(() => expect(screen.getByTestId("project-name")).toBeTruthy());

    fireEvent.change(screen.getByTestId("project-name"), {
      target: { value: "Episode 13" },
    });
    fireEvent.click(screen.getByTestId("project-save"));

    await waitFor(() =>
      expect(setProjectMeta).toHaveBeenCalledWith(
        "Episode 13",
        "rough cut",
        "A Podcast",
        "Season 2",
        "2026",
      ),
    );
  });

  it("refuses an empty name rather than sending it", async () => {
    // `set_project_meta` rejects an empty name. Catching it here turns
    // a round-trip and an error into a disabled button with a reason.
    render(<ProjectEditor />);
    await waitFor(() => expect(screen.getByTestId("project-name")).toBeTruthy());

    fireEvent.change(screen.getByTestId("project-name"), {
      target: { value: "   " },
    });

    expect(screen.getByTestId("project-save")).toBeDisabled();
    expect(screen.getByTestId("project-name-required")).toBeInTheDocument();
    expect(setProjectMeta).not.toHaveBeenCalled();
  });

  it("says so when it saved", async () => {
    render(<ProjectEditor />);
    await waitFor(() => expect(screen.getByTestId("project-name")).toBeTruthy());
    fireEvent.click(screen.getByTestId("project-save"));
    await waitFor(() =>
      expect(screen.getByTestId("project-saved")).toBeInTheDocument(),
    );
  });

  it("reports a save failure instead of looking like it worked", async () => {
    setProjectMeta.mockRejectedValue(new Error("read-only volume"));
    render(<ProjectEditor />);
    await waitFor(() => expect(screen.getByTestId("project-name")).toBeTruthy());

    fireEvent.click(screen.getByTestId("project-save"));

    await waitFor(() =>
      expect(screen.getByTestId("project-save-error").textContent).toMatch(
        /read-only volume/,
      ),
    );
    expect(screen.queryByTestId("project-saved")).toBeNull();
  });

  it("treats no open project as an ordinary state, not a fault", async () => {
    // Opening Settings before opening a project is a normal thing to
    // do; it should not look like something broke.
    getProjectMeta.mockRejectedValue(new Error("no project is open"));
    render(<ProjectEditor />);

    await waitFor(() =>
      expect(screen.getByTestId("project-editor-empty")).toBeInTheDocument(),
    );
    expect(screen.queryByTestId("project-editor-error")).toBeNull();
  });

  it("still reports a real failure to read", async () => {
    getProjectMeta.mockRejectedValue(new Error("permission denied"));
    render(<ProjectEditor />);

    await waitFor(() =>
      expect(screen.getByTestId("project-editor-error").textContent).toMatch(
        /permission denied/,
      ),
    );
  });

  it("has no title field, because the name is the title", async () => {
    // A second box holding the same thing is a pair that can disagree.
    render(<ProjectEditor />);
    await waitFor(() => expect(screen.getByTestId("project-name")).toBeTruthy());
    expect(screen.queryByTestId("project-title")).toBeNull();
    // And the name's own label has to say what it does, or nothing does.
    expect(screen.getByTestId("project-editor").textContent).toMatch(
      /title tag/i,
    );
  });

  it("tolerates a project that has set none of the tags", async () => {
    getProjectMeta.mockResolvedValue({ name: "take3" });
    render(<ProjectEditor />);
    await waitFor(() =>
      expect(screen.getByTestId("project-name")).toHaveValue("take3"),
    );
    expect(screen.getByTestId("project-artist")).toHaveValue("");
  });
});
