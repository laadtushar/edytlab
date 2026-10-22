/**
 * The speech model row in Settings → Account, in a real browser (#383).
 *
 * jsdom cannot see where the row is drawn: that it fits in the dialog at
 * the smallest window, that the progress bar's fill is the share of its
 * track the bytes say, and that a click is what starts a download. These
 * run the real frontend in Chromium with only the IPC boundary replaced:
 * the backend answers the four model commands as `backend.ts` says, and
 * the spec raises `ml-model-progress` the way the download thread would.
 *
 * The rule under test, again: nothing downloads until the user presses
 * Download. Opening Settings asks for the status and makes no other call,
 * and `download_ml_model` is called once per press.
 */

import {
  readyToLoad,
  speechModelCommands,
  speechModelStatus,
  SPEECH_MODEL_BYTES,
} from "./backend";
import { expect, test, type App } from "./fixtures";

test.use({ viewport: { width: 1280, height: 800 } });

async function openSettings(app: App, extra = {}) {
  await app.boot({ ...readyToLoad(), ...speechModelCommands(), ...extra });
  const page = app.page;
  await expect(page.getByTestId("empty-state")).toBeVisible();
  await page.getByTestId("open-settings-button").click();
  await expect(page.getByTestId("settings-card")).toBeVisible();
  const row = page.getByTestId("settings-speech-model");
  await expect(row).toBeVisible();
  return { page, row, card: page.getByTestId("settings-card") };
}

const PROGRESS = "ml-model-progress";

test("opening Settings shows the model, its size and source, and downloads nothing", async ({
  app,
}) => {
  const { row, page } = await openSettings(app);

  await expect(row).toHaveAttribute("data-state", "not_downloaded");
  await expect(page.getByTestId("speech-model-about")).toHaveText(
    "Whisper base, 139 MB, from Hugging Face.",
  );
  await expect(page.getByTestId("speech-model-download")).toHaveText("Download (139 MB)");
  // Said in the row: not available yet, what it is for, the one request.
  const note = page.getByTestId("speech-model-note");
  await expect(note).toContainText("Transcription is not available yet");
  await expect(note).toContainText("does not turn anything on");
  await expect(note).toContainText("huggingface.co");

  await app.settle();
  expect(await app.requestsFor("ml_model_status")).toEqual([{ model: "whisper-base" }]);
  for (const cmd of ["download_ml_model", "cancel_ml_model_download", "remove_ml_model"]) {
    expect(await app.requestsFor(cmd), `${cmd} was called`).toEqual([]);
  }
});

test("Download starts it once, and the bar follows the events to Ready", async ({ app }) => {
  const { row, page } = await openSettings(app);

  await page.getByTestId("speech-model-download").click();
  await expect(row).toHaveAttribute("data-state", "downloading");
  expect(await app.requestsFor("download_ml_model")).toEqual([{ model: "whisper-base" }]);

  // Half-way: the fill is half of its track.
  await app.emit(PROGRESS, speechModelStatus({ state: "downloading", received: SPEECH_MODEL_BYTES / 2 }));
  const track = page.getByTestId("speech-model-progress");
  await expect(track).toHaveAttribute("aria-valuenow", "50");
  await expect(page.getByTestId("speech-model-status")).toHaveText("69.3 of 138.6 MB (50%)");
  const trackBox = await track.boundingBox();
  await expect
    .poll(async () => (await page.getByTestId("speech-model-progress-fill").boundingBox())?.width ?? 0)
    .toBeCloseTo((trackBox?.width ?? 0) / 2, -1);

  await app.emit(PROGRESS, speechModelStatus({ state: "ready" }));
  await expect(row).toHaveAttribute("data-state", "ready");
  await expect(page.getByTestId("speech-model-status")).toHaveText(
    "Ready: 139 MB on disk. Nothing uses it yet.",
  );
  await expect(page.getByTestId("speech-model-remove")).toBeVisible();
  // One press, one call.
  expect(await app.requestsFor("download_ml_model")).toHaveLength(1);
});

test("Cancel asks the backend to stop, and Download then resumes", async ({ app }) => {
  const { row, page } = await openSettings(app, {
    ml_model_status: { ok: speechModelStatus({ state: "downloading", received: 20_000_000 }) },
    // Cancel answers the state at the time of the call: still 20 MB in.
    ...speechModelCommands({ received: 20_000_000 }),
  });

  // A download already running when Settings opened is shown as it is.
  await expect(row).toHaveAttribute("data-state", "downloading");
  await expect(page.getByTestId("speech-model-progress")).toHaveAttribute("aria-valuenow", "14");

  await page.getByTestId("speech-model-cancel").click();
  expect(await app.requestsFor("cancel_ml_model_download")).toEqual([{ model: "whisper-base" }]);
  await expect(page.getByTestId("speech-model-cancel")).toHaveText("Cancelling…");
  await expect(page.getByTestId("speech-model-cancel")).toBeDisabled();
  // The answer to Cancel did not move the bar back to 0.
  await expect(page.getByTestId("speech-model-progress")).toHaveAttribute("aria-valuenow", "14");

  // The thread stopped; what it had received is kept.
  await app.emit(PROGRESS, speechModelStatus({ state: "not_downloaded", bytes: 20_500_000 }));
  await expect(row).toHaveAttribute("data-state", "not_downloaded");
  await expect(page.getByTestId("speech-model-status")).toContainText(
    "20.5 of 138.6 MB already downloaded",
  );
  await expect(page.getByTestId("speech-model-download")).toHaveText("Resume download");
  expect(await app.requestsFor("download_ml_model")).toEqual([]);
});

test("Remove asks first, and removes only when told yes", async ({ app }) => {
  const { row, page } = await openSettings(app, {
    ml_model_status: { ok: speechModelStatus({ state: "ready" }) },
  });
  await expect(row).toHaveAttribute("data-state", "ready");

  // No: nothing is removed.
  const asked: string[] = [];
  page.once("dialog", async (dialog) => {
    asked.push(dialog.message());
    await dialog.dismiss();
  });
  await page.getByTestId("speech-model-remove").click();
  await expect.poll(() => asked.length).toBe(1);
  expect(asked[0]).toContain("Remove the speech model");
  expect(asked[0]).toContain("139 MB");
  expect(await app.requestsFor("remove_ml_model")).toEqual([]);
  await expect(row).toHaveAttribute("data-state", "ready");

  // Yes: it goes.
  page.once("dialog", (dialog) => void dialog.accept());
  await page.getByTestId("speech-model-remove").click();
  await expect(row).toHaveAttribute("data-state", "not_downloaded");
  expect(await app.requestsFor("remove_ml_model")).toEqual([{ model: "whisper-base" }]);
  await expect(page.getByTestId("speech-model-download")).toBeVisible();
});

test("a failed download shows its reason and Retry starts it again", async ({ app }) => {
  const reason = "downloading https://huggingface.co/x failed (HTTP 503): Service Unavailable";
  const { row, page } = await openSettings(app, {
    ml_model_status: { ok: speechModelStatus({ state: "failed", reason }) },
  });

  await expect(row).toHaveAttribute("data-state", "failed");
  await expect(page.getByTestId("speech-model-status")).toContainText("The download failed:");
  await expect(page.getByTestId("speech-model-status")).toContainText("HTTP 503");

  await page.getByTestId("speech-model-retry").click();
  await expect(row).toHaveAttribute("data-state", "downloading");
  expect(await app.requestsFor("download_ml_model")).toEqual([{ model: "whisper-base" }]);
});

for (const size of [
  { label: "the default window", width: 1280, height: 775 },
  { label: "the smallest window", width: 1024, height: 640 },
] as const) {
  test(`the row fits inside the dialog in ${size.label}, in every state`, async ({ app }) => {
    const { row, page, card } = await openSettings(app);
    await page.setViewportSize({ width: size.width, height: size.height });

    const states = [
      speechModelStatus({ state: "not_downloaded" }),
      speechModelStatus({ state: "not_downloaded", bytes: 41_200_000 }),
      // Files as big as the model that were not verified.
      speechModelStatus({ state: "not_downloaded", bytes: SPEECH_MODEL_BYTES }),
      speechModelStatus({ state: "downloading", received: 41_200_000 }),
      speechModelStatus({ state: "ready" }),
      speechModelStatus({
        state: "failed",
        reason: "downloading https://huggingface.co/onnx-community/whisper-base_timestamped/resolve/608c49e61301901684bc36cac8f74b95ff6b5a8e/onnx/encoder_model.onnx failed (HTTP 503): Service Unavailable",
        bytes: 12_000_000,
      }),
    ];
    for (const status of states) {
      await app.emit(PROGRESS, status);
      await expect(row).toHaveAttribute("data-state", String(status.state));
      await row.scrollIntoViewIfNeeded();
      const rowBox = await row.boundingBox();
      const cardBox = await card.boundingBox();
      expect(rowBox && cardBox).toBeTruthy();
      expect(rowBox!.x).toBeGreaterThanOrEqual(cardBox!.x);
      expect(rowBox!.x + rowBox!.width).toBeLessThanOrEqual(cardBox!.x + cardBox!.width + 0.5);
      // No sideways scroll inside the row: nothing is clipped on the right.
      expect(
        await row.evaluate((el) => el.scrollWidth <= el.clientWidth + 1),
        `a ${String(status.state)} row is wider than the dialog`,
      ).toBe(true);
      // Its buttons can be reached: inside the card, not under the footer.
      for (const button of await row.getByRole("button").all()) {
        await button.scrollIntoViewIfNeeded();
        const b = await button.boundingBox();
        expect(b!.x + b!.width).toBeLessThanOrEqual(cardBox!.x + cardBox!.width + 0.5);
      }
    }
  });
}
