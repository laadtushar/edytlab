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

import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const setApiKeyForMock = vi.fn();
const testApiKeyForMock = vi.fn();
const clearApiKeyMock = vi.fn();
const setActiveProviderMock = vi.fn();
const listModelsForMock = vi.fn();
const setActiveModelMock = vi.fn();
const getBaseUrlForMock = vi.fn();
const defaultBaseUrlForMock = vi.fn();
const setBaseUrlForMock = vi.fn();

vi.mock("../lib/tauri-bridge", () => ({
  setApiKeyFor: (provider: string, key: string) =>
    setApiKeyForMock(provider, key),
  testApiKeyFor: (provider: string, key: string) =>
    testApiKeyForMock(provider, key),
  setActiveProvider: (provider: string) => setActiveProviderMock(provider),
  setActiveModel: (provider: string, model: string) =>
    setActiveModelMock(provider, model),
  listModelsFor: (provider: string, apiKey?: string) =>
    listModelsForMock(provider, apiKey),
  clearApiKey: () => clearApiKeyMock(),
  getBaseUrlFor: (provider: string) => getBaseUrlForMock(provider),
  defaultBaseUrlFor: (provider: string) => defaultBaseUrlForMock(provider),
  setBaseUrlFor: (provider: string, baseUrl: string) =>
    setBaseUrlForMock(provider, baseUrl),
}));

import { Settings } from "../components/Settings";

describe("Settings", () => {
  beforeEach(() => {
    setApiKeyForMock.mockReset().mockResolvedValue(undefined);
    testApiKeyForMock.mockReset().mockResolvedValue(undefined);
    clearApiKeyMock.mockReset().mockResolvedValue(undefined);
    setActiveProviderMock.mockReset().mockResolvedValue(undefined);
    setActiveModelMock.mockReset().mockResolvedValue(undefined);
    listModelsForMock.mockReset().mockResolvedValue([]);
    getBaseUrlForMock.mockReset().mockResolvedValue(null);
    defaultBaseUrlForMock
      .mockReset()
      .mockResolvedValue("https://api.anthropic.com");
    setBaseUrlForMock.mockReset().mockResolvedValue(undefined);
    window.localStorage.clear();
  });

  it("disables Save when the key field is empty", () => {
    render(<Settings mode="blocking" onSaved={vi.fn()} />);
    const save = screen.getByTestId("settings-save-button");
    expect(save).toBeDisabled();
  });

  it("saves a base URL, and saves it before the key so the rebuilt agent uses it", async () => {
    const user = userEvent.setup();
    render(<Settings mode="blocking" onSaved={vi.fn()} />);

    await user.type(
      screen.getByTestId("settings-base-url-input"),
      "http://localhost:1234/v1",
    );
    await user.type(screen.getByTestId("settings-key-input"), "sk-ant-good");
    await user.click(screen.getByTestId("settings-save-button"));

    expect(setBaseUrlForMock).toHaveBeenCalledWith(
      "anthropic",
      "http://localhost:1234/v1",
    );
    // `setApiKeyFor` rebuilds the agent and the rebuild reads the stored
    // URL, so the other order would leave the agent on the old endpoint.
    expect(setBaseUrlForMock.mock.invocationCallOrder[0]).toBeLessThan(
      setApiKeyForMock.mock.invocationCallOrder[0],
    );
  });

  it("shows the provider's own endpoint as the placeholder", async () => {
    defaultBaseUrlForMock.mockResolvedValue("http://localhost:11434/v1");
    render(<Settings mode="blocking" onSaved={vi.fn()} />);

    const input = await screen.findByTestId("settings-base-url-input");
    await waitFor(() =>
      expect(input).toHaveAttribute("placeholder", "http://localhost:11434/v1"),
    );
  });

  it("prefills an override that was already saved", async () => {
    getBaseUrlForMock.mockResolvedValue("https://gateway.internal/v1");
    render(<Settings mode="blocking" onSaved={vi.fn()} />);

    const input = await screen.findByTestId("settings-base-url-input");
    await waitFor(() => expect(input).toHaveValue("https://gateway.internal/v1"));
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

  it("persists the model selection to per-provider localStorage", async () => {
    const user = userEvent.setup();
    render(<Settings mode="panel" onSaved={vi.fn()} onClose={vi.fn()} />);

    // The model picker is now a free-form combo (text input + datalist)
    // so users can type any id, and we store per-provider.
    await user.clear(screen.getByTestId("settings-model-input"));
    await user.type(
      screen.getByTestId("settings-model-input"),
      "claude-haiku-4-5",
    );
    expect(window.localStorage.getItem("edytlab.model.anthropic")).toBe(
      "claude-haiku-4-5",
    );
  });
});
