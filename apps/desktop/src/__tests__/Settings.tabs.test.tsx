/**
 * The Settings tab strip scrolls itself and never shifts the dialog (#392).
 *
 * Seven tabs did not fit in a 30rem dialog: Plugins sat past the right
 * edge of a card that is `overflow-hidden`, so it was clipped and could
 * not be clicked. Worse, the card is then the nearest scroll container,
 * so keyboard focus on Plugins (or any scroll-into-view) slid the whole
 * dialog sideways instead of bringing the tab into reach.
 *
 * The strip is now its own scroller. jsdom has no layout, so each test
 * stubs the strip's measurements and the tabs' rectangles with a model
 * of the layout: seven 60px tabs behind 12px of padding, scrolled by
 * `scrollLeft`. The real widths and the real clipping are checked in
 * Chromium by `e2e/settings-tabs.spec.ts`.
 */

import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
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
}));

import {
  clearApiKeyFor,
  defaultBaseUrlFor,
  getActiveModel,
  getActiveProvider,
  getBaseUrlFor,
  getEffortFor,
  hasApiKeyFor,
  installPlugin,
  listModelsFor,
  setActiveModel,
  setActiveProvider,
  setApiKeyFor,
  setBaseUrlFor,
  setEffortFor,
  testApiKeyFor,
} from "../lib/tauri-bridge";
import { Settings } from "../components/Settings";

const TAB_IDS = [
  "account",
  "project",
  "memory",
  "skills",
  "agents",
  "mcp",
  "plugins",
] as const;

/** Where the first tab starts: the strip's left padding. */
const PAD = 12;
const TAB_W = 60;

function rect(left: number, right: number, height: number) {
  return {
    left,
    right,
    top: 0,
    bottom: height,
    x: left,
    y: 0,
    width: right - left,
    height,
    toJSON: () => ({}),
  } as DOMRect;
}

/**
 * Give the strip the measurements a browser would, and the tabs the
 * rectangles that go with them. `scrollLeft` is backed by a variable so
 * the component can move it and the tabs' positions follow.
 */
function stubStrip(
  strip: HTMLElement,
  {
    clientWidth,
    scrollWidth,
    scrollLeft = 0,
  }: { clientWidth: number; scrollWidth: number; scrollLeft?: number },
) {
  let left = scrollLeft;
  Object.defineProperty(strip, "clientWidth", {
    configurable: true,
    get: () => clientWidth,
  });
  Object.defineProperty(strip, "scrollWidth", {
    configurable: true,
    get: () => scrollWidth,
  });
  Object.defineProperty(strip, "scrollLeft", {
    configurable: true,
    get: () => left,
    set: (v: number) => {
      left = v;
    },
  });
  strip.getBoundingClientRect = () => rect(0, clientWidth, 30);
  TAB_IDS.forEach((id, i) => {
    const tab = screen.getByTestId(`settings-tab-${id}`);
    tab.getBoundingClientRect = () => {
      const l = PAD + TAB_W * i - left;
      return rect(l, l + TAB_W, 30);
    };
  });
}

/** 7 tabs of 60px between 12px of padding: 444px, in a 300px strip. */
const OVERFLOWING = { clientWidth: 300, scrollWidth: 444 };
/** The most the overflowing strip can scroll: 444 - 300. */
const MAX_SCROLL = 144;

async function renderPanel() {
  render(
    <Settings
      mode="panel"
      onSaved={() => {}}
      onClose={() => {}}
      onCleared={() => {}}
    />,
  );
  // Let the mount-time reads land inside act, so nothing updates the
  // component behind a test's back.
  await waitFor(() => expect(getBaseUrlFor).toHaveBeenCalled());
  await act(async () => {});
  return screen.getByTestId("settings-tabs");
}

function flag(strip: HTMLElement, which: "start" | "end") {
  return strip.getAttribute(`data-overflow-${which}`);
}

describe("Settings tab strip", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.localStorage.clear();
    vi.mocked(getActiveProvider).mockResolvedValue("anthropic");
    vi.mocked(getActiveModel).mockResolvedValue("");
    vi.mocked(getBaseUrlFor).mockResolvedValue(null);
    vi.mocked(getEffortFor).mockResolvedValue(null);
    vi.mocked(listModelsFor).mockResolvedValue([]);
    vi.mocked(defaultBaseUrlFor).mockResolvedValue(
      "https://api.anthropic.com",
    );
    for (const fn of [
      setEffortFor,
      hasApiKeyFor,
      clearApiKeyFor,
      setApiKeyFor,
      setActiveProvider,
      setActiveModel,
      setBaseUrlFor,
      testApiKeyFor,
      installPlugin,
    ]) {
      vi.mocked(fn as (...args: never[]) => Promise<unknown>).mockResolvedValue(
        undefined,
      );
    }
  });

  it("reports whether more tabs lie past either end as the strip scrolls", async () => {
    const strip = await renderPanel();
    stubStrip(strip, OVERFLOWING);

    strip.scrollLeft = 0;
    fireEvent.scroll(strip);
    await waitFor(() => {
      expect(flag(strip, "start")).toBe("false");
      expect(flag(strip, "end")).toBe("true");
    });

    strip.scrollLeft = MAX_SCROLL;
    fireEvent.scroll(strip);
    await waitFor(() => {
      expect(flag(strip, "start")).toBe("true");
      expect(flag(strip, "end")).toBe("false");
    });

    strip.scrollLeft = 70;
    fireEvent.scroll(strip);
    await waitFor(() => {
      expect(flag(strip, "start")).toBe("true");
      expect(flag(strip, "end")).toBe("true");
    });
  });

  it("flags nothing when every tab fits", async () => {
    const strip = await renderPanel();
    stubStrip(strip, { clientWidth: 444, scrollWidth: 444 });

    fireEvent.scroll(strip);
    await waitFor(() => {
      expect(flag(strip, "start")).toBe("false");
      expect(flag(strip, "end")).toBe("false");
    });
  });

  it("scrolls the strip, not the dialog, as Tab walks to Plugins, and Enter opens it", async () => {
    const strip = await renderPanel();
    stubStrip(strip, OVERFLOWING);
    const account = screen.getByTestId("settings-tab-account");
    const plugins = screen.getByTestId("settings-tab-plugins");

    act(() => account.focus());
    const user = userEvent.setup();
    for (let i = 0; i < 6; i += 1) await user.tab();

    expect(plugins).toHaveFocus();
    // Agents scrolls it by 36, MCP by 60 more, and Plugins would need 60
    // more still but the strip ends at 144.
    expect(strip.scrollLeft).toBe(MAX_SCROLL);

    await user.keyboard("{Enter}");
    await waitFor(() => {
      expect(screen.getByTestId("plugins-panel")).toBeInTheDocument();
      expect(plugins).toHaveAttribute("aria-selected", "true");
    });
  });

  it("scrolls back when focus returns to a tab that is off the left edge", async () => {
    const strip = await renderPanel();
    stubStrip(strip, { ...OVERFLOWING, scrollLeft: MAX_SCROLL });
    const account = screen.getByTestId("settings-tab-account");

    act(() => account.focus());

    expect(strip.scrollLeft).toBe(0);
    await waitFor(() => {
      expect(flag(strip, "start")).toBe("false");
      expect(flag(strip, "end")).toBe("true");
    });
  });

  it("does not move the strip between pressing a tab and releasing it", async () => {
    const strip = await renderPanel();
    stubStrip(strip, OVERFLOWING);
    const plugins = screen.getByTestId("settings-tab-plugins");

    // Plugins starts half under the right-hand fade: at 0 its right edge
    // is 432 in a 300px strip. Pressing it focuses it, and if focus
    // scrolled the strip the tab would slide out from under the pointer
    // before the button came up. The release would then land on the
    // padding or the next tab, the click would go to their common
    // ancestor, and the tab would be focused but not selected.
    let atRelease = -1;
    plugins.addEventListener("mouseup", () => {
      atRelease = strip.scrollLeft;
    });

    const user = userEvent.setup();
    await user.click(plugins);

    expect(atRelease).toBe(0);
    // Once the click has happened it is safe to bring the tab into view.
    expect(strip.scrollLeft).toBe(MAX_SCROLL);
    await waitFor(() => {
      expect(plugins).toHaveAttribute("aria-selected", "true");
      expect(screen.getByTestId("plugins-panel")).toBeInTheDocument();
    });
  });

  it("has no strip at all in the blocking first-launch dialog", async () => {
    expect(() =>
      render(
        <Settings mode="blocking" onSaved={() => {}} onCleared={() => {}} />,
      ),
    ).not.toThrow();
    await waitFor(() => expect(getBaseUrlFor).toHaveBeenCalled());
    await act(async () => {});
    expect(screen.queryByTestId("settings-tabs")).toBeNull();
  });
});
