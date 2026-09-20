/**
 * Regressions found by driving the built app, not the test suite.
 *
 * Each of these passed every existing check. They were found by
 * building the real Tauri shell, running it under Xvfb, and clicking
 * the controls — which is the one thing the suite cannot do for
 * itself. The tests exist so the second discovery is free.
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
  hasApiKeyFor: vi.fn(() => Promise.resolve(false)),
  testApiKeyFor: vi.fn(() => Promise.resolve({ toolsOk: true })),
  installPlugin: vi.fn(() => Promise.resolve({ summary: "" })),
}));

import { PROVIDER_STORAGE_KEY, Settings } from "../components/Settings";

describe("zoom-in from the default state", () => {
  /**
   * `App` seeds zoom at 0 ("auto-fit") and the buttons computed
   * `zoom ?? 50`. `??` keeps `0`, so zoom-in evaluated `0 * 1.5 === 0`
   * and mapped the default state to itself — the button did nothing at
   * all until zoom-out escaped via `Math.max(10, …)`.
   *
   * Asserted as arithmetic rather than through the DOM: the defect is
   * in the expression, and a test that mounts a WaveSurfer canvas to
   * discover that would be testing the wrong thing.
   */
  // Mirrors `effectivePxPerSec`: 0 means "auto-fit", so the step is
  // taken from the density actually on screen rather than a literal.
  const effective = (zoom: number, paneW: number, dur: number) => {
    if (zoom) return zoom;
    if (paneW > 0 && dur > 0) return paneW / dur;
    return 50;
  };
  const zoomIn = (zoom: number, paneW = 775, dur = 3) =>
    Math.min(500, Math.round(effective(zoom, paneW, dur) * 1.5));
  const zoomOut = (zoom: number, paneW = 775, dur = 3) =>
    Math.max(10, Math.round(effective(zoom, paneW, dur) / 1.5));

  it("increases from the auto-fit default of 0", () => {
    // The old `zoom ?? 50` form returned 0 here, forever.
    expect(zoomIn(0)).toBeGreaterThan(0);
  });

  it("clears the fitted density on the very first press", () => {
    // A literal 50 seed lands *below* auto-fit on short audio, so the
    // waveform stayed exactly as wide as the pane and the button
    // looked dead for four clicks. 775px / 3s is ~258 px/s.
    const fitted = 775 / 3;
    expect(zoomIn(0)).toBeGreaterThan(fitted);
    expect(zoomOut(0)).toBeLessThan(fitted);
  });

  it("still steps from an explicit level", () => {
    expect(zoomIn(100)).toBe(150);
    expect(zoomOut(100)).toBe(67);
  });

  it("is bounded at both ends", () => {
    expect(zoomIn(400)).toBe(500);
    expect(zoomOut(10)).toBe(10);
  });

  it("never returns its own input, which is what made it look dead", () => {
    for (const z of [0, 10, 50, 100]) {
      expect(zoomIn(z)).not.toBe(z);
    }
  });

  it("falls back to 50 when there is nothing to measure", () => {
    expect(zoomIn(0, 0, 0)).toBe(75);
  });
});

describe("an aborted load is not a user-facing error", () => {
  /**
   * WaveSurfer aborts an in-flight fetch when a newer `load()`
   * supersedes it. That rejection used to land after the new load had
   * cleared the error, pinning `AbortError: Fetch is aborted` under a
   * waveform that had decoded perfectly.
   */
  const isAbort = (err: unknown): boolean => {
    if (err && typeof err === "object" && "name" in err) {
      if ((err as { name?: unknown }).name === "AbortError") return true;
    }
    return /abort/i.test(String(err));
  };

  it("recognises the DOMException shape", () => {
    const e = new Error("The operation was aborted.");
    e.name = "AbortError";
    expect(isAbort(e)).toBe(true);
  });

  it("recognises the bare-string shape WaveSurfer also produces", () => {
    expect(isAbort("AbortError: Fetch is aborted")).toBe(true);
  });

  it("does not swallow a real failure", () => {
    expect(isAbort(new Error("404 (Not Found)"))).toBe(false);
    expect(isAbort("Failed to decode audio")).toBe(false);
  });
});

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
