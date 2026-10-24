"use client";

import Link from "next/link";
import { Suspense, lazy, useEffect, useRef, useState } from "react";

import {
  OPEN_SETTINGS_EVENT,
  grantAnalyticsConsent,
  parseMeasurementId,
  readConsent,
  revokeAnalyticsConsent,
  setConsentDefaults,
  writeConsent,
  type ConsentChoice,
} from "@/lib/consent";

/**
 * The Google tag's component, fetched only once a visitor has accepted:
 * a visitor who has not never downloads it, and a build with no
 * measurement ID ships it as a chunk that no page asks for.
 */
const GoogleAnalytics = lazy(() =>
  import("@next/third-parties/google").then((m) => ({ default: m.GoogleAnalytics })),
);

/**
 * The cookie banner, and Google Analytics 4 behind it.
 *
 * Nothing from Google loads until the visitor presses Accept: before
 * that there is no request to googletagmanager.com and no `_ga` cookie.
 * Accept and Decline look the same and weigh the same. The choice is
 * kept in `localStorage`; the footer's "Cookie settings" link reopens
 * the banner so it can be withdrawn.
 *
 * `measurementId` is `NEXT_PUBLIC_GA_MEASUREMENT_ID`. Unset or not a
 * GA4 ID, the component renders nothing and does nothing. It renders
 * nothing on the server either, so the page is the same without
 * JavaScript and the banner never moves the layout: it is `fixed`.
 */
export function GoogleAnalyticsConsent({
  measurementId,
}: {
  measurementId?: string;
}) {
  const id = parseMeasurementId(measurementId);
  const [choice, setChoice] = useState<ConsentChoice | null>(null);
  const [ready, setReady] = useState(false);
  const [reopened, setReopened] = useState(false);
  const banner = useRef<HTMLElement>(null);

  // Read the stored choice once the page is on screen (never during the
  // server render or hydration, which would not match). The Consent Mode
  // commands are pushed here, and the tag is rendered only by the render
  // that follows this state change, so they precede every command the
  // tag itself runs: Google ignores a default that comes after `config`.
  useEffect(() => {
    if (!id) return;
    setConsentDefaults();
    const stored = readConsent();
    if (stored === "granted") grantAnalyticsConsent(id);
    /* eslint-disable react-hooks/set-state-in-effect -- browser storage is read after hydration, and the order above is the point */
    setChoice(stored);
    setReady(true);
    /* eslint-enable react-hooks/set-state-in-effect */
  }, [id]);

  useEffect(() => {
    if (!id) return;
    const reopen = () => setReopened(true);
    window.addEventListener(OPEN_SETTINGS_EVENT, reopen);
    return () => window.removeEventListener(OPEN_SETTINGS_EVENT, reopen);
  }, [id]);

  const visible = ready && (choice === null || reopened);

  // A visitor who asked for the banner by keyboard lands in it.
  useEffect(() => {
    if (visible && reopened) banner.current?.focus();
  }, [visible, reopened]);

  if (!id || !ready) return null;

  function decide(next: ConsentChoice) {
    if (!id) return;
    writeConsent(next);
    if (next === "granted") grantAnalyticsConsent(id);
    else revokeAnalyticsConsent(id);
    setChoice(next);
    setReopened(false);
  }

  const button =
    "inline-flex h-9 flex-1 items-center justify-center rounded-md border border-border bg-secondary px-4 text-sm font-medium text-secondary-foreground transition-colors hover:bg-accent hover:text-accent-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring";

  return (
    <>
      {choice === "granted" ? (
        <Suspense fallback={null}>
          <GoogleAnalytics gaId={id} />
        </Suspense>
      ) : null}
      {visible ? (
        <section
          ref={banner}
          tabIndex={-1}
          role="region"
          aria-label="Cookie settings"
          data-cookie-banner
          className="fixed inset-x-4 bottom-4 z-50 rounded-lg border border-border bg-card p-4 text-sm text-card-foreground shadow-lg outline-none sm:left-auto sm:max-w-sm"
        >
          <p className="font-medium">Analytics cookies</p>
          <p className="mt-1 text-muted-foreground">
            May we use Google Analytics to count visits to this website? It
            sets cookies and sends data to Google, and only if you accept. The
            desktop app sends no telemetry.{" "}
            <Link
              href="/privacy#telemetry-and-analytics"
              className="text-primary underline underline-offset-2"
            >
              Details
            </Link>
          </p>
          {choice ? (
            <p className="mt-1 text-xs text-muted-foreground">
              Your choice now: {choice === "granted" ? "accepted" : "declined"}.
            </p>
          ) : null}
          <div className="mt-3 flex gap-2">
            <button type="button" className={button} onClick={() => decide("granted")}>
              Accept
            </button>
            <button type="button" className={button} onClick={() => decide("denied")}>
              Decline
            </button>
          </div>
        </section>
      ) : null}
    </>
  );
}
