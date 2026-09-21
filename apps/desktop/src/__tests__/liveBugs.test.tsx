/**
 * The onboarding lockout, found by launching the built app twice.
 *
 * The zoom and abort regressions that used to live here moved to
 * `liveBugs.timeline.test.tsx`, which drives the real `Timeline`
 * instead of a local copy of its arithmetic — raised in review on
 * #320, and correctly: the copies stayed green when the production
 * wiring was reverted.
 */

import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

// Settings talks to the keychain through the bridge. The whole point
// of the onboarding case below is *which* bridge call it makes, so the
// calls are spies rather than a stubbed-out module.
const setApiKeyFor = vi.fn((_p: string, _k: string) => Promise.resolve());
const setActiveProvider = vi.fn((_p: string) => Promise.resolve());

vi.mock("../lib/tauri-bridge", () => ({
  setApiKeyFor: (p: string, k: string) => setApiKeyFor(p, k),
  setActiveProvider: (p: string) => setActiveProvider(p),
  clearApiKeyFor: vi.fn(() => Promise.resolve()),
  setBaseUrlFor: vi.fn(() => Promise.resolve()),
  getBaseUrlFor: vi.fn(() => Promise.resolve("")),
  defaultBaseUrlFor: vi.fn(() => Promise.resolve("")),
  listModelsFor: vi.fn(() => Promise.resolve([])),
  setActiveModel: vi.fn(() => Promise.resolve()),
  getActiveModel: vi.fn(() => Promise.resolve("")),
  getActiveProvider: vi.fn(() => Promise.resolve("")),
  hasApiKeyFor: vi.fn(() => Promise.resolve(false)),
  testApiKeyFor: vi.fn(() => Promise.resolve({ toolsOk: true })),
  installPlugin: vi.fn(() => Promise.resolve({ summary: "" })),
}));

import { PROVIDER_STORAGE_KEY, Settings } from "../components/Settings";

describe("onboarding with a keyless provider already selected", () => {
  /**
   * The lockout. `handleSave` wrote `setApiKeyFor(provider, "")` for
   * every provider, and the keychain rejects an empty secret. A new
   * user never saw it, because *clicking* the Ollama radio activates
   * the provider and that is what dismisses the blocking modal.
   *
   * On the next launch Ollama is restored from storage, so the radio
   * handler never fires — the selection has not changed — and the only
   * button on a modal with no Close is one that always errors.
   *
   * Mounted with the provider pre-stored, which is exactly the state a
   * returning user opens the app in.
   */
  beforeEach(() => {
    setApiKeyFor.mockClear();
    setActiveProvider.mockClear();
    window.localStorage.setItem(PROVIDER_STORAGE_KEY, "ollama");
  });

  it("activates the provider instead of writing an empty key", async () => {
    const onSaved = vi.fn();
    render(<Settings mode="blocking" onSaved={onSaved} />);

    fireEvent.click(await screen.findByText("Save & Continue"));

    await waitFor(() => expect(setActiveProvider).toHaveBeenCalledWith("ollama"));
    expect(
      setApiKeyFor,
      "wrote an empty secret to the keychain, which it rejects",
    ).not.toHaveBeenCalled();
    await waitFor(() => expect(onSaved).toHaveBeenCalled());
  });

  it("still writes the key for a provider that needs one", async () => {
    window.localStorage.setItem(PROVIDER_STORAGE_KEY, "anthropic");
    render(<Settings mode="blocking" onSaved={vi.fn()} />);

    fireEvent.change(await screen.findByPlaceholderText("sk-ant-..."), {
      target: { value: "sk-ant-test" },
    });
    fireEvent.click(screen.getByText("Save & Continue"));

    await waitFor(() =>
      expect(setApiKeyFor).toHaveBeenCalledWith("anthropic", "sk-ant-test"),
    );
  });
});
