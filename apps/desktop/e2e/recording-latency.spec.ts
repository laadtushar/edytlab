/**
 * The recording latency row in Settings → Audio, in a real browser (#203).
 *
 * jsdom cannot see where the row is drawn or whether the tab strip still
 * reaches it. These run the real frontend in Chromium with only the IPC
 * boundary replaced. The rule under test: opening the tab asks which
 * devices are the defaults and plays nothing; one press of Measure is one
 * call; a refusal is shown in the row with its advice.
 */

import { readyToLoad, recordingLatencyCommands, recordingLatencyStatus } from "./backend";
import { expect, test, type App } from "./fixtures";

test.use({ viewport: { width: 1280, height: 800 } });

async function openAudioTab(app: App, extra = {}) {
  await app.boot({ ...readyToLoad(), ...recordingLatencyCommands(), ...extra });
  const page = app.page;
  await expect(page.getByTestId("empty-state")).toBeVisible();
  await page.getByTestId("open-settings-button").click();
  await expect(page.getByTestId("settings-card")).toBeVisible();
  await page.getByTestId("settings-tab-audio").click();
  await expect(page.getByTestId("recording-latency")).toBeVisible();
  return page;
}

test("opening the Audio tab names the devices, shows the setup line and plays nothing", async ({
  app,
}) => {
  const page = await openAudioTab(app);

  await expect(page.getByTestId("recording-latency-value")).toHaveText("Not measured yet");
  await expect(page.getByTestId("recording-latency-devices")).toContainText("Speakers: default");
  await expect(page.getByTestId("recording-latency-setup")).toContainText(
    "Put the speakers close to the microphone",
  );
  await expect(page.getByTestId("recording-latency-measure")).toHaveText("Measure");

  await app.settle();
  expect(await app.requestsFor("get_recording_latency")).toHaveLength(1);
  expect(await app.requestsFor("calibrate_recording_latency")).toEqual([]);
});

test("Measure is one call, and its answer is the value shown", async ({ app }) => {
  const page = await openAudioTab(app);

  await page.getByTestId("recording-latency-measure").click();
  await expect(page.getByTestId("recording-latency-value")).toContainText(
    "75.3 ms (steady to 0.1 ms)",
  );
  await expect(page.getByTestId("recording-latency-measure")).toHaveText("Measure again");
  expect(await app.requestsFor("calibrate_recording_latency")).toHaveLength(1);
});

test("a kept value is shown when the tab opens", async ({ app }) => {
  const page = await openAudioTab(app, {
    get_recording_latency: {
      ok: recordingLatencyStatus({
        latency_ms: 42.46,
        spread_ms: 0.31,
        measured_at: 1_800_000_000,
        source: "measured",
      }),
    },
  });
  await expect(page.getByTestId("recording-latency-value")).toContainText("42.5 ms");
  await expect(page.getByTestId("recording-latency-measure")).toHaveText("Measure again");
});

test("a refusal appears in the row with its advice, inside the dialog", async ({ app }) => {
  const page = await openAudioTab(app, recordingLatencyCommands("not_heard"));

  await page.getByTestId("recording-latency-measure").click();
  const refusal = page.getByTestId("recording-latency-refusal");
  await expect(refusal).toContainText("heard 2 of 5 test sweeps");
  await expect(refusal).toContainText("hold the headphones against the microphone");
  await expect(page.getByTestId("recording-latency-value")).toHaveText("Not measured yet");

  const card = await page.getByTestId("settings-card").boundingBox();
  const box = await refusal.boundingBox();
  expect(card && box).toBeTruthy();
  expect(box!.x).toBeGreaterThanOrEqual(card!.x);
  expect(box!.x + box!.width).toBeLessThanOrEqual(card!.x + card!.width + 0.5);
});
