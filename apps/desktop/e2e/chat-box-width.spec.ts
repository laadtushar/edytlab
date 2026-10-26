/**
 * The message box gets most of the assistant pane (#738).
 *
 * Plan and Tools used to sit in one row with the box and the send button,
 * which left the box about 100 px at the default window size: the hint
 * wrapped over three lines and a typed sentence after two or three words.
 * jsdom has no layout, so this is measured in Chromium at the app's default
 * window size (`tauri.conf.json`: 1280 x 800).
 */

import { fixturePath } from "./audio-fixtures";
import { nodeId, ok, projectWith, toneTrack } from "./backend";
import { expect, test } from "./fixtures";

test.use({ viewport: { width: 1280, height: 800 } });

test("the message box is at least 60% of the chat pane, with Plan and Tools above it", async ({ app }) => {
  await app.boot({
    ...projectWith([toneTrack()], nodeId(2)),
    render_preview: ok(fixturePath("tone3s")),
  });
  const page = app.page;
  await expect(page.getByTestId("ruler")).toContainText("0:03");

  const pane = await page.getByTestId("chat-root").boundingBox();
  const box = await page.getByLabel("Message", { exact: true }).boundingBox();
  const plan = await page.getByTestId("plan-first-toggle").boundingBox();
  const tools = await page.getByTestId("capabilities-toggle").boundingBox();
  expect(pane && box && plan && tools).toBeTruthy();

  expect(box!.width).toBeGreaterThanOrEqual(pane!.width * 0.6);
  // Their own row, above the box.
  expect(plan!.y + plan!.height).toBeLessThanOrEqual(box!.y);
  expect(tools!.y + tools!.height).toBeLessThanOrEqual(box!.y);
  // The `/` hint is still on one line: the box is one row tall when empty.
  await expect(page.getByLabel("Message", { exact: true })).toHaveAttribute("placeholder", /\/ for commands/);
  expect(box!.height).toBeLessThan(40);
});
