// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import {
  CONSENT_STORAGE_KEY,
  clearGoogleAnalyticsCookies,
  grantAnalyticsConsent,
  parseMeasurementId,
  readConsent,
  revokeAnalyticsConsent,
  setConsentDefaults,
  writeConsent,
} from "./consent";

type Layer = ArrayLike<unknown>[];
const layer = () => (window as unknown as { dataLayer: Layer }).dataLayer;

beforeEach(() => {
  window.localStorage.clear();
  (window as unknown as { dataLayer?: Layer }).dataLayer = undefined;
});
afterEach(() => {
  for (const c of document.cookie.split(";")) {
    const name = c.split("=")[0].trim();
    if (name) document.cookie = `${name}=; expires=Thu, 01 Jan 1970 00:00:00 GMT; path=/`;
  }
});

describe("parseMeasurementId", () => {
  it.each(["G-ABC123", "G-TEST000000", "G-ABCDEF123456", " G-ABC123 "])("accepts %j", (raw) => {
    expect(parseMeasurementId(raw)).toBe(raw.trim());
  });
  it.each([undefined, "", "  ", "G-ABC12", "G-ABCDEF1234567", "g-abc123", "UA-12345-1", "GTM-ABC123", "G-ABC 123", "G-ABC123; alert(1)"])(
    "rejects %j",
    (raw) => {
      expect(parseMeasurementId(raw)).toBeUndefined();
    },
  );
});

describe("the stored choice", () => {
  it("is null until one is made, and round-trips", () => {
    expect(readConsent()).toBeNull();
    writeConsent("granted");
    expect(readConsent()).toBe("granted");
    writeConsent("denied");
    expect(readConsent()).toBe("denied");
  });
  it("ignores a value it did not write", () => {
    window.localStorage.setItem(CONSENT_STORAGE_KEY, "yes");
    expect(readConsent()).toBeNull();
  });
});

describe("Consent Mode v2", () => {
  it("denies all four signals by default", () => {
    setConsentDefaults();
    expect(Array.from(layer()[0])).toEqual([
      "consent",
      "default",
      {
        ad_storage: "denied",
        ad_user_data: "denied",
        ad_personalization: "denied",
        analytics_storage: "denied",
      },
    ]);
  });
  it("pushes arguments objects, which is what the tag reads", () => {
    setConsentDefaults();
    expect(Array.isArray(layer()[0])).toBe(false);
  });
  it("grants analytics_storage only, and takes it back", () => {
    grantAnalyticsConsent("G-ABC123");
    revokeAnalyticsConsent("G-ABC123");
    expect(Array.from(layer()[0])).toEqual(["consent", "update", { analytics_storage: "granted" }]);
    expect(Array.from(layer()[1])).toEqual(["consent", "update", { analytics_storage: "denied" }]);
    expect((window as unknown as Record<string, unknown>)["ga-disable-G-ABC123"]).toBe(true);
  });
});

describe("clearGoogleAnalyticsCookies", () => {
  it("removes _ga cookies and leaves the others", () => {
    document.cookie = "_ga=GA1.1.1.1; path=/";
    document.cookie = "_ga_ABC123=GS2.1.s1; path=/";
    document.cookie = "theme=dark; path=/";
    clearGoogleAnalyticsCookies();
    expect(document.cookie).toBe("theme=dark");
  });
});
