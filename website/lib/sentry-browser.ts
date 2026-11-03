/**
 * The part of `@sentry/react` the browser uses (#958): `init` for
 * `instrumentation-client.ts` and `captureException` for
 * `app/global-error.tsx`. Both load this file with a dynamic `import()`, and
 * only when the build has a DSN, so it is a chunk of its own that a build
 * without one never requests.
 *
 * The named imports are the point. A dynamic `import("@sentry/react")` loads
 * the package's whole namespace, which the bundler cannot trim, so the chunk
 * carried session replay, the feedback widget and tracing, none of which
 * `sentryOptions` turns on. Importing names here lets it drop the rest
 * (`@sentry/react` and its dependencies declare `sideEffects: false`).
 */

export { captureException, init } from "@sentry/react";
