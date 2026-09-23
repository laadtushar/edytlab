import { defineConfig, devices } from "@playwright/test";

/**
 * Live tests: the real frontend in Chromium, with the Rust backend
 * replaced at the IPC boundary (see `e2e/harness.ts`).
 *
 * A port of its own rather than Tauri's 1420, so a `tauri dev` session
 * and a test run never collide.
 *
 * No retries. A test that passes on its second attempt has a cause, and
 * a retry is how that cause stays hidden.
 */
const PORT = 5199;

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
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    command: `pnpm exec vite --port ${PORT} --strictPort --host 127.0.0.1`,
    url: `http://127.0.0.1:${PORT}/e2e/harness.html`,
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
  },
});
