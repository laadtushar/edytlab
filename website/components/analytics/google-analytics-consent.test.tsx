// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { CONSENT_STORAGE_KEY } from "@/lib/consent";

import { CookieSettingsLink } from "./cookie-settings-link";
import { GoogleAnalyticsConsent } from "./google-analytics-consent";

// The real component adds two `next/script` tags; what matters here is
// whether it is rendered, and with which ID.
vi.mock("@next/third-parties/google", () => ({
  GoogleAnalytics: ({ gaId }: { gaId: string }) => (
    <div data-testid="ga" data-ga-id={gaId} />
  ),
}));

const ID = "G-TEST000000";

beforeEach(() => {
  window.localStorage.clear();
  (window as unknown as { dataLayer?: unknown[] }).dataLayer = undefined;
});
afterEach(() => {
  cleanup();
  for (const c of document.cookie.split(";")) {
    const name = c.split("=")[0].trim();
    if (name) document.cookie = `${name}=; expires=Thu, 01 Jan 1970 00:00:00 GMT; path=/`;
  }
});

const banner = () => screen.queryByRole("region", { name: "Cookie settings" });
const ga = () => screen.queryByTestId("ga");
const loadedGa = () => screen.findByTestId("ga");
const stored = () => window.localStorage.getItem(CONSENT_STORAGE_KEY);
const googleCommands = () =>
  ((window as unknown as { dataLayer?: ArrayLike<unknown>[] }).dataLayer ?? []).map((a) =>
    Array.from(a),
  );

describe("without a usable measurement ID", () => {
  it.each([undefined, "", "G-short", "UA-1234-5", "G-ABC123; x"])(
    "renders no banner and no script for %j",
    (id) => {
      const { container } = render(<GoogleAnalyticsConsent measurementId={id} />);
      expect(container.innerHTML).toBe("");
      expect(banner()).toBeNull();
      expect(ga()).toBeNull();
      expect(googleCommands()).toEqual([]);
    },
  );

  it("renders no footer link", () => {
    const { container } = render(<CookieSettingsLink id="" />);
    expect(container.innerHTML).toBe("");
  });
});

describe("with an ID and no stored choice", () => {
  it("shows the banner with Accept and Decline, and loads nothing", () => {
    render(<GoogleAnalyticsConsent measurementId={ID} />);
    expect(banner()).not.toBeNull();
    expect(screen.getByRole("button", { name: "Accept" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Decline" })).toBeTruthy();
    expect(ga()).toBeNull();
    expect(document.cookie).not.toContain("_ga");
  });

  it("gives Accept and Decline the same weight", () => {
    render(<GoogleAnalyticsConsent measurementId={ID} />);
    expect(screen.getByRole("button", { name: "Accept" }).className).toBe(
      screen.getByRole("button", { name: "Decline" }).className,
    );
  });

  it("sets Consent Mode defaults to denied before any choice", () => {
    render(<GoogleAnalyticsConsent measurementId={ID} />);
    expect(googleCommands()).toEqual([
      [
        "consent",
        "default",
        {
          ad_storage: "denied",
          ad_user_data: "denied",
          ad_personalization: "denied",
          analytics_storage: "denied",
        },
      ],
    ]);
  });

  it("Decline stores the choice, hides the banner and loads nothing", () => {
    render(<GoogleAnalyticsConsent measurementId={ID} />);
    fireEvent.click(screen.getByRole("button", { name: "Decline" }));
    expect(stored()).toBe("denied");
    expect(banner()).toBeNull();
    expect(ga()).toBeNull();
    expect(googleCommands().some((c) => c[2] && (c[2] as Record<string, string>).analytics_storage === "granted")).toBe(false);
  });

  it("Accept stores the choice, hides the banner and loads GA with the ID", async () => {
    render(<GoogleAnalyticsConsent measurementId={ID} />);
    fireEvent.click(screen.getByRole("button", { name: "Accept" }));
    expect(stored()).toBe("granted");
    expect(banner()).toBeNull();
    expect((await loadedGa()).getAttribute("data-ga-id")).toBe(ID);
  });

  it("Accept grants analytics_storage only, after the denied defaults", () => {
    render(<GoogleAnalyticsConsent measurementId={ID} />);
    fireEvent.click(screen.getByRole("button", { name: "Accept" }));
    const commands = googleCommands();
    expect(commands[0][1]).toBe("default");
    expect(commands[1]).toEqual(["consent", "update", { analytics_storage: "granted" }]);
    expect(commands).toHaveLength(2);
  });
});

describe("with a stored choice", () => {
  it("a stored Accept loads GA without showing the banner", async () => {
    window.localStorage.setItem(CONSENT_STORAGE_KEY, "granted");
    render(<GoogleAnalyticsConsent measurementId={ID} />);
    expect(banner()).toBeNull();
    expect((await loadedGa()).getAttribute("data-ga-id")).toBe(ID);
    const commands = googleCommands();
    expect(commands[0][1]).toBe("default");
    expect(commands[1]).toEqual(["consent", "update", { analytics_storage: "granted" }]);
  });

  it("a stored Decline shows no banner and loads nothing", () => {
    window.localStorage.setItem(CONSENT_STORAGE_KEY, "denied");
    render(<GoogleAnalyticsConsent measurementId={ID} />);
    expect(banner()).toBeNull();
    expect(ga()).toBeNull();
  });
});

describe("withdrawing", () => {
  it("the footer link reopens the banner, and Decline stops GA and clears the cookies", async () => {
    window.localStorage.setItem(CONSENT_STORAGE_KEY, "granted");
    document.cookie = "_ga=GA1.1.1.1; path=/";
    document.cookie = `_ga_TEST000000=GS2.1.s1; path=/`;
    render(
      <>
        <GoogleAnalyticsConsent measurementId={ID} />
        <CookieSettingsLink id={ID} />
      </>,
    );
    await loadedGa();
    expect(banner()).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Cookie settings" }));
    expect(banner()).not.toBeNull();
    expect(banner()?.textContent).toContain("accepted");
    expect(document.activeElement).toBe(banner());

    fireEvent.click(screen.getByRole("button", { name: "Decline" }));
    expect(stored()).toBe("denied");
    expect(ga()).toBeNull();
    expect(banner()).toBeNull();
    expect(document.cookie).not.toContain("_ga");
    expect(googleCommands().at(-1)).toEqual(["consent", "update", { analytics_storage: "denied" }]);
    expect((window as unknown as Record<string, unknown>)[`ga-disable-${ID}`]).toBe(true);
  });

  it("a visitor who declined can still change their mind", async () => {
    window.localStorage.setItem(CONSENT_STORAGE_KEY, "denied");
    render(
      <>
        <GoogleAnalyticsConsent measurementId={ID} />
        <CookieSettingsLink id={ID} />
      </>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Cookie settings" }));
    fireEvent.click(screen.getByRole("button", { name: "Accept" }));
    expect(stored()).toBe("granted");
    await loadedGa();
  });
});

describe("storage that throws", () => {
  it("still asks, and still works for this visit", async () => {
    const get = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    const set = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    render(<GoogleAnalyticsConsent measurementId={ID} />);
    expect(banner()).not.toBeNull();
    act(() => {
      fireEvent.click(screen.getByRole("button", { name: "Accept" }));
    });
    await loadedGa();
    get.mockRestore();
    set.mockRestore();
  });
});
