/**
 * Settings — first-launch modal + panel behaviours.
 *
 * The bridge is mocked so we can drive Save/Test/Clear paths directly
 * and assert on the bridge calls. M13 acceptance criteria addressed
 * here:
 *  - #1: with no key, App renders blocking modal (covered in App-level
 *    smoke; here we cover the modal component itself).
 *  - #2: Test against a bad key surfaces the `"401 invalid x-api-key"`
 *    string verbatim, not an unhandled error.
 *  - #3: Clear transitions the UI back to first-launch state — the
 *    component invokes `onCleared`, which App.tsx wires to flipping
 *    `keyConfigured` back to `false`.
 *
 * Updated for the multi-provider abstraction: Save/Test now route
 * through `setApiKeyFor` / `testApiKeyFor` (provider-explicit), with
 * Anthropic as the default selection.
 */

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const setApiKeyForMock = vi.fn();
const testApiKeyForMock = vi.fn();
const clearApiKeyMock = vi.fn();
const setActiveProviderMock = vi.fn();

vi.mock("../lib/tauri-bridge", () => ({
  setApiKeyFor: (provider: string, key: string) =>
    setApiKeyForMock(provider, key),
  testApiKeyFor: (provider: string, key: string) =>
    testApiKeyForMock(provider, key),
  setActiveProvider: (provider: string) => setActiveProviderMock(provider),
  clearApiKey: () => clearApiKeyMock(),
}));

import { Settings } from "../components/Settings";

describe("Settings", () => {
  beforeEach(() => {
    setApiKeyForMock.mockReset().mockResolvedValue(undefined);
    testApiKeyForMock.mockReset().mockResolvedValue(undefined);
    clearApiKeyMock.mockReset().mockResolvedValue(undefined);
    setActiveProviderMock.mockReset().mockResolvedValue(undefined);
    window.localStorage.clear();
  });

  it("disables Save when the key field is empty", () => {
    render(<Settings mode="blocking" onSaved={vi.fn()} />);
    const save = screen.getByTestId("settings-save-button");
    expect(save).toBeDisabled();
  });

  it("calls setApiKeyFor with the active provider and onSaved when Save is clicked with a non-empty key", async () => {
    const onSaved = vi.fn();
    const user = userEvent.setup();
    render(<Settings mode="blocking" onSaved={onSaved} />);

    await user.type(screen.getByTestId("settings-key-input"), "sk-ant-good");
    await user.click(screen.getByTestId("settings-save-button"));

    expect(setApiKeyForMock).toHaveBeenCalledWith("anthropic", "sk-ant-good");
    expect(onSaved).toHaveBeenCalledTimes(1);
    // Input should be wiped after a successful save so the key does not
    // linger in component state.
    expect(screen.getByTestId("settings-key-input")).toHaveValue("");
  });

  it("shows the verbatim error from testApiKey on a failed Test", async () => {
    testApiKeyForMock.mockRejectedValueOnce("401 invalid x-api-key");
    const user = userEvent.setup();
    render(<Settings mode="blocking" onSaved={vi.fn()} />);

    await user.type(screen.getByTestId("settings-key-input"), "sk-ant-bad");
    await user.click(screen.getByTestId("settings-test-button"));

    const err = await screen.findByTestId("settings-test-error");
    expect(err.textContent).toContain("401 invalid x-api-key");
    // Sanity: this is rendered as an alert, not an unhandled-error
    // overlay or throw.
    expect(err).toHaveAttribute("role", "alert");
  });

  it("shows a green confirmation when Test succeeds", async () => {
    const user = userEvent.setup();
    render(<Settings mode="blocking" onSaved={vi.fn()} />);

    await user.type(screen.getByTestId("settings-key-input"), "sk-ant-good");
    await user.click(screen.getByTestId("settings-test-button"));

    expect(await screen.findByTestId("settings-test-ok")).toBeInTheDocument();
    expect(testApiKeyForMock).toHaveBeenCalledWith("anthropic", "sk-ant-good");
  });

  it("renders Clear only in panel mode and triggers onCleared", async () => {
    const onCleared = vi.fn();
    const user = userEvent.setup();
    const { rerender } = render(
      <Settings mode="blocking" onSaved={vi.fn()} onCleared={onCleared} />,
    );
    expect(screen.queryByTestId("settings-clear-button")).toBeNull();

    rerender(
      <Settings
        mode="panel"
        onSaved={vi.fn()}
        onClose={vi.fn()}
        onCleared={onCleared}
      />,
    );
    await user.click(screen.getByTestId("settings-clear-button"));

    expect(clearApiKeyMock).toHaveBeenCalledTimes(1);
    expect(onCleared).toHaveBeenCalledTimes(1);
  });

  it("persists the model selection to localStorage", async () => {
    const user = userEvent.setup();
    render(<Settings mode="panel" onSaved={vi.fn()} onClose={vi.fn()} />);

    await user.selectOptions(
      screen.getByTestId("settings-model-select"),
      "claude-haiku-4-5",
    );
    expect(window.localStorage.getItem("edytlab.model")).toBe(
      "claude-haiku-4-5",
    );
  });
});
