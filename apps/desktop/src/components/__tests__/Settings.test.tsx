/**
 * Settings — provider picker behaviour.
 *
 * The Rust IPC layer is mocked. We assert that:
 *  1. Both Anthropic + OpenRouter radios render with stable test ids.
 *  2. Toggling the radio fires `setActiveProvider` with the new id and
 *     swaps the visible label / placeholder so the user knows which key
 *     they're entering.
 *  3. "Test" routes through `testApiKeyFor(provider, key)` (not the
 *     legacy `testApiKey(key)`), so the probe hits the picked
 *     provider's endpoint.
 *  4. "Save" routes through `setApiKeyFor(provider, key)` and the input
 *     is wiped on success.
 *
 * We deliberately don't test the localStorage round-trip because the
 * useEffect runs synchronously after render and would just be a
 * tautology against the implementation.
 */

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const setApiKeyForMock = vi.fn();
const testApiKeyForMock = vi.fn();
const setActiveProviderMock = vi.fn();
const clearApiKeyMock = vi.fn();

vi.mock("../../lib/tauri-bridge", async () => {
  const actual = await vi.importActual<
    typeof import("../../lib/tauri-bridge")
  >("../../lib/tauri-bridge");
  return {
    ...actual,
    setApiKeyFor: (provider: string, key: string) =>
      setApiKeyForMock(provider, key),
    testApiKeyFor: (provider: string, key: string) =>
      testApiKeyForMock(provider, key),
    setActiveProvider: (provider: string) => setActiveProviderMock(provider),
    clearApiKey: () => clearApiKeyMock(),
  };
});

import { Settings } from "../Settings";

beforeEach(() => {
  setApiKeyForMock.mockReset();
  testApiKeyForMock.mockReset();
  setActiveProviderMock.mockReset();
  clearApiKeyMock.mockReset();
  setApiKeyForMock.mockResolvedValue(undefined);
  testApiKeyForMock.mockResolvedValue(undefined);
  setActiveProviderMock.mockResolvedValue(undefined);
  clearApiKeyMock.mockResolvedValue(undefined);
  // Reset localStorage to avoid leaking state across tests (the picker
  // remembers the previous selection there).
  window.localStorage.clear();
});

afterEach(() => {
  window.localStorage.clear();
});

describe("Settings provider picker", () => {
  it("renders both Anthropic and OpenRouter radios with Anthropic selected by default", () => {
    render(<Settings mode="blocking" onSaved={() => {}} />);

    const anth = screen.getByTestId("settings-provider-anthropic");
    const open = screen.getByTestId("settings-provider-openrouter");
    expect(anth).toBeChecked();
    expect(open).not.toBeChecked();

    // The labelled-input should match Anthropic by default.
    expect(
      screen.getByLabelText(/Anthropic API key/i),
    ).toBeInTheDocument();
  });

  it("switches active provider and updates the visible label when the user picks OpenRouter", async () => {
    render(<Settings mode="blocking" onSaved={() => {}} />);

    fireEvent.click(screen.getByTestId("settings-provider-openrouter"));

    await waitFor(() =>
      expect(setActiveProviderMock).toHaveBeenCalledWith("openrouter"),
    );
    // The picker's visual state and the key-input label both flip to
    // OpenRouter.
    expect(screen.getByTestId("settings-provider-openrouter")).toBeChecked();
    expect(
      screen.getByLabelText(/OpenRouter API key/i),
    ).toBeInTheDocument();
  });

  it("clears any typed key when the user switches provider, to prevent cross-provider credential leakage", async () => {
    render(<Settings mode="blocking" onSaved={() => {}} />);

    // User pastes an Anthropic key against the default Anthropic radio.
    const input = screen.getByTestId("settings-key-input") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "sk-ant-leak" } });
    expect(input.value).toBe("sk-ant-leak");

    // Switching to OpenRouter must wipe the field — saving here would
    // otherwise persist the Anthropic key under the OpenRouter slot.
    fireEvent.click(screen.getByTestId("settings-provider-openrouter"));
    await waitFor(() =>
      expect(setActiveProviderMock).toHaveBeenCalledWith("openrouter"),
    );
    expect(
      (screen.getByTestId("settings-key-input") as HTMLInputElement).value,
    ).toBe("");
  });

  it("Test button validates against the picked provider's endpoint", async () => {
    render(<Settings mode="blocking" onSaved={() => {}} />);

    // Switch to OpenRouter first.
    fireEvent.click(screen.getByTestId("settings-provider-openrouter"));
    await waitFor(() =>
      expect(setActiveProviderMock).toHaveBeenCalledWith("openrouter"),
    );

    // Type a key and click Test.
    fireEvent.change(screen.getByTestId("settings-key-input"), {
      target: { value: "sk-or-v1-test" },
    });
    fireEvent.click(screen.getByTestId("settings-test-button"));

    await waitFor(() =>
      expect(testApiKeyForMock).toHaveBeenCalledWith(
        "openrouter",
        "sk-or-v1-test",
      ),
    );
    // The success banner appears after the resolved test promise settles.
    await waitFor(() =>
      expect(screen.getByTestId("settings-test-ok")).toBeInTheDocument(),
    );
  });

  it("Save persists the key against the picked provider and wipes the input", async () => {
    const onSaved = vi.fn();
    render(<Settings mode="blocking" onSaved={onSaved} />);

    fireEvent.change(screen.getByTestId("settings-key-input"), {
      target: { value: "sk-ant-test" },
    });
    fireEvent.click(screen.getByTestId("settings-save-button"));

    await waitFor(() =>
      expect(setApiKeyForMock).toHaveBeenCalledWith("anthropic", "sk-ant-test"),
    );
    await waitFor(() => expect(onSaved).toHaveBeenCalled());

    // The input is wiped after a successful save.
    expect(
      (screen.getByTestId("settings-key-input") as HTMLInputElement).value,
    ).toBe("");
  });
});
