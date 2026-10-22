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

vi.mock("../lib/tauri-bridge", () => ({
  mlRuntimeStatus: () => mlRuntimeStatusMock(),
  // The Account tab's speech model row (#383): not downloaded, and nothing
  // to hear. Plain functions, so a test's `mockReset` leaves them alone.
  mlModelStatus: () =>
    Promise.resolve({
      model: "whisper-base",
      state: "not_downloaded",
      bytes: 0,
      size: 138_644_904,
    }),
  onMlModelProgress: () => Promise.resolve(() => undefined),
  downloadMlModel: vi.fn(),
  cancelMlModelDownload: vi.fn(),
  removeMlModel: vi.fn(),
  getActiveProvider: vi.fn().mockResolvedValue(""),
  getActiveModel: vi.fn().mockResolvedValue(""),
  getBaseUrlFor: vi.fn().mockResolvedValue(null),
  defaultBaseUrlFor: vi.fn().mockResolvedValue("https://api.anthropic.com"),
  getEffortFor: vi.fn().mockResolvedValue(null),
  setEffortFor: vi.fn(),
  listModelsFor: vi
    .fn()
    .mockResolvedValue({ models: [], source: "live", notice: null }),
  hasApiKeyFor: vi.fn().mockResolvedValue(true),
  clearApiKeyFor: vi.fn(),
  setApiKeyFor: vi.fn(),
  setActiveProvider: vi.fn(),
  setActiveModel: vi.fn(),
  setBaseUrlFor: vi.fn(),
  testApiKeyFor: vi.fn(),
  installPlugin: vi.fn(),
}));

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
