/**
 * Sentry on the server (#958), errors only. Loaded by `instrumentation.ts`
 * only when the build has a DSN; see `lib/sentry.ts`.
 */

import * as Sentry from "@sentry/nextjs";

import { SENTRY_DSN, sentryOptions } from "./lib/sentry";

if (SENTRY_DSN) Sentry.init(sentryOptions(SENTRY_DSN));
