/**
 * Playwright config for the v0.7.0 promo recordings.
 *
 * Not part of the app's e2e suite: `apps/desktop/playwright.config.ts` has
 * `testDir: "./e2e"`, so nothing here is ever picked up by `test:e2e` or by CI.
 * Run it from `apps/desktop`:
 *
 *   pnpm exec playwright test -c ../../marketing/promo-v0.7.0/record/playwright.config.ts
 *
 * It serves the same production build of the frontend the e2e suite uses
 * (`e2e/vite.config.ts`: the real `src/main.tsx` with only the IPC boundary
 * replaced), on a port of its own (`PROMO_PORT`, default 5311) so it never
 * collides with a test run on 5199.
 *
 * A plain object, not `defineConfig(...)`: this file sits outside the app's
 * package, where `@playwright/test` does not resolve, and a config needs no
 * import. The shots import everything else through `apps/desktop/e2e`.
 */

import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const DESKTOP = resolve(HERE, "../../../apps/desktop");
const PORT = Number(process.env.PROMO_PORT ?? 5311);
const E2E_VITE = "--config e2e/vite.config.ts";

export default {
  testDir: HERE,
  // Writes the e2e suite's generated fixtures, which the preview server serves.
  globalSetup: resolve(DESKTOP, "e2e/global-setup.ts"),
  testMatch: /.*\.shot\.ts$/,
  // One shot at a time: each is a real-time recording, and a second
  // Chromium rendering beside it would cost it frames.
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: "list",
  // A shot runs at a human pace; the longest is well under this.
  timeout: 240_000,
  outputDir: resolve(HERE, ".out"),
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    // Recording sets its own viewport and scale (see `record.ts`).
    contextOptions: { reducedMotion: "no-preference" },
  },
  projects: [{ name: "chromium", use: {} }],
  webServer: {
    command:
      `pnpm exec vite build ${E2E_VITE} && ` +
      `pnpm exec vite preview ${E2E_VITE} --port ${PORT} --strictPort --host 127.0.0.1`,
    cwd: DESKTOP,
    url: `http://127.0.0.1:${PORT}/e2e/harness.html`,
    reuseExistingServer: false,
    timeout: 180_000,
  },
};
