/**
 * Playwright config for `shots.spec.ts`: the app screenshots the LinkedIn
 * visuals are built from.
 *
 * Not part of CI. `apps/desktop/playwright.config.ts` (the one CI runs)
 * has `testDir: ./e2e`, so nothing here is picked up by it, and this file
 * runs only when `render.mjs --shots` (or the README's command) asks.
 *
 * It does what the CI config does, on a port of its own: build the
 * frontend with `e2e/vite.config.ts` (the production bundle, under the
 * shipped Content Security Policy), serve it with `vite preview`, and
 * let each test fake the IPC boundary. 5199 is the CI config's fixed
 * port, so a screenshot run never collides with a test run; set
 * SHOTS_PORT to move this one.
 *
 * A plain object, not `defineConfig(...)`: this folder has no
 * `node_modules`, so it cannot import `@playwright/test`. The spec
 * borrows `test` from `apps/desktop/e2e/fixtures.ts`, which can.
 */

import { fileURLToPath } from "node:url";

const here = fileURLToPath(new URL(".", import.meta.url));
const desktop = fileURLToPath(new URL("../../../apps/desktop/", import.meta.url));
const PORT = Number(process.env.SHOTS_PORT ?? 5391);
const E2E_VITE = "--config e2e/vite.config.ts";

export default {
  testDir: here,
  testMatch: "shots.spec.ts",
  outputDir: fileURLToPath(new URL("../../../apps/desktop/test-results/linkedin-shots/", import.meta.url)),
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: "list",
  timeout: 60_000,
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    // The window Tauri opens by default (`tauri.conf.json`: 1280 x 800),
    // drawn at twice the pixels so a crop of it stays sharp.
    viewport: { width: 1280, height: 800 },
    deviceScaleFactor: 2,
    // A screenshot is of the final state, as in the CI config.
    contextOptions: { reducedMotion: "reduce" },
  },
  projects: [{ name: "chromium", use: { browserName: "chromium" } }],
  webServer: {
    command:
      `pnpm exec vite build ${E2E_VITE} && ` +
      `pnpm exec vite preview ${E2E_VITE} --port ${PORT} --strictPort --host 127.0.0.1`,
    cwd: desktop,
    url: `http://127.0.0.1:${PORT}/e2e/harness.html`,
    reuseExistingServer: false,
    timeout: 180_000,
  },
};
