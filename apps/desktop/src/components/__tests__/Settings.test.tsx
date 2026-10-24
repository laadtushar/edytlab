/**
 * Settings — provider picker + model combo behaviour.
 *
 * The Rust IPC layer is mocked. We assert that:
 *  1. All three provider radios (Anthropic, OpenRouter, OpenAI) render
 *     with stable test ids.
 *  2. Toggling the radio fires `setActiveProvider` with the new id and
 *     swaps the visible label / placeholder so the user knows which key
 *     they're entering.
 *  3. "Test" routes through `testApiKeyFor(provider, key, …)`.
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
    testApiKeyFor: (
      provider: string,
      key: string,
      baseUrl?: string,
      model?: string,
    ) => testApiKeyForMock(provider, key, baseUrl, model),
    setActiveProvider: (provider: string) => setActiveProviderMock(provider),
    setActiveModel: (provider: string, model: string) =>
      setActiveModelMock(provider, model),
    getActiveModel: (provider: string) => getActiveModelMock(provider),
    listModelsFor: (provider: string, apiKey?: string) =>
      listModelsForMock(provider, apiKey),
    clearApiKey: () => clearApiKeyMock(),
  };
});

import type { ModelInfo } from "../../lib/tauri-bridge";
import { Settings } from "../Settings";

const info = (
  id: string,
  display_name: string,
): ModelInfo => ({
  id,
  display_name,
  context_length: 1_000_000,
  provider_hint: null,
  reasoning_levels: [],
});

/** The built-in list: `anthropic_fallback_models()` in `crates/ai/src/models.rs`. */
const ANTHROPIC_MODELS = [
  info("claude-sonnet-5-5", "Claude Sonnet 5.5 (default)"),
  info("claude-haiku-5-5", "Claude Haiku 5.5"),
  info("claude-opus-5-5", "Claude Opus 5.5"),
  info("claude-fable-5-1", "Claude Fable 5.1"),
];

/** What `list_models_for` answers: the models and which list they are. */
const live = (models: ModelInfo[]) => ({
  models,
  source: "live" as const,
  notice: null,
});

const ANTHROPIC_CATALOGUE = live(ANTHROPIC_MODELS);

const ANTHROPIC_FALLBACK = {
  models: ANTHROPIC_MODELS,
  source: "fallback" as const,
  notice: "no API key saved",
};

const OPENAI_CATALOGUE = live([
  { ...info("gpt-6-luna", "gpt-6-luna (default)"), context_length: null },
  { ...info("gpt-6-astra", "gpt-6-astra"), context_length: null },
]);

beforeEach(() => {
  setApiKeyForMock.mockReset();
  testApiKeyForMock.mockReset();
  setActiveProviderMock.mockReset();
  setActiveModelMock.mockReset();
  getActiveModelMock.mockReset();
  clearApiKeyMock.mockReset();
  listModelsForMock.mockReset();
  setApiKeyForMock.mockResolvedValue(undefined);
  testApiKeyForMock.mockResolvedValue({
    model: "claude-sonnet-5-5",
    toolsOk: true,
    detail: null,
    modelUnavailable: false,
  });
  setActiveProviderMock.mockResolvedValue(undefined);
  setActiveModelMock.mockResolvedValue(undefined);
  getActiveModelMock.mockResolvedValue("");
  clearApiKeyMock.mockResolvedValue(undefined);
  // Default: catalogue resolves to a live Anthropic list. Tests
  // override per-provider as needed.
  listModelsForMock.mockImplementation(async (p: string) => {
    if (p === "anthropic") return ANTHROPIC_CATALOGUE;
    if (p === "openai") return OPENAI_CATALOGUE;
    return live([]);
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
        undefined,
        expect.any(String),
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
    // The wipe is drawn after the save resolves, not with it.
    await waitFor(() => {
      expect(onSaved).toHaveBeenCalled();
      expect(
        (screen.getByTestId("settings-key-input") as HTMLInputElement).value,
      ).toBe("");
    });
  });
});

describe("Settings model combo", () => {
  it("populates the datalist from the live catalogue", async () => {
    render(<Settings mode="blocking" onSaved={() => {}} />);

    await waitFor(() =>
      expect(listModelsForMock).toHaveBeenCalledWith("anthropic", undefined),
    );

    const datalist = await screen.findByTestId("settings-model-datalist");
    // The datalist is drawn before the catalogue arrives, so wait for its
    // options, not for it: one <option> per curated entry, the first
    // matching the first catalogue id.
    await waitFor(() => {
      const opts = datalist.querySelectorAll("option");
      expect(opts.length).toBe(ANTHROPIC_CATALOGUE.models.length);
      expect(opts[0].getAttribute("value")).toBe("claude-sonnet-5-5");
    });
    // A live list says how many models it holds and nothing about a fallback.
    const hint = await screen.findByTestId("settings-model-hint");
    expect(hint.textContent).toMatch(/4 models available/);
    expect(screen.queryByTestId("settings-model-fallback")).toBeNull();
  });

  // The list shown is not always Anthropic's. With no key to fetch it with, or
  // when the fetch fails, the backend answers with edytlab's built-in list, and
  // the hint says so rather than presenting it as the provider's own.
  it("says when the list is edytlab's built-in one, and why", async () => {
    listModelsForMock.mockResolvedValue(ANTHROPIC_FALLBACK);
    render(<Settings mode="blocking" onSaved={() => {}} />);

    const note = await screen.findByTestId("settings-model-fallback");
    expect(note.textContent).toContain("built-in list");
    expect(note.textContent).toContain("no API key saved");
    expect(note.textContent).toContain("Anthropic");
    expect(screen.queryByTestId("settings-model-hint")).toBeNull();

    // The suggestions are still there to pick from.
    const datalist = screen.getByTestId("settings-model-datalist");
    await waitFor(() => {
      const ids = Array.from(datalist.querySelectorAll("option")).map((o) =>
        o.getAttribute("value"),
      );
      expect(ids).toEqual([
        "claude-sonnet-5-5",
        "claude-haiku-5-5",
        "claude-opus-5-5",
        "claude-fable-5-1",
      ]);
    });
  });

  // A failed fetch has no source to speak of; the error says what happened.
  it("shows the error and no source when the fetch fails", async () => {
    listModelsForMock.mockRejectedValue(new Error("boom"));
    render(<Settings mode="blocking" onSaved={() => {}} />);
    await screen.findByTestId("settings-model-error");
    expect(screen.queryByTestId("settings-model-fallback")).toBeNull();
    expect(screen.queryByTestId("settings-model-hint")).toBeNull();
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
      target: { value: "claude-haiku-5-5" },
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
      "claude-haiku-5-5",
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
      ).toBe("claude-haiku-5-5"),
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

/**
 * A saved model the provider has retired (#468). Settings used to list
 * it, or nothing, and TEST reported it as a model that "did not call the
 * tool". Anthropic's live list is complete, so a saved model missing from it
 * is flagged; another provider's is filtered or spelt differently, so
 * absence there proves nothing.
 */
describe("Settings, when the saved model is gone", () => {
  const RETIRED = "claude-opus-4-1-20250805";

  it("warns when a saved model is missing from Anthropic's live list, and offers the default", async () => {
    getActiveModelMock.mockResolvedValue(RETIRED);
    render(<Settings mode="blocking" onSaved={() => {}} />);

    const warning = await screen.findByTestId("settings-model-unlisted");
    expect(warning.textContent).toContain(RETIRED);
    expect(warning.textContent).toMatch(/retired/);
    expect(warning.textContent).toContain("claude-sonnet-5-5");
    expect(setActiveModelMock).not.toHaveBeenCalled();

    fireEvent.click(screen.getByTestId("settings-model-use-default"));

    expect(setActiveModelMock).toHaveBeenCalledWith(
      "anthropic",
      "claude-sonnet-5-5",
    );
    await waitFor(() =>
      expect(
        (screen.getByTestId("settings-model-input") as HTMLInputElement).value,
      ).toBe("claude-sonnet-5-5"),
    );
    expect(screen.queryByTestId("settings-model-unlisted")).toBeNull();
  });

  // `claude-haiku-4-5` is an alias that resolves to the dated id the list
  // holds, so it is a model Anthropic has.
  it("does not warn about an alias of a listed dated id", async () => {
    listModelsForMock.mockResolvedValue(
      live([info("claude-haiku-4-5-20251001", "Claude Haiku 4.5")]),
    );
    getActiveModelMock.mockResolvedValue("claude-haiku-4-5");
    render(<Settings mode="blocking" onSaved={() => {}} />);

    await screen.findByTestId("settings-model-hint");
    await waitFor(() =>
      expect(
        (screen.getByTestId("settings-model-input") as HTMLInputElement).value,
      ).toBe("claude-haiku-4-5"),
    );
    expect(screen.queryByTestId("settings-model-unlisted")).toBeNull();
  });

  it("does not warn about a model that is listed, nor about the default", async () => {
    getActiveModelMock.mockResolvedValue("claude-opus-5-5");
    render(<Settings mode="blocking" onSaved={() => {}} />);
    await screen.findByTestId("settings-model-hint");
    expect(screen.queryByTestId("settings-model-unlisted")).toBeNull();
  });

  // The built-in list is edytlab's, not the provider's, so what it lacks says
  // nothing about what the provider has.
  it("does not warn against the built-in list", async () => {
    listModelsForMock.mockResolvedValue(ANTHROPIC_FALLBACK);
    getActiveModelMock.mockResolvedValue(RETIRED);
    render(<Settings mode="blocking" onSaved={() => {}} />);
    await screen.findByTestId("settings-model-fallback");
    await waitFor(() =>
      expect(
        (screen.getByTestId("settings-model-input") as HTMLInputElement).value,
      ).toBe(RETIRED),
    );
    expect(screen.queryByTestId("settings-model-unlisted")).toBeNull();
  });

  it("does not warn for another provider's list", async () => {
    getActiveModelMock.mockResolvedValue("some-model-not-listed");
    render(<Settings mode="blocking" onSaved={() => {}} />);
    fireEvent.click(screen.getByTestId("settings-provider-openai"));
    await waitFor(() =>
      expect(setActiveProviderMock).toHaveBeenCalledWith("openai"),
    );
    await screen.findByTestId("settings-model-hint");
    expect(screen.queryByTestId("settings-model-unlisted")).toBeNull();
  });

  it("reports a retired model from TEST as gone, not as unable to call tools", async () => {
    getActiveModelMock.mockResolvedValue(RETIRED);
    testApiKeyForMock.mockResolvedValue({
      model: RETIRED,
      toolsOk: false,
      detail: null,
      modelUnavailable: true,
    });
    render(<Settings mode="blocking" onSaved={() => {}} />);
    fireEvent.change(screen.getByTestId("settings-key-input"), {
      target: { value: "k" },
    });
    await waitFor(() =>
      expect(
        (screen.getByTestId("settings-model-input") as HTMLInputElement).value,
      ).toBe(RETIRED),
    );
    fireEvent.click(screen.getByTestId("settings-test-button"));

    const missing = await screen.findByTestId("settings-test-model-missing");
    expect(missing.textContent).toContain(RETIRED);
    expect(missing.textContent).toContain("Anthropic");
    expect(missing.textContent).toMatch(/retired/);
    expect(screen.queryByTestId("settings-test-no-tools")).toBeNull();
    expect(screen.queryByTestId("settings-test-ok")).toBeNull();

    fireEvent.click(screen.getByTestId("settings-test-use-default"));
    expect(setActiveModelMock).toHaveBeenCalledWith(
      "anthropic",
      "claude-sonnet-5-5",
    );
  });

  it("offers no switch from TEST when the model that is gone is the default", async () => {
    testApiKeyForMock.mockResolvedValue({
      model: "claude-sonnet-5-5",
      toolsOk: false,
      detail: null,
      modelUnavailable: true,
    });
    render(<Settings mode="blocking" onSaved={() => {}} />);
    fireEvent.change(screen.getByTestId("settings-key-input"), {
      target: { value: "k" },
    });
    fireEvent.click(screen.getByTestId("settings-test-button"));

    await screen.findByTestId("settings-test-model-missing");
    expect(screen.queryByTestId("settings-test-use-default")).toBeNull();
  });
});
