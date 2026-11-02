/**
 * Sentry in the browser (#958), errors only, through `@sentry/react`. See
 * `lib/sentry.ts`.
 *
 * Without `NEXT_PUBLIC_SENTRY_DSN` nothing here runs: the SDK is not loaded
 * and no request is made. With it, the SDK is fetched as a chunk of its own
 * after the page starts, so it adds nothing to what the first paint waits
 * for; an error thrown before it arrives is not reported.
 */

import { SENTRY_DSN, sentryOptions } from "./lib/sentry";

if (SENTRY_DSN) {
  const dsn = SENTRY_DSN;
  void import("@sentry/react").then((Sentry) => Sentry.init(sentryOptions(dsn)));
}
