/**
 * Next's instrumentation hook, for Sentry (#958). With no
 * `NEXT_PUBLIC_SENTRY_DSN` it loads nothing and reports nothing; see
 * `lib/sentry.ts`.
 */

import type { Instrumentation } from "next";

import { SENTRY_DSN } from "./lib/sentry";

export async function register() {
  if (!SENTRY_DSN) return;
  if (process.env.NEXT_RUNTIME === "nodejs") await import("./sentry.server.config");
  if (process.env.NEXT_RUNTIME === "edge") await import("./sentry.edge.config");
}

/** Errors from server components, route handlers and the proxy. */
export const onRequestError: Instrumentation.onRequestError = async (...args) => {
  if (!SENTRY_DSN) return;
  const Sentry = await import("@sentry/nextjs");
  Sentry.captureRequestError(...args);
};
