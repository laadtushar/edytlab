/**
 * Error reports from this website to Sentry (#958).
 *
 * The DSN comes from `NEXT_PUBLIC_SENTRY_DSN`, a Vercel environment variable;
 * none is committed. Without one (or with a value that is not a DSN) Sentry is
 * never initialised, the browser never loads its code, and the privacy page
 * does not name it (`SENTRY_DSN` below decides all three).
 *
 * The browser reports through `@sentry/react` (`instrumentation-client.ts`
 * and `app/global-error.tsx`), the server through `@sentry/node`, in the
 * Node.js runtime only (`instrumentation.ts`). There is no Sentry build step:
 * nothing is uploaded to Sentry when the site is built.
 *
 * With a DSN, only errors are reported: no performance tracing, no session
 * replay, no release-health sessions, no logs or metrics, no breadcrumbs, and
 * no personal data. `sendDefaultPii` is off and every `dataCollection`
 * category is off too, because in this SDK version an explicit
 * `dataCollection` is what decides (`sendDefaultPii` is ignored beside it).
 * `scrubEvent` then takes the user, cookies, headers and the query string and
 * fragment of every URL out of whatever is left.
 *
 * This file holds no runtime import of the SDK, so reading it (in a test, or
 * in a page that only asks whether Sentry is on) costs nothing.
 */

import type { ErrorEvent, EventHint } from "@sentry/react";

/** A DSN as Sentry issues them: `https://<public key>@<host>/<project id>`. */
const DSN = /^https:\/\/[0-9a-f]{16,64}@[a-z0-9.-]+(?::\d+)?\/\d+$/i;

/** The DSN if `raw` is one, otherwise `undefined`. */
export function parseSentryDsn(raw: string | undefined): string | undefined {
  const dsn = raw?.trim();
  return dsn && DSN.test(dsn) ? dsn : undefined;
}

/**
 * The DSN this build was made with. `NEXT_PUBLIC_*` is replaced when the site
 * is built, so a build without the variable has no DSN and reports nothing.
 */
export const SENTRY_DSN = parseSentryDsn(process.env.NEXT_PUBLIC_SENTRY_DSN);

/**
 * The integrations a report-only setup leaves out of Sentry's defaults, in the
 * browser (`@sentry/react`) and on the server (`@sentry/node`). A name that a
 * runtime does not have is ignored there.
 */
export const DROPPED_INTEGRATIONS = [
  // Release health: a "session" sent on every page view, and on the server
  // one for the process (`Http` below also counts each request as one).
  "BrowserSession",
  "ProcessSession",
  // Performance tracing.
  "BrowserTracing",
  // Clicks, console lines, fetches and page changes before an error.
  "Breadcrumbs",
  // The server's breadcrumbs: console lines and child processes.
  "Console",
  "ChildProcess",
  // The server's request and fetch hooks: request sessions, spans,
  // breadcrumbs and the `sentry-trace` and `baggage` headers they add to the
  // site's own outgoing requests.
  "Http",
  "NodeFetch",
  // The values of a failing function's local variables, on the server.
  "LocalVariables",
  // Session replay, if a version of the SDK ever adds it by default.
  "Replay",
  "ReplayCanvas",
  // The feedback widget.
  "Feedback",
];

/** A URL without its query string and fragment. */
function withoutQuery(url: string): string {
  const end = url.search(/[?#]/);
  return end === -1 ? url : url.slice(0, end);
}

/**
 * The `beforeSend` of every runtime: what may leave for Sentry. The user,
 * cookies, headers, request bodies and query strings are removed; URLs keep
 * only their path.
 */
export function scrubEvent(event: ErrorEvent, _hint?: EventHint): ErrorEvent {
  delete event.user;
  delete event.breadcrumbs;
  delete event.server_name;
  if (event.request) {
    const url = event.request.url ? withoutQuery(event.request.url) : undefined;
    event.request = url ? { url } : {};
  }
  for (const exception of event.exception?.values ?? []) {
    for (const frame of exception.stacktrace?.frames ?? []) {
      if (frame.abs_path) frame.abs_path = withoutQuery(frame.abs_path);
      if (frame.filename) frame.filename = withoutQuery(frame.filename);
      delete frame.vars;
    }
  }
  return event;
}

/** The options the browser's and the server's `Sentry.init` are given. */
export function sentryOptions(dsn: string) {
  return {
    dsn,
    environment: process.env.NEXT_PUBLIC_VERCEL_ENV ?? process.env.VERCEL_ENV ?? process.env.NODE_ENV,
    // Errors only. There is deliberately no `tracesSampleRate`: with none
    // (and no `tracesSampler`) the SDK records no spans at all, where a rate
    // of 0 would still record them and send none.
    enableLogs: false,
    enableMetrics: false,
    sendClientReports: false,
    maxBreadcrumbs: 0,
    // No personal data. `dataCollection` decides in this version; each
    // category is off.
    sendDefaultPii: false,
    dataCollection: {
      userInfo: false,
      cookies: false,
      httpHeaders: false,
      httpBodies: [],
      queryParams: false,
      urlQueryParams: false,
      graphQL: { document: false, variables: false },
      genAI: { inputs: false, outputs: false },
      databaseQueryData: false,
      stackFrameVariables: false,
      frameContextLines: 0,
    },
    beforeSend: scrubEvent,
    // Sentry's default integrations, minus tracing, sessions, breadcrumbs,
    // request hooks, local variables, replay and feedback.
    integrations: <T extends { name: string }>(defaults: T[]) =>
      defaults.filter((integration) => !DROPPED_INTEGRATIONS.includes(integration.name)),
  };
}
