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
const clearApiKeyForMock = vi.fn();
const setActiveProviderMock = vi.fn();
const hasApiKeyForMock = vi.fn();
const listModelsForMock = vi.fn();
const setActiveModelMock = vi.fn();
// Settings reconciles the dropdown against the backend on mount (#249).
// Empty means "the backend has no opinion", so the stored value stands
// and these tests see the behaviour they were written for.
const getActiveModelMock = vi.fn();
// Same reconciliation, for the provider (#225 §3). Empty means "the
// backend has no opinion", so the stored value stands and the existing
// tests below see the behaviour they were written for.
const getActiveProviderMock = vi.fn();
const getBaseUrlForMock = vi.fn();
const defaultBaseUrlForMock = vi.fn();
const setBaseUrlForMock = vi.fn();

vi.mock("../lib/tauri-bridge", () => ({
  setApiKeyFor: (provider: string, key: string) =>
    setApiKeyForMock(provider, key),
  testApiKeyFor: (
    provider: string,
    key: string,
    baseUrl?: string,
    model?: string,
  ) => testApiKeyForMock(provider, key, baseUrl, model),
  setActiveProvider: (provider: string) => setActiveProviderMock(provider),
  hasApiKeyFor: (provider: string) => hasApiKeyForMock(provider),
  setActiveModel: (provider: string, model: string) =>
    setActiveModelMock(provider, model),
  getActiveModel: (provider: string) => getActiveModelMock(provider),
  getActiveProvider: () => getActiveProviderMock(),
  listModelsFor: (provider: string, apiKey?: string) =>
    listModelsForMock(provider, apiKey),
  clearApiKey: () => clearApiKeyMock(),
  clearApiKeyFor: (provider: string) => clearApiKeyForMock(provider),
  getBaseUrlFor: (provider: string) => getBaseUrlForMock(provider),
  defaultBaseUrlFor: (provider: string) => defaultBaseUrlForMock(provider),
  setBaseUrlFor: (provider: string, baseUrl: string) =>
    setBaseUrlForMock(provider, baseUrl),
}));

import { PROVIDER_STORAGE_KEY, Settings } from "../components/Settings";
import { held } from "./held";

describe("Settings", () => {
  beforeEach(() => {
    setApiKeyForMock.mockReset().mockResolvedValue(undefined);
    testApiKeyForMock
      .mockReset()
      .mockResolvedValue({ model: "claude-sonnet-4-6", toolsOk: true, detail: null });
    clearApiKeyMock.mockReset().mockResolvedValue(undefined);
    clearApiKeyForMock.mockReset().mockResolvedValue(undefined);
    setActiveProviderMock.mockReset().mockResolvedValue(undefined);
    hasApiKeyForMock.mockReset().mockResolvedValue(true);
    setActiveModelMock.mockReset().mockResolvedValue(undefined);
    getActiveModelMock.mockReset().mockResolvedValue("");
    getActiveProviderMock.mockReset().mockResolvedValue("");
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
    // Input should be wiped after a successful save so the key does not
    // linger in component state. The save is awaited inside the click
    // handler, so the wipe is drawn some time after the click resolves.
    await waitFor(() =>
      expect(screen.getByTestId("settings-key-input")).toHaveValue(""),
    );
    expect(onSaved).toHaveBeenCalledTimes(1);
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
    expect(testApiKeyForMock).toHaveBeenCalledWith(
      "anthropic",
      "sk-ant-good",
      undefined,
      "claude-sonnet-4-6",
    );
  });

  /**
   * The middle state. A model that connects but ignores tools passes the
   * old reachability test and then fails on the first edit — the panel
   * has to say so, and say which model is at fault, rather than showing
   * a green tick.
   */
  it("warns instead of confirming when the model cannot call tools", async () => {
    testApiKeyForMock.mockResolvedValueOnce({
      model: "gemma-2-9b",
      toolsOk: false,
      detail: "Sure, ok = true.",
    });
    const user = userEvent.setup();
    render(<Settings mode="blocking" onSaved={vi.fn()} />);

    await user.type(screen.getByTestId("settings-key-input"), "sk-ant-good");
    await user.click(screen.getByTestId("settings-test-button"));

    const warn = await screen.findByTestId("settings-test-no-tools");
    expect(warn.textContent).toContain("gemma-2-9b");
    expect(warn.textContent).toMatch(/editing will not work/i);
    expect(warn.textContent).toContain("Sure, ok = true.");
    expect(screen.queryByTestId("settings-test-ok")).toBeNull();
    expect(screen.queryByTestId("settings-test-error")).toBeNull();
  });

  /**
   * Test has to probe the endpoint that is on screen. It used to probe
   * the provider's default, so testing a local server reported on a
   * server the user was not about to use.
   */
  it("tests the typed base URL and model, not the saved ones", async () => {
    const user = userEvent.setup();
    render(<Settings mode="blocking" onSaved={vi.fn()} />);

    await user.type(
      screen.getByTestId("settings-base-url-input"),
      "http://localhost:1234/v1",
    );
    await user.clear(screen.getByTestId("settings-model-input"));
    await user.type(screen.getByTestId("settings-model-input"), "local-model");
    await user.type(screen.getByTestId("settings-key-input"), "sk-ant-good");
    await user.click(screen.getByTestId("settings-test-button"));

    await waitFor(() =>
      expect(testApiKeyForMock).toHaveBeenCalledWith(
        "anthropic",
        "sk-ant-good",
        "http://localhost:1234/v1",
        "local-model",
      ),
    );
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

    // Clears the provider on screen, by name. It used to call the
    // argument-less `clearApiKey`, which resolves the slot from the
    // *backend's* active provider — so deleting a credential depended
    // on the UI and the backend agreeing which provider is current,
    // and #225 §3 is open because they have two sources of truth for
    // that.
    expect(clearApiKeyForMock).toHaveBeenCalledTimes(1);
    expect(clearApiKeyForMock).toHaveBeenCalledWith("anthropic");
    expect(clearApiKeyMock).not.toHaveBeenCalled();
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

  // ---- The dropdown reflects the agent, not just this browser (#249) ----
  //
  // The model lived only in memory on the Rust side and nothing
  // re-pushed it at startup, so after a restart the agent ran the
  // provider default while this control went on displaying the user's
  // pick from localStorage. Nothing ever read `get_active_model` back,
  // so the disagreement was invisible.

  it("shows what the agent is configured with, not the stale stored value", async () => {
    window.localStorage.setItem("edytlab.model.anthropic", "stale-pick");
    getActiveModelMock.mockResolvedValue("claude-opus-5");

    render(<Settings mode="panel" onSaved={vi.fn()} onClose={vi.fn()} />);

    await waitFor(() =>
      expect(screen.getByTestId("settings-model-input")).toHaveValue(
        "claude-opus-5",
      ),
    );
    // And the stored value is reconciled, so the next mount agrees
    // before the IPC even resolves.
    expect(window.localStorage.getItem("edytlab.model.anthropic")).toBe(
      "claude-opus-5",
    );
  });

  it("keeps the stored value when the backend has no opinion yet", async () => {
    window.localStorage.setItem("edytlab.model.anthropic", "my-pick");
    const read = held<string>();
    getActiveModelMock.mockReturnValue(read.promise);

    render(<Settings mode="panel" onSaved={vi.fn()} onClose={vi.fn()} />);

    await waitFor(() => expect(getActiveModelMock).toHaveBeenCalled());
    await read.resolve("");
    expect(screen.getByTestId("settings-model-input")).toHaveValue("my-pick");
  });

  // The same disagreement, one line above the model's (#225 §3). The
  // radio group read localStorage alone and nothing ever called
  // `get_active_provider`, so Settings could show one provider while
  // the agent answered on another — and the model list, base URL,
  // Test and Clear in this panel all key off the one on screen.

  it("selects the agent's provider, not the stale stored one", async () => {
    window.localStorage.setItem(PROVIDER_STORAGE_KEY, "anthropic");
    getActiveProviderMock.mockResolvedValue("openai");

    render(<Settings mode="panel" onSaved={vi.fn()} onClose={vi.fn()} />);

    // The OpenAI placeholder is only rendered while OpenAI is selected.
    // And the stored value is reconciled, so the next mount agrees
    // before the IPC even resolves. Both inside the wait: the value is
    // written by an effect, which React runs after the draw — asserting
    // it once the placeholder showed failed on Windows CI whenever the
    // effect had not run yet.
    await waitFor(() => {
      expect(screen.getByPlaceholderText("sk-...")).toBeInTheDocument();
      expect(window.localStorage.getItem(PROVIDER_STORAGE_KEY)).toBe("openai");
    });
  });

  it("keeps the stored provider when the backend has no opinion", async () => {
    window.localStorage.setItem(PROVIDER_STORAGE_KEY, "groq");
    const read = held<string>();
    getActiveProviderMock.mockReturnValue(read.promise);

    render(<Settings mode="panel" onSaved={vi.fn()} onClose={vi.fn()} />);

    await waitFor(() => expect(getActiveProviderMock).toHaveBeenCalled());
    await read.resolve("");
    expect(screen.getByPlaceholderText("gsk_...")).toBeInTheDocument();
  });

  it("ignores a provider id this build does not offer", async () => {
    // Leaving it selected would light no radio at all, and every
    // keyed-off-provider control would address a provider with no row.
    window.localStorage.setItem(PROVIDER_STORAGE_KEY, "groq");
    const read = held<string>();
    getActiveProviderMock.mockReturnValue(read.promise);

    render(<Settings mode="panel" onSaved={vi.fn()} onClose={vi.fn()} />);

    await waitFor(() => expect(getActiveProviderMock).toHaveBeenCalled());
    await read.resolve("mistral");
    expect(screen.getByPlaceholderText("gsk_...")).toBeInTheDocument();
  });

  it("does not undo a selection made while the read is in flight", async () => {
    // The race Copilot raised on #324. `cancelled` only covers
    // unmount, so a click landing before the hydration resolves was
    // snapped back to the value that read before it happened —
    // discarding a selection the backend had already been told about.
    const user = userEvent.setup();
    window.localStorage.setItem(PROVIDER_STORAGE_KEY, "anthropic");

    const read = held<string>();
    getActiveProviderMock.mockReturnValue(read.promise);

    render(<Settings mode="panel" onSaved={vi.fn()} onClose={vi.fn()} />);
    await waitFor(() => expect(getActiveProviderMock).toHaveBeenCalled());

    // The user picks Groq while the read is still pending.
    await user.click(screen.getByTestId("settings-provider-groq"));
    await waitFor(() =>
      expect(setActiveProviderMock).toHaveBeenCalledWith("groq"),
    );

    // The read now lands, carrying what was true before the click.
    await read.resolve("anthropic");

    // Groq stays selected: its key field is the one on screen.
    expect(screen.getByPlaceholderText("gsk_...")).toBeInTheDocument();
    expect(window.localStorage.getItem(PROVIDER_STORAGE_KEY)).toBe("groq");
  });

  it("falls back to the stored provider when the backend call fails", async () => {
    window.localStorage.setItem(PROVIDER_STORAGE_KEY, "groq");
    const read = held<string>();
    getActiveProviderMock.mockReturnValue(read.promise);

    render(<Settings mode="panel" onSaved={vi.fn()} onClose={vi.fn()} />);

    await waitFor(() => expect(getActiveProviderMock).toHaveBeenCalled());
    await read.reject(new Error("ipc down"));
    expect(screen.getByPlaceholderText("gsk_...")).toBeInTheDocument();
  });

  /// A failed selection means the dropdown and the agent disagree —
  /// the one state worth saying out loud. It used to be swallowed.
  it("reports a failure to select the model instead of swallowing it", async () => {
    const user = userEvent.setup();
    setActiveModelMock.mockRejectedValue(new Error("keychain locked"));

    render(<Settings mode="panel" onSaved={vi.fn()} onClose={vi.fn()} />);
    await user.clear(screen.getByTestId("settings-model-input"));
    await user.type(screen.getByTestId("settings-model-input"), "x");

    await waitFor(() =>
      expect(screen.getByText(/keychain locked/i)).toBeInTheDocument(),
    );
  });

  // ---- Switching provider must not kill the agent silently (#250) ----
  //
  // Selecting a radio activates the provider immediately and rebuilds
  // the agent, which sets it to None when the new provider needs a key
  // and none is stored. Nothing said so: `has_api_key` was read once on
  // mount, and `has_api_key_for` — written for exactly this — had no
  // caller. The user clicked a radio to look at a model list and found
  // out at the next chat message.

  it("warns when the newly selected provider has no key stored", async () => {
    const user = userEvent.setup();
    hasApiKeyForMock.mockResolvedValue(false);
    render(<Settings mode="panel" onSaved={vi.fn()} onClose={vi.fn()} />);

    await user.click(screen.getByTestId("settings-provider-groq"));

    expect(hasApiKeyForMock).toHaveBeenCalledWith("groq");
    await waitFor(() =>
      expect(screen.getByText(/no api key stored/i)).toBeInTheDocument(),
    );
  });

  it("tells the parent the agent is now unconfigured", async () => {
    const user = userEvent.setup();
    hasApiKeyForMock.mockResolvedValue(false);
    const onProviderChanged = vi.fn();
    render(
      <Settings
        mode="panel"
        onSaved={vi.fn()}
        onClose={vi.fn()}
        onProviderChanged={onProviderChanged}
      />,
    );

    await user.click(screen.getByTestId("settings-provider-groq"));

    await waitFor(() => expect(onProviderChanged).toHaveBeenCalledWith(false));
  });

  it("does not warn when the provider already has a key", async () => {
    const user = userEvent.setup();
    const hasKey = held<boolean>();
    hasApiKeyForMock.mockReturnValue(hasKey.promise);
    const onProviderChanged = vi.fn();
    render(
      <Settings
        mode="panel"
        onSaved={vi.fn()}
        onClose={vi.fn()}
        onProviderChanged={onProviderChanged}
      />,
    );

    await user.click(screen.getByTestId("settings-provider-groq"));
    await waitFor(() => expect(hasApiKeyForMock).toHaveBeenCalledWith("groq"));
    // Answered inside `act`, so the warning — if there were one — is
    // drawn before the check below, not after it.
    await hasKey.resolve(true);

    expect(onProviderChanged).toHaveBeenCalledWith(true);
    expect(screen.queryByText(/no api key stored/i)).not.toBeInTheDocument();
  });

  /// Ollama needs no key, so switching to it is never a deactivation —
  /// warning there would train the user to ignore the warning.
  it("does not warn for a provider that needs no key", async () => {
    const user = userEvent.setup();
    const hasKey = held<boolean>();
    hasApiKeyForMock.mockReturnValue(hasKey.promise);
    const onProviderChanged = vi.fn();
    render(
      <Settings
        mode="panel"
        onSaved={vi.fn()}
        onClose={vi.fn()}
        onProviderChanged={onProviderChanged}
      />,
    );

    await user.click(screen.getByTestId("settings-provider-ollama"));
    await waitFor(() => expect(hasApiKeyForMock).toHaveBeenCalledWith("ollama"));
    await hasKey.resolve(false);

    expect(onProviderChanged).toHaveBeenCalledWith(true);
    expect(screen.queryByText(/no api key stored/i)).not.toBeInTheDocument();
  });
});
