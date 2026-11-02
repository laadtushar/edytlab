/**
 * Next's instrumentation hook, for Sentry on the server (#958), errors only.
 * See `lib/sentry.ts`.
 *
 * It reports through `@sentry/node` in the Node.js runtime and does nothing
 * in the edge runtime. With no `NEXT_PUBLIC_SENTRY_DSN` it loads nothing and
 * reports nothing. `process.env.NEXT_RUNTIME` is replaced when each runtime's
 * bundle is built, so the edge bundle holds no import of the SDK.
 */

import type { Instrumentation } from "next";

import { SENTRY_DSN, sentryOptions } from "./lib/sentry";

export async function register() {
  if (!SENTRY_DSN || process.env.NEXT_RUNTIME !== "nodejs") return;
  const Sentry = await import("@sentry/node");
  Sentry.init(sentryOptions(SENTRY_DSN));
}

/**
 * Errors from server components, route handlers and the proxy, which Next
 * catches and so never reach the process's own error handlers. The report
 * carries the page's path (`scrubEvent` takes its query string off, as it does
 * in the browser) and the route and its kind, and nothing else about the
 * request.
 */
export const onRequestError: Instrumentation.onRequestError = async (error, request, context) => {
  if (!SENTRY_DSN || process.env.NEXT_RUNTIME !== "nodejs") return;
  const Sentry = await import("@sentry/node");
  Sentry.withScope((scope) => {
    scope.setContext("nextjs", {
      router_kind: context.routerKind,
      route_path: context.routePath,
      route_type: context.routeType,
    });
    scope.addEventProcessor((event) => ({ ...event, request: { url: request.path } }));
    Sentry.captureException(error, {
      mechanism: { type: "auto.function.nextjs.on_request_error", handled: false },
    });
  });
};
