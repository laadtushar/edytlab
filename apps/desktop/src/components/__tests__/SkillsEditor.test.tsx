/**
 * SkillsEditor — CRUD smoke. The bridge is mocked so the test asserts
 * on the IPC calls the editor makes, not on disk side-effects.
 */

import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const listSkillsMock = vi.fn();
const readSkillMock = vi.fn();
const upsertSkillMock = vi.fn();
const deleteSkillMock = vi.fn();

vi.mock("../../lib/tauri-bridge", () => ({
  listSkills: () => listSkillsMock(),
  readSkill: (name: string) => readSkillMock(name),
  upsertSkill: (name: string, content: unknown) =>
    upsertSkillMock(name, content),
  deleteSkill: (name: string) => deleteSkillMock(name),
}));

import { SkillsEditor } from "../SkillsEditor";

const STUB = {
  name: "mix",
  description: "mixing glossary",
  trigger: "keywords" as const,
  keywords: ["mix"],
  pattern: "",
  enabled: true,
  body: "Mix terminology…",
};

describe("SkillsEditor", () => {
  beforeEach(() => {
    listSkillsMock.mockReset().mockResolvedValue([
      { name: "mix", description: "mixing glossary", trigger: "keywords: mix", enabled: true },
    ]);
    readSkillMock.mockReset().mockResolvedValue(STUB);
    upsertSkillMock.mockReset().mockResolvedValue(undefined);
    deleteSkillMock.mockReset().mockResolvedValue(undefined);
    vi.spyOn(window, "confirm").mockReturnValue(true);
  });

  it("lists skills on mount and opens one on click", async () => {
    const user = userEvent.setup();
    render(<SkillsEditor />);

    await waitFor(() => {
      expect(listSkillsMock).toHaveBeenCalled();
    });
    await user.click(await screen.findByTestId("skills-row-mix"));
    await waitFor(() => {
      expect(readSkillMock).toHaveBeenCalledWith("mix");
    });
    expect(
      (screen.getByTestId("skills-name") as HTMLInputElement).value,
    ).toBe("mix");
  });

  it("creates a new skill via the New button + Save", async () => {
    const user = userEvent.setup();
    render(<SkillsEditor />);
    await waitFor(() => expect(listSkillsMock).toHaveBeenCalled());

    await user.click(screen.getByTestId("skills-new"));
    const name = screen.getByTestId("skills-name") as HTMLInputElement;
    await user.type(name, "ducking");

    await act(async () => {
      await user.click(screen.getByTestId("skills-save"));
    });

    await waitFor(() => {
      expect(upsertSkillMock).toHaveBeenCalled();
    });
    const [calledName, calledContent] = upsertSkillMock.mock.calls[0];
    expect(calledName).toBe("ducking");
    expect((calledContent as { name: string }).name).toBe("ducking");
  });

  it("deletes an existing skill after confirmation", async () => {
    const user = userEvent.setup();
    render(<SkillsEditor />);
    await waitFor(() => expect(listSkillsMock).toHaveBeenCalled());

    await user.click(await screen.findByTestId("skills-row-mix"));
    await waitFor(() => expect(readSkillMock).toHaveBeenCalled());

    await act(async () => {
      await user.click(screen.getByTestId("skills-delete"));
    });

    await waitFor(() => {
      expect(deleteSkillMock).toHaveBeenCalledWith("mix");
    });
  });
});
