import { defineConfig, devices } from "@playwright/test";

/**
 * Live tests: the real frontend in Chromium, built for production, with
 * the Rust backend replaced at the IPC boundary (see `e2e/harness.ts`).
 *
 * A port of its own rather than Tauri's 1420, so a `tauri dev` session
 * and a test run never collide.
 *
 * The server is never reused. It serves a bundle built when it started,
 * so reusing one left running from an earlier build would test stale
 * code and pass. The dev server could be reused because it always
 * serves current source; a preview server cannot.
 *
 * No retries. A test that passes on its second attempt has a cause, and
 * a retry is how that cause stays hidden.
 *
 * `@playwright/test` is pinned exactly (no `^`). Each Playwright release
 * drives one Chromium build, so upgrading it upgrades the browser under
 * test, and that should be a deliberate change, not a lockfile refresh.
 */
const PORT = 5199;
const E2E_VITE = "--config e2e/vite.config.ts";

export default defineConfig({
  testDir: "./e2e",
  globalSetup: "./e2e/global-setup.ts",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: 0,
  reporter: process.env.CI ? [["list"], ["html", { open: "never" }]] : "list",
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    trace: "retain-on-failure",
    // Motion is off for the whole suite, so no spec waits on an entrance
    // or races a 120ms transition for actionability, and a screenshot is
    // of the final state. The app honours the setting through one global
    // block in `styles.css`. The specs where motion is the subject opt
    // back in with `test.use({ contextOptions: { reducedMotion:
    // "no-preference" } })`: `motion.spec.ts` and `automation.spec.ts`.
    // (`reducedMotion` is a context option; Playwright Test has no
    // top-level `use.reducedMotion`.)
    contextOptions: { reducedMotion: "reduce" },
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    command:
      `pnpm exec vite build ${E2E_VITE} && ` +
      `pnpm exec vite preview ${E2E_VITE} --port ${PORT} --strictPort --host 127.0.0.1`,
    url: `http://127.0.0.1:${PORT}/e2e/harness.html`,
    reuseExistingServer: false,
    timeout: 180_000,
  },
});
