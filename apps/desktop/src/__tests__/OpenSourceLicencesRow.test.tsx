/**
 * "Open source licences" in Settings → Account.
 *
 * The notices file is read when Show is pressed and not before, is drawn as
 * plain text (a licence text with markup characters in it stays text), and a
 * build without the file says so in a sentence instead of an empty panel.
 */

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const noticesMock = vi.fn();

vi.mock("../lib/tauri-bridge", () => ({
  thirdPartyNotices: () => noticesMock(),
}));

import { OpenSourceLicencesRow } from "../components/OpenSourceLicencesRow";

describe("OpenSourceLicencesRow", () => {
  beforeEach(() => {
    noticesMock.mockReset();
  });

  it("reads nothing until Show is pressed", () => {
    render(<OpenSourceLicencesRow />);
    expect(screen.getByTestId("settings-licences-toggle")).toHaveTextContent("Show");
    expect(noticesMock).not.toHaveBeenCalled();
    expect(screen.queryByTestId("settings-licences-text")).toBeNull();
  });

  it("shows the notices as plain text and hides them again", async () => {
    const text = "MIT License\n\nCopyright (c) <script>alert(1)</script> Someone\n";
    noticesMock.mockResolvedValue(text);
    const user = userEvent.setup();
    render(<OpenSourceLicencesRow />);

    await user.click(screen.getByTestId("settings-licences-toggle"));
    const pre = await screen.findByTestId("settings-licences-text");
    expect(pre.tagName).toBe("PRE");
    expect(pre.textContent).toBe(text);
    expect(pre.querySelector("script")).toBeNull();
    expect(screen.getByTestId("settings-licences-toggle")).toHaveTextContent("Hide");
    expect(screen.getByTestId("settings-licences-toggle")).toHaveAttribute("aria-expanded", "true");

    await user.click(screen.getByTestId("settings-licences-toggle"));
    expect(screen.queryByTestId("settings-licences-text")).toBeNull();
    expect(noticesMock).toHaveBeenCalledTimes(1);
  });

  it("says so when the build has no notices file", async () => {
    // Tauri rejects an invoke with the command's error string.
    noticesMock.mockRejectedValue("The open source licences file is not in this build.");
    const user = userEvent.setup();
    render(<OpenSourceLicencesRow />);

    await user.click(screen.getByTestId("settings-licences-toggle"));
    const error = await screen.findByTestId("settings-licences-error");
    expect(error).toHaveTextContent("not in this build");
    expect(screen.queryByTestId("settings-licences-text")).toBeNull();
    // Press Show again to retry.
    expect(screen.getByTestId("settings-licences-toggle")).toHaveTextContent("Show");
  });
});
