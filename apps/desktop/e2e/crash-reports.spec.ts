/**
 * Send crash reports in Settings → Account (#958), in a real browser.
 *
 * The switch is off on a fresh install. In a build without a reporting
 * address (every build from source, and this one) it stays off and disabled
 * and the row says why; in a build with one, a click turns reporting on and
 * off through `set_crash_reports` and nothing else. Opening Settings only
 * reads the status.
 */

import { crashReportsCommands, readyToLoad } from "./backend";
import { expect, test, type App } from "./fixtures";

test.use({ viewport: { width: 1024, height: 640 } });

async function openSettings(app: App, backend = {}) {
  await app.boot({ ...readyToLoad(), ...backend });
  const page = app.page;
  await expect(page.getByTestId("empty-state")).toBeVisible();
  await page.getByTestId("open-settings-button").click();
  await expect(page.getByTestId("settings-card")).toBeVisible();
  return page;
}

test("a build that cannot send keeps the switch off and says why", async ({ app }) => {
  const page = await openSettings(app);
  const row = page.getByTestId("settings-crash-reports");
  await row.scrollIntoViewIfNeeded();
  await expect(row).toBeVisible();
  await expect(page.getByTestId("settings-crash-reports-unavailable")).toBeVisible();
  const toggle = page.getByTestId("settings-crash-reports-toggle");
  await expect(toggle).not.toBeChecked();
  await expect(toggle).toBeDisabled();
  await app.settle();
  expect(await app.requestsFor("get_crash_reports")).toEqual([{}]);
  expect(await app.requestsFor("set_crash_reports")).toEqual([]);
});

test("a build that can send starts off, and a click turns reporting on and off", async ({ app }) => {
  const page = await openSettings(app, crashReportsCommands());
  const toggle = page.getByTestId("settings-crash-reports-toggle");
  await toggle.scrollIntoViewIfNeeded();
  await expect(toggle).toBeEnabled();
  await expect(toggle).not.toBeChecked();
  await expect(page.getByTestId("settings-crash-reports-unavailable")).toHaveCount(0);

  await toggle.click();
  await expect(toggle).toBeChecked();
  await toggle.click();
  await expect(toggle).not.toBeChecked();
  await app.settle();
  expect(await app.requestsFor("set_crash_reports")).toEqual([{ enabled: true }, { enabled: false }]);
});
