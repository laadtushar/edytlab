/**
 * Undo works with a slider focused, and still leaves text fields alone.
 *
 * A fader is the control a person nudges and then wants to take back.
 * `isTyping` counted every `<input>` as text entry, a range slider
 * included, so with one focused Ctrl+Z did nothing until focus moved.
 */

import { fixturePath } from "./audio-fixtures";
import { nodeId, ok, projectWith, sessionNode, toneTrack } from "./backend";
import { expect, test } from "./fixtures";

const PARENT = nodeId(1);
const HEAD = nodeId(2);

async function bootWithTrack(app: Parameters<Parameters<typeof test>[2]>[0]["app"]) {
  await app.boot({
    ...projectWith([toneTrack()], HEAD),
    render_preview: ok(fixturePath("tone3s")),
    get_node: ok(sessionNode(HEAD, PARENT)),
    set_head_to: ok(PARENT),
  });
  await expect(app.page.getByTestId("ruler")).toContainText("0:03");
}

test("Ctrl+Z undoes with the gain slider focused", async ({ app }) => {
  await bootWithTrack(app);
  const page = app.page;

  await page.getByTestId("timeline-lane-gain").focus();
  await expect(page.getByTestId("timeline-lane-gain")).toBeFocused();
  await page.keyboard.press("Control+z");

  await expect.poll(() => app.requestsFor("set_head_to")).toEqual([{ nodeId: PARENT }]);
});

test("Ctrl+Z is left to a text field, which has its own undo", async ({ app }) => {
  await bootWithTrack(app);
  const page = app.page;

  const box = page.getByTestId("chat-form").locator("textarea");
  await box.focus();
  await expect(box).toBeFocused();
  await page.keyboard.press("Control+z");
  await app.settle();

  expect(await app.requestsFor("set_head_to")).toEqual([]);
});

test("Ctrl+Z on a focused slider does not write the stale value back", async ({ app }) => {
  // The key-up of the chord reaches the slider before the track list is
  // refreshed, carrying the value it showed. That is the session's own
  // value, so it must not be committed as a new edit.
  await bootWithTrack(app);
  const page = app.page;

  await page.getByTestId("timeline-lane-gain").focus();
  await page.keyboard.press("Control+z");
  await expect.poll(() => app.requestsFor("set_head_to")).toEqual([{ nodeId: PARENT }]);
  await app.settle();

  expect(await app.requestsFor("set_track_gain")).toEqual([]);
});

test("a slider moved by a key is written once, with its new value", async ({ app }) => {
  await app.boot({
    ...projectWith([toneTrack()], HEAD),
    render_preview: ok(fixturePath("tone3s")),
    set_track_gain: ok(nodeId(3)),
  });
  const page = app.page;
  await expect(page.getByTestId("ruler")).toContainText("0:03");

  await page.getByTestId("timeline-lane-gain").focus();
  await page.keyboard.press("ArrowRight"); // +0.5 dB
  await expect.poll(() => app.requestsFor("set_track_gain")).toEqual([{ track: 0, gainDb: 0.5 }]);

  // The fake session still answers `list_tracks` with 0 dB, so after the
  // refresh the slider shows the session's value again, and leaving it
  // (a blur commit) is not an edit.
  await page.getByTestId("timeline-lane-name").first().click();
  await app.settle();
  expect(await app.requestsFor("set_track_gain")).toHaveLength(1);
});
