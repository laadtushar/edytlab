/**
 * Settings shows which ONNX Runtime library the app found (#383).
 *
 * In the panel, at the foot of the Account tab. Not in the first-run card:
 * that one is already as tall as a small window allows
 * (`e2e/settings-fits-window.spec.ts`), and the line is of no use to someone
 * who has not set a key. The line's wording is `MlRuntimeLine.test.tsx`'s;
 * this checks where it is mounted.
 */

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mlRuntimeStatusMock = vi.fn();

vi.mock("../lib/tauri-bridge", async () => {
  const { crashReportsRow, restingAccount, speechRow } = await import("./settingsBridge");
  return { ...restingAccount(), ...speechRow(), ...crashReportsRow(), mlRuntimeStatus: () => mlRuntimeStatusMock() };
});

import { Settings } from "../components/Settings";

describe("Settings: the ONNX Runtime line", () => {
  beforeEach(() => {
    mlRuntimeStatusMock.mockReset();
    window.localStorage.clear();
  });

  it("shows a bundled runtime's version in the panel's Account tab", async () => {
    mlRuntimeStatusMock.mockResolvedValue({
      state: "found",
      version: "1.23.2",
      path: "/usr/lib/edytlab/libonnxruntime.so",
      source: "bundled",
    });
    render(<Settings mode="panel" onSaved={vi.fn()} onClose={vi.fn()} />);

    expect(
      await screen.findByText("ONNX Runtime: 1.23.2 (bundled)"),
    ).toBeInTheDocument();
  });

  it("says so when the build carries no runtime", async () => {
    mlRuntimeStatusMock.mockResolvedValue({
      state: "missing",
      searched: ["/usr/lib/edytlab/libonnxruntime.so"],
    });
    render(<Settings mode="panel" onSaved={vi.fn()} onClose={vi.fn()} />);

    expect(
      await screen.findByText("ONNX Runtime: not installed in this build"),
    ).toBeInTheDocument();
  });

  it("shows the failure when the status cannot be had", async () => {
    mlRuntimeStatusMock.mockRejectedValue("no session loaded");
    render(<Settings mode="panel" onSaved={vi.fn()} onClose={vi.fn()} />);

    expect(
      await screen.findByText(/ONNX Runtime: could not be checked \(no session loaded\)/),
    ).toBeInTheDocument();
  });

  it("is on the Account tab only", async () => {
    mlRuntimeStatusMock.mockResolvedValue({ state: "missing", searched: [] });
    const user = userEvent.setup();
    render(<Settings mode="panel" onSaved={vi.fn()} onClose={vi.fn()} />);
    await screen.findByTestId("settings-ml-runtime");

    await user.click(screen.getByTestId("settings-tab-plugins"));
    expect(screen.queryByTestId("settings-ml-runtime")).toBeNull();
  });

  it("is left out of the first-run card, which does not ask for it", async () => {
    render(<Settings mode="blocking" onSaved={vi.fn()} />);
    await screen.findByTestId("settings-provider-picker");

    expect(screen.queryByTestId("settings-ml-runtime")).toBeNull();
    expect(mlRuntimeStatusMock).not.toHaveBeenCalled();
  });
});
