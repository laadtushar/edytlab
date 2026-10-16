/**
 * Where Settings keeps Save, and what scrolls (the dialog must fit the window).
 *
 * The card is a column capped to the window: header, tab strip, a body
 * that scrolls, and a footer that does not. jsdom has no layout, so it
 * cannot say whether the card fits, or where Save ends up in a 775 px
 * window; `e2e/settings-fits-window.spec.ts` checks that in Chromium. What
 * it can check is the structure that the layout depends on: Save lives
 * outside the scrolling body, after it in the Tab order, with the message
 * a failed save leaves; and the one body, shared by every tab, is back at
 * its top when the tab changes.
 */

import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../lib/tauri-bridge", () => ({
  getActiveProvider: vi.fn(),
  getActiveModel: vi.fn(),
  getBaseUrlFor: vi.fn(),
  defaultBaseUrlFor: vi.fn(),
  getEffortFor: vi.fn(),
  setEffortFor: vi.fn(),
  listModelsFor: vi.fn(),
  hasApiKeyFor: vi.fn(),
  clearApiKeyFor: vi.fn(),
  setApiKeyFor: vi.fn(),
  setActiveProvider: vi.fn(),
  setActiveModel: vi.fn(),
  setBaseUrlFor: vi.fn(),
  testApiKeyFor: vi.fn(),
  installPlugin: vi.fn(),
  mlRuntimeStatus: vi.fn(),
}));

import {
  defaultBaseUrlFor,
  getActiveModel,
  getActiveProvider,
  getBaseUrlFor,
  getEffortFor,
  mlRuntimeStatus,
  listModelsFor,
  setActiveModel,
  setApiKeyFor,
  setBaseUrlFor,
  setEffortFor,
} from "../lib/tauri-bridge";
import { Settings } from "../components/Settings";

async function renderSettings(mode: "blocking" | "panel") {
  render(
    <Settings
      mode={mode}
      onSaved={() => {}}
      onClose={() => {}}
      onCleared={() => {}}
    />,
  );
  // Let the mount-time reads land inside act, so nothing updates the
  // component behind a test's back.
  await waitFor(() => expect(getBaseUrlFor).toHaveBeenCalled());
  await act(async () => {});
}

/** Whether `a` comes before `b` in the document, which is the Tab order. */
function before(a: HTMLElement, b: HTMLElement): boolean {
  return Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);
}

describe("Settings layout", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.localStorage.clear();
    vi.mocked(getActiveProvider).mockResolvedValue("anthropic");
    vi.mocked(getActiveModel).mockResolvedValue("");
    vi.mocked(getBaseUrlFor).mockResolvedValue(null);
    vi.mocked(getEffortFor).mockResolvedValue(null);
    vi.mocked(defaultBaseUrlFor).mockResolvedValue("https://api.anthropic.com");
    // The panel's foot line (#383): no runtime in this build.
    vi.mocked(mlRuntimeStatus).mockResolvedValue({ state: "missing", searched: [] });
    vi.mocked(listModelsFor).mockResolvedValue({
      models: [],
      source: "live",
      notice: null,
    });
    // Saves answer where they landed (#497); these all went to the
    // durable store.
    const durable = { persistent: true, reason: null };
    vi.mocked(setEffortFor).mockResolvedValue(durable);
    vi.mocked(setBaseUrlFor).mockResolvedValue(durable);
    vi.mocked(setActiveModel).mockResolvedValue(durable);
    vi.mocked(setApiKeyFor).mockResolvedValue(durable);
  });

  it.each(["blocking", "panel"] as const)(
    "%s: Save is outside the scrolling body, after every field in it",
    async (mode) => {
      await renderSettings(mode);

      const body = screen.getByTestId("settings-body");
      const footer = screen.getByTestId("settings-footer");
      const save = screen.getByTestId("settings-save-button");

      expect(footer.contains(save)).toBe(true);
      expect(body.contains(footer)).toBe(false);
      expect(body.contains(save)).toBe(false);
      // Tab reaches the footer after the last field of the form.
      expect(before(body, footer)).toBe(true);
      expect(before(screen.getByTestId("settings-effort-select"), save)).toBe(true);
      expect(body.contains(screen.getByTestId("settings-effort-select"))).toBe(true);
    },
  );

  it.each(["blocking", "panel"] as const)(
    "%s: enters like the other overlays, and its scrim has no blur to animate",
    async (mode) => {
      // The scrim's opacity is animated (`backdrop-in`); a backdrop blur
      // on a layer whose opacity animates makes WebKitGTK re-blur the
      // window every frame, so there is none. It also stopped using
      // `app-fade-in`, which translated the full-screen scrim by 4px.
      await renderSettings(mode);
      const dialog = screen.getByTestId("settings");
      expect(dialog.className).toContain("backdrop-in");
      expect(dialog.className).not.toContain("backdrop-blur");
      expect(dialog.className).not.toContain("app-fade-in");
      expect(screen.getByTestId("settings-card").className).toContain("overlay-in");
    },
  );

  it("fades each tab's content in as a fresh element, and only the content", async () => {
    await renderSettings("panel");
    const body = screen.getByTestId("settings-body");
    const first = body.firstElementChild as HTMLElement;
    expect(first.className).toContain("fade-in-quick");
    // Content only: the strip and the body stay where they are.
    expect(screen.getByTestId("settings-tabs").className).not.toContain("fade-in");
    expect(body.className).not.toContain("fade-in");

    await userEvent.click(screen.getByTestId("settings-tab-plugins"));
    const second = screen.getByTestId("settings-body").firstElementChild as HTMLElement;
    expect(second).not.toBe(first);
    expect(second.className).toContain("fade-in-quick");
    expect(second.contains(screen.getByTestId("plugins-panel"))).toBe(true);
  });

  it("panel: Clear key is in the footer with Save", async () => {
    await renderSettings("panel");

    const footer = screen.getByTestId("settings-footer");
    expect(footer.contains(screen.getByTestId("settings-clear-button"))).toBe(true);
    expect(screen.getByTestId("settings-body").contains(
      screen.getByTestId("settings-clear-button"),
    )).toBe(false);
  });

  it("a failed save says so in the footer, beside the button that was clicked", async () => {
    vi.mocked(setApiKeyFor).mockRejectedValue("the keychain refused the key");
    await renderSettings("blocking");

    await userEvent.type(screen.getByTestId("settings-key-input"), "sk-ant-test");
    await userEvent.click(screen.getByTestId("settings-save-button"));

    const error = await screen.findByTestId("settings-save-error");
    expect(error).toHaveTextContent("the keychain refused the key");
    expect(screen.getByTestId("settings-footer").contains(error)).toBe(true);
    expect(screen.getByTestId("settings-body").contains(error)).toBe(false);
  });

  it("the footer belongs to the Account tab, and a tab opens at the top of the body", async () => {
    await renderSettings("panel");
    const body = screen.getByTestId("settings-body");
    expect(screen.getByTestId("settings-footer")).toBeInTheDocument();

    // jsdom keeps whatever is assigned: the body as scrolled down a tall tab.
    body.scrollTop = 240;
    await userEvent.click(screen.getByTestId("settings-tab-plugins"));

    expect(screen.getByTestId("plugins-panel")).toBeInTheDocument();
    expect(screen.queryByTestId("settings-footer")).toBeNull();
    // The same element, reset: not a fresh one that happens to start at 0.
    expect(screen.getByTestId("settings-body")).toBe(body);
    expect(body.scrollTop).toBe(0);
  });
});
