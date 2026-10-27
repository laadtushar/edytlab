/**
 * "Open source licences" in Settings → Account, in a real browser.
 *
 * jsdom has no layout, so it cannot see that the notices open in a panel the
 * dialog keeps inside its own box and scrolls, instead of growing past the
 * window. The backend answers `third_party_notices` as `backend.ts` says (a
 * stand-in for the generated file); the rule under test is that Settings
 * reads the file only when Show is pressed.
 */

import { readyToLoad } from "./backend";
import { expect, test, type App } from "./fixtures";

test.use({ viewport: { width: 1024, height: 640 } });

async function openSettings(app: App) {
  await app.boot({ ...readyToLoad() });
  const page = app.page;
  await expect(page.getByTestId("empty-state")).toBeVisible();
  await page.getByTestId("open-settings-button").click();
  await expect(page.getByTestId("settings-card")).toBeVisible();
  return page;
}

test("the notices are read when Show is pressed, scroll inside the dialog, and hide again", async ({
  app,
}) => {
  const page = await openSettings(app);
  const row = page.getByTestId("settings-licences");
  await row.scrollIntoViewIfNeeded();
  await expect(row).toBeVisible();

  await app.settle();
  expect(await app.requestsFor("third_party_notices")).toEqual([]);

  await page.getByTestId("settings-licences-toggle").click();
  const text = page.getByTestId("settings-licences-text");
  await expect(text).toContainText("THIRD-PARTY NOTICES");
  await expect(text).toContainText("SOURCE AVAILABILITY (MPL-2.0)");
  expect(await app.requestsFor("third_party_notices")).toEqual([{}]);

  // A panel of its own that scrolls: the text is taller than its box.
  const box = await text.evaluate((el) => ({
    client: el.clientHeight,
    scroll: el.scrollHeight,
    overflowY: getComputedStyle(el).overflowY,
  }));
  expect(box.overflowY).toBe("auto");
  expect(box.scroll).toBeGreaterThan(box.client);
  // And the dialog did not grow past the window to hold it.
  const card = await page.getByTestId("settings-card").boundingBox();
  expect(card).not.toBeNull();
  expect(card!.y + card!.height).toBeLessThanOrEqual(640);

  await page.getByTestId("settings-licences-toggle").click();
  await expect(text).toHaveCount(0);
});
