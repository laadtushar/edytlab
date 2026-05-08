/**
 * Settings — provider picker + model combo behaviour.
 *
 * The Rust IPC layer is mocked. We assert that:
 *  1. All three provider radios (Anthropic, OpenRouter, OpenAI) render
 *     with stable test ids.
 *  2. Toggling the radio fires `setActiveProvider` with the new id and
 *     swaps the visible label / placeholder so the user knows which key
 *     they're entering.
 *  3. "Test" routes through `testApiKeyFor(provider, key)`.
 *  4. "Save" routes through `setApiKeyFor(provider, key)` and the input
 *     is wiped on success.
 *  5. The model combo populates suggestions from the catalogue mock
 *     and accepts free-form input not in the list.
 *  6. Per-provider model selection round-trips through localStorage.
 *  7. A failing catalogue fetch surfaces a graceful inline hint and
 *     does NOT block the user from typing a model id manually.
 */

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const setApiKeyForMock = vi.fn();
const testApiKeyForMock = vi.fn();
const setActiveProviderMock = vi.fn();
const setActiveModelMock = vi.fn();
const getActiveModelMock = vi.fn();
const clearApiKeyMock = vi.fn();
const listModelsForMock = vi.fn();

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
    setActiveModel: (provider: string, model: string) =>
      setActiveModelMock(provider, model),
    getActiveModel: (provider: string) => getActiveModelMock(provider),
    listModelsFor: (provider: string, apiKey?: string) =>
      listModelsForMock(provider, apiKey),
    clearApiKey: () => clearApiKeyMock(),
  };
});

import { Settings } from "../Settings";

const ANTHROPIC_CATALOGUE = [
  {
    id: "claude-sonnet-4-6",
    display_name: "Claude Sonnet 4.6 (default)",
    context_length: 200000,
    provider_hint: null,
  },
  {
    id: "claude-haiku-4-5-20251001",
    display_name: "Claude Haiku 4.5 (cheap mode)",
    context_length: 200000,
    provider_hint: null,
  },
  {
    id: "claude-opus-4-1-20250805",
    display_name: "Claude Opus 4.1",
    context_length: 200000,
    provider_hint: null,
  },
];

const OPENAI_CATALOGUE = [
  {
    id: "gpt-4o-mini",
    display_name: "gpt-4o-mini",
    context_length: null,
    provider_hint: null,
  },
  {
    id: "gpt-4o",
    display_name: "gpt-4o",
    context_length: null,
    provider_hint: null,
  },
];

beforeEach(() => {
  setApiKeyForMock.mockReset();
  testApiKeyForMock.mockReset();
  setActiveProviderMock.mockReset();
  setActiveModelMock.mockReset();
  getActiveModelMock.mockReset();
  clearApiKeyMock.mockReset();
  listModelsForMock.mockReset();
  setApiKeyForMock.mockResolvedValue(undefined);
  testApiKeyForMock.mockResolvedValue(undefined);
  setActiveProviderMock.mockResolvedValue(undefined);
  setActiveModelMock.mockResolvedValue(undefined);
  getActiveModelMock.mockResolvedValue("");
  clearApiKeyMock.mockResolvedValue(undefined);
  // Default: catalogue resolves to the Anthropic curated list. Tests
  // override per-provider as needed.
  listModelsForMock.mockImplementation(async (p: string) => {
    if (p === "anthropic") return ANTHROPIC_CATALOGUE;
    if (p === "openai") return OPENAI_CATALOGUE;
    return [];
  });
  window.localStorage.clear();
});

afterEach(() => {
  window.localStorage.clear();
});

describe("Settings provider picker", () => {
  it("renders Anthropic, OpenRouter, AND OpenAI radios with Anthropic selected by default", () => {
    render(<Settings mode="blocking" onSaved={() => {}} />);

    expect(screen.getByTestId("settings-provider-anthropic")).toBeChecked();
    expect(screen.getByTestId("settings-provider-openrouter")).not.toBeChecked();
    expect(screen.getByTestId("settings-provider-openai")).not.toBeChecked();

    expect(screen.getByLabelText(/Anthropic API key/i)).toBeInTheDocument();
  });

  it("switches active provider and updates the visible label when the user picks OpenAI", async () => {
    render(<Settings mode="blocking" onSaved={() => {}} />);

    fireEvent.click(screen.getByTestId("settings-provider-openai"));

    await waitFor(() =>
      expect(setActiveProviderMock).toHaveBeenCalledWith("openai"),
    );
    expect(screen.getByTestId("settings-provider-openai")).toBeChecked();
    expect(screen.getByLabelText(/OpenAI API key/i)).toBeInTheDocument();
  });

  it("clears any typed key when the user switches provider, to prevent cross-provider credential leakage", async () => {
    render(<Settings mode="blocking" onSaved={() => {}} />);

    const input = screen.getByTestId("settings-key-input") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "sk-ant-leak" } });
    expect(input.value).toBe("sk-ant-leak");

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

    fireEvent.click(screen.getByTestId("settings-provider-openrouter"));
    await waitFor(() =>
      expect(setActiveProviderMock).toHaveBeenCalledWith("openrouter"),
    );

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

    expect(
      (screen.getByTestId("settings-key-input") as HTMLInputElement).value,
    ).toBe("");
  });
});

describe("Settings model combo", () => {
  it("populates the datalist from the live catalogue", async () => {
    render(<Settings mode="blocking" onSaved={() => {}} />);

    await waitFor(() =>
      expect(listModelsForMock).toHaveBeenCalledWith("anthropic", undefined),
    );

    const datalist = await screen.findByTestId("settings-model-datalist");
    // The datalist should contain one <option> per curated entry.
    const opts = datalist.querySelectorAll("option");
    expect(opts.length).toBe(ANTHROPIC_CATALOGUE.length);
    // The first option's value matches the first catalogue id.
    expect(opts[0].getAttribute("value")).toBe("claude-sonnet-4-6");
  });

  it("accepts free-form model ids not in the suggestion list", async () => {
    render(<Settings mode="blocking" onSaved={() => {}} />);
    const input = screen.getByTestId("settings-model-input") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "claude-future-99" } });
    expect(input.value).toBe("claude-future-99");
    // Local persistence so the next mount restores it.
    expect(window.localStorage.getItem("edytlab.model.anthropic")).toBe(
      "claude-future-99",
    );
  });

  it("persists per-provider model selection across provider toggles", async () => {
    render(<Settings mode="blocking" onSaved={() => {}} />);

    // Type an Anthropic-specific id under the Anthropic radio.
    fireEvent.change(screen.getByTestId("settings-model-input"), {
      target: { value: "claude-haiku-4-5-20251001" },
    });

    // Switch to OpenAI; the input should reset to OpenAI's default
    // (or whatever was previously stored under
    // edytlab.model.openai — empty here, so the placeholder default).
    fireEvent.click(screen.getByTestId("settings-provider-openai"));
    await waitFor(() =>
      expect(setActiveProviderMock).toHaveBeenCalledWith("openai"),
    );

    // Type an OpenAI-specific id.
    fireEvent.change(screen.getByTestId("settings-model-input"), {
      target: { value: "gpt-4o" },
    });

    // Both selections must be persisted independently.
    expect(window.localStorage.getItem("edytlab.model.anthropic")).toBe(
      "claude-haiku-4-5-20251001",
    );
    expect(window.localStorage.getItem("edytlab.model.openai")).toBe("gpt-4o");

    // Switch back to Anthropic; the field should restore the prior
    // Anthropic-side selection.
    fireEvent.click(screen.getByTestId("settings-provider-anthropic"));
    await waitFor(() =>
      expect(setActiveProviderMock).toHaveBeenCalledWith("anthropic"),
    );
    await waitFor(() =>
      expect(
        (screen.getByTestId("settings-model-input") as HTMLInputElement).value,
      ).toBe("claude-haiku-4-5-20251001"),
    );
  });

  it("migrates the legacy edytlab.model key onto the per-provider Anthropic slot", async () => {
    window.localStorage.setItem("edytlab.model", "claude-legacy-pinned");
    render(<Settings mode="blocking" onSaved={() => {}} />);
    const input = screen.getByTestId("settings-model-input") as HTMLInputElement;
    expect(input.value).toBe("claude-legacy-pinned");
    expect(window.localStorage.getItem("edytlab.model")).toBeNull();
    expect(window.localStorage.getItem("edytlab.model.anthropic")).toBe(
      "claude-legacy-pinned",
    );
  });

  it("degrades gracefully when the catalogue fetch errors", async () => {
    listModelsForMock.mockRejectedValueOnce(new Error("boom"));
    render(<Settings mode="blocking" onSaved={() => {}} />);

    const err = await screen.findByTestId("settings-model-error");
    expect(err.textContent).toMatch(/Couldn't fetch model list/i);

    // The input is still usable — user can type a free-form id.
    const input = screen.getByTestId("settings-model-input") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "claude-typed-by-hand" } });
    expect(input.value).toBe("claude-typed-by-hand");
  });
});
