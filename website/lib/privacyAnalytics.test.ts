import { readFileSync } from "node:fs";
import { join } from "node:path";

import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { GoogleAnalyticsDisclosure } from "@/components/analytics/ga-disclosure";

const WEBSITE = join(__dirname, "..");
const read = (file: string) => readFileSync(join(WEBSITE, file), "utf8");

/**
 * Google Analytics is consent-first (`lib/consent.ts`) and described on
 * the privacy page only in a build that has a measurement ID. These
 * tests pin what that description says and that the desktop app is
 * still described as sending no telemetry.
 */
describe("the privacy page and Google Analytics", () => {
  const html = renderToStaticMarkup(GoogleAnalyticsDisclosure());
  const text = html.replace(/<[^>]+>/g, " ").replace(/\s+/g, " ");

  it("names Google Analytics and says it needs consent", () => {
    expect(text).toContain("Google Analytics");
    expect(text).toMatch(/only if you accept/i);
    expect(text).toMatch(/no request to Google/);
  });

  it("says what Google receives and how to withdraw", () => {
    expect(text).toContain("_ga");
    expect(text).toContain("IP address");
    expect(text).toContain("Cookie settings");
    expect(text).toContain("Decline");
  });

  it("says ads signals stay denied", () => {
    expect(text).toContain("analytics_storage");
    expect(text).toMatch(/advertising signals stay denied/);
  });

  it("says the desktop app contains no Google Analytics and sends no telemetry", () => {
    expect(text).toMatch(/desktop app is not part of this/);
    expect(text).toMatch(/still sends no telemetry/);
  });

  it("links Google's own privacy and data-sharing pages", () => {
    expect(html).toContain('href="https://policies.google.com/privacy"');
    expect(html).toContain('href="https://support.google.com/analytics/answer/1011397"');
  });

  it("is on the privacy page, behind the measurement ID", () => {
    const page = read("app/privacy/page.tsx");
    expect(page).toContain("GoogleAnalyticsDisclosure");
    expect(page).toMatch(/GA_MEASUREMENT_ID \? <GoogleAnalyticsDisclosure \/>/);
  });

  it("still says the desktop app sends no telemetry", () => {
    const page = read("app/privacy/page.tsx");
    expect(page).toMatch(/<strong>The desktop app<\/strong> does not send telemetry/);
    expect(page).toContain("the app sends no telemetry");
  });

  it("says it does not run Google Analytics only when the build does not", () => {
    const page = read("app/privacy/page.tsx");
    expect(page).toMatch(
      /GA_MEASUREMENT_ID\s*\?\s*"Apart from those two and the Google Analytics described below[^"]*"\s*:\s*"We do not run Google Analytics/,
    );
  });
});

describe("the script", () => {
  it("is mounted in the layout only through the consent component", () => {
    const layout = read("app/layout.tsx");
    expect(layout).toContain("<GoogleAnalyticsConsent");
    expect(layout).not.toContain("googletagmanager");
    expect(layout).not.toContain("@next/third-parties");
  });

  it("is loaded from one file, behind a granted choice", () => {
    const source = read("components/analytics/google-analytics-consent.tsx");
    expect(source).toMatch(/choice === "granted" \? \(\s*<Suspense fallback=\{null\}>\s*<GoogleAnalytics gaId=\{id\} \/>/);
    // Fetched on demand, never imported at the top of the file.
    expect(source).not.toMatch(/^import .* from "@next\/third-parties/m);
  });
});
