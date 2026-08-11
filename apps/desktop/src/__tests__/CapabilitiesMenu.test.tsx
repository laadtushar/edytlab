/**
 * The capabilities menu is where a user goes to see and restrict what
 * the agent can reach. Four defects meant it did neither reliably: it
 * showed a snapshot from the first open, and two of its three checkbox
 * groups persisted a preference that nothing acted on.
 */

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const listCapabilities = vi.fn();
vi.mock("../lib/tauri-bridge", () => ({
  listCapabilities: (...a: unknown[]) => listCapabilities(...a),
}));

import { CapabilitiesMenu } from "../components/CapabilitiesMenu";

const EMPTY = { tools: [], skills: [], agents: [], mcp_servers: [] };

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  listCapabilities.mockResolvedValue(EMPTY);
});

describe("CapabilitiesMenu", () => {
  /**
   * The backend calls `reload_skills_from_disk()` on every
   * `list_capabilities` precisely so this surface stays current. Caching
   * the first result for the app's lifetime defeated that: the empty
   * state tells you to drop a .md into ~/.edytlab/skills/, and doing so
   * changed nothing visible until a restart.
   */
  it("reloads capabilities every time it opens", async () => {
    const { rerender } = render(
      <CapabilitiesMenu open={true} onClose={() => {}} />,
    );
    await waitFor(() => expect(listCapabilities).toHaveBeenCalledTimes(1));

    // A skill appears on disk between the two opens.
    listCapabilities.mockResolvedValue({
      ...EMPTY,
      skills: [
        { id: "podcast", name: "podcast", description: "cleanup", category: "always" },
      ],
    });

    rerender(<CapabilitiesMenu open={false} onClose={() => {}} />);
    rerender(<CapabilitiesMenu open={true} onClose={() => {}} />);

    await waitFor(() =>
      expect(
        listCapabilities,
        "the menu kept its first snapshot for the app's lifetime",
      ).toHaveBeenCalledTimes(2),
    );
    await screen.findByText("podcast");
  });

  /**
   * One transient IPC failure used to pin the error banner permanently:
   * `error` was never cleared and is checked before `caps`, so the menu
   * never recovered even though the retry would have succeeded.
   */
  it("recovers from a transient load failure on the next open", async () => {
    listCapabilities.mockRejectedValueOnce(new Error("ipc blip"));
    const { rerender } = render(
      <CapabilitiesMenu open={true} onClose={() => {}} />,
    );
    await screen.findByText(/Failed to load capabilities/);

    listCapabilities.mockResolvedValue({
      ...EMPTY,
      tools: [{ id: "gain", name: "gain", description: "set gain", category: "Volume" }],
    });
    rerender(<CapabilitiesMenu open={false} onClose={() => {}} />);
    rerender(<CapabilitiesMenu open={true} onClose={() => {}} />);

    await screen.findByText("gain");
    expect(screen.queryByText(/Failed to load capabilities/)).toBeNull();
  });

  /**
   * The menu persisted the readable `<server>::<tool>` name while the
   * backend blacklist is matched against the dispatcher's mangled
   * `<server>__<tool>`. The two never compare equal, so unchecking an
   * MCP tool looked like it worked — and persisted across restarts —
   * while the model could still call it on the very next turn.
   */
  it("persists the wire name for an MCP tool, not the display name", async () => {
    listCapabilities.mockResolvedValue({
      ...EMPTY,
      mcp_servers: [
        {
          id: "files__read_file",
          name: "files::read_file",
          description: "read a file",
          category: "files",
        },
      ],
    });
    render(<CapabilitiesMenu open={true} onClose={() => {}} />);

    const row = await screen.findByLabelText("Enable files::read_file");
    fireEvent.click(row);

    await waitFor(() => {
      const raw = localStorage.getItem("edytlab.capabilities.disabled");
      expect(raw, "nothing was persisted").not.toBeNull();
      const disabled = JSON.parse(raw as string) as string[];
      expect(
        disabled,
        "the blacklist matches dispatcher wire names; the display name never matches",
      ).toContain("files__read_file");
    });
  });

  /**
   * Skills cannot be filtered by the disabled list — `apply_blacklist`
   * only ever operates on tool names — so a skill checkbox claimed a
   * restriction nothing applied.
   */
  it("does not offer a checkbox for skills, which cannot be disabled", async () => {
    listCapabilities.mockResolvedValue({
      ...EMPTY,
      skills: [
        { id: "podcast", name: "podcast", description: "cleanup", category: "always" },
      ],
    });
    render(<CapabilitiesMenu open={true} onClose={() => {}} />);

    await screen.findByText("podcast");
    expect(
      screen.queryByLabelText("Enable podcast"),
      "a control that persists a preference nothing reads is worse than none",
    ).toBeNull();
  });
});
