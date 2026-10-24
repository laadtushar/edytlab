/**
 * Consent for Google Analytics 4 on the website.
 *
 * Nothing from Google loads until a visitor accepts. The measurement ID
 * comes from `NEXT_PUBLIC_GA_MEASUREMENT_ID`; when it is unset or not a
 * GA4 ID, the banner, the footer link and the script all render nothing
 * and the site behaves as it did before Google Analytics. The desktop app
 * is not part of this: it sends no telemetry.
 *
 * This file holds the parts that need no React: the ID check, the stored
 * choice, the Consent Mode v2 commands and the cookie clean-up. The
 * component is `components/analytics/google-analytics-consent.tsx`.
 */

/** A GA4 measurement ID: `G-` and 6 to 12 capitals or digits. */
const MEASUREMENT_ID = /^G-[A-Z0-9]{6,12}$/;

/** The ID if it is a valid GA4 measurement ID, otherwise `undefined`. */
export function parseMeasurementId(raw: string | undefined): string | undefined {
  const id = raw?.trim();
  return id && MEASUREMENT_ID.test(id) ? id : undefined;
}

/**
 * The ID this build was made with. `NEXT_PUBLIC_*` is replaced when the
 * site is built, so a build without the variable has no trace of it.
 */
export const GA_MEASUREMENT_ID = parseMeasurementId(
  process.env.NEXT_PUBLIC_GA_MEASUREMENT_ID,
);

export type ConsentChoice = "granted" | "denied";

/** The `localStorage` key that remembers the visitor's choice. */
export const CONSENT_STORAGE_KEY = "edytlab-analytics-consent";

/** Dispatched on `window` by the footer link to reopen the banner. */
export const OPEN_SETTINGS_EVENT = "edytlab:open-cookie-settings";

/** The stored choice, or `null` when there is none or storage is blocked. */
export function readConsent(): ConsentChoice | null {
  try {
    const value = window.localStorage.getItem(CONSENT_STORAGE_KEY);
    return value === "granted" || value === "denied" ? value : null;
  } catch {
    return null;
  }
}

/** Remember the choice. A blocked store loses it at the next visit, not now. */
export function writeConsent(choice: ConsentChoice): void {
  try {
    window.localStorage.setItem(CONSENT_STORAGE_KEY, choice);
  } catch {
    // Private window or blocked site data: the banner asks again next time.
  }
}

type Gtag = (...args: unknown[]) => void;
type GaWindow = Window & {
  dataLayer?: unknown[];
  [flag: `ga-disable-${string}`]: boolean | undefined;
};

/**
 * `gtag()` as Google defines it. The tag reads `arguments` objects from
 * the data layer, not arrays, so this cannot be a rest-parameter push.
 */
const gtag: Gtag = function gtag() {
  const w = window as GaWindow;
  w.dataLayer = w.dataLayer || [];
  w.dataLayer.push(arguments);
};

/**
 * Consent Mode v2 defaults: everything denied. Called before the tag
 * loads, so it has to come before any `config`. Pushing to the data layer
 * makes no request.
 */
export function setConsentDefaults(): void {
  gtag("consent", "default", {
    ad_storage: "denied",
    ad_user_data: "denied",
    ad_personalization: "denied",
    analytics_storage: "denied",
  });
}

/** Accept: `analytics_storage` only. The three ads signals stay denied. */
export function grantAnalyticsConsent(id: string): void {
  (window as GaWindow)[`ga-disable-${id}`] = false;
  gtag("consent", "update", { analytics_storage: "granted" });
}

/**
 * Withdraw. A tag that is already on the page cannot be unloaded, so
 * this tells it to stop (Consent Mode and Google's `ga-disable` switch)
 * and removes the `_ga*` cookies; the next page load does not fetch it.
 */
export function revokeAnalyticsConsent(id: string): void {
  (window as GaWindow)[`ga-disable-${id}`] = true;
  gtag("consent", "update", { analytics_storage: "denied" });
  clearGoogleAnalyticsCookies();
}

/** Cookie names the Google tag sets: `_ga` and `_ga_<container id>`. */
const GA_COOKIE = /^(?:_ga|_ga_[A-Z0-9]+|_gid|_gat(?:_.*)?)$/;

/** The host and each parent domain the tag may have used, never a bare top-level domain (`www.a.com` -> `www.a.com`, `a.com`). */
function cookieDomains(hostname: string): (string | undefined)[] {
  const labels = hostname.split(".");
  const out: (string | undefined)[] = [undefined, hostname];
  for (let i = 1; i < labels.length - 1; i++) {
    out.push(labels.slice(i).join("."), `.${labels.slice(i).join(".")}`);
  }
  return out;
}

/**
 * Expire every `_ga*` cookie. Google's tag sets them on the highest
 * domain it can ("Cookies are set on the highest level of domain
 * possible"), so `www.example.com` gets `example.com`; this tries the
 * host and each parent that is not a bare top-level domain.
 */
export function clearGoogleAnalyticsCookies(): void {
  try {
    const names = document.cookie
      .split(";")
      .map((part) => part.split("=")[0].trim())
      .filter((name) => GA_COOKIE.test(name));
    for (const name of names) {
      for (const domain of cookieDomains(window.location.hostname)) {
        document.cookie =
          `${name}=; expires=Thu, 01 Jan 1970 00:00:00 GMT; path=/` +
          (domain ? `; domain=${domain}` : "");
      }
    }
  } catch {
    // Cookies blocked: there are none of ours to remove.
  }
}
