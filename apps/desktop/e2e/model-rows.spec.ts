/**
 * The model rows in Settings → Account (the speech model, #383, the speaker
 * model, #168, and the stem model, #385), in a real browser.
 *
 * The rows are the same component with different words, so one table drives
 * the same behaviours for each: every test below runs once per entry of
 * `MODELS`, with the same assertions, and a check only one row has (the
 * speaker model's licence credit, say) is an `extra` of its entry.
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
  modelStatuses,
  readyToLoad,
  speakerModelCommands,
  speakerModelStatus,
  speechModelCommands,
  speechModelStatus,
  stemModelCommands,
  stemModelStatus,
  SPEAKER_MODEL_BYTES,
  SPEECH_MODEL_BYTES,
  STEM_MODEL_BYTES,
} from "./backend";
import { expect, test, type App } from "./fixtures";

test.use({ viewport: { width: 1280, height: 800 } });

type Status = Record<string, unknown>;
type StatusFields = Parameters<typeof speechModelStatus>[0];

interface ModelRow {
  /** Names the model in test titles. */
  name: string;
  /** The model id every command is called with. */
  id: string;
  /** The key `modelStatuses` takes this row's status under. */
  statusKey: "speech" | "speaker" | "stem";
  /** The row's id, and the prefix of the test ids inside it. */
  rowTestId: string;
  prefix: string;
  commands: (opts?: { received?: number }) => ReturnType<typeof speechModelCommands>;
  status: (fields: StatusFields) => Status;
  bytes: number;
  about: string;
  downloadText: string;
  /** What the note says: what it is for, the one request, what leaves. */
  noteParts: string[];
  /** The text asserted only for this row, run after the note's. */
  extra?: (page: App["page"]) => Promise<void>;
  halfText: string;
  readyText: string;
  /** `aria-valuenow` of the bar at 20,000,000 received bytes. */
  cancelPercent: string;
  partialText: string;
  /** The words of the Remove question, and the size it names. */
  removeSubject: string;
  removeSize: string;
  /** A failure reason as long as the model's real one (a pinned file URL). */
  longReason: string;
}

const MODELS: ModelRow[] = [
  {
    name: "speech model",
    id: "whisper-base",
    statusKey: "speech",
    rowTestId: "settings-speech-model",
    prefix: "speech-model",
    commands: speechModelCommands,
    status: speechModelStatus,
    bytes: SPEECH_MODEL_BYTES,
    about: "Whisper base, 139 MB, from Hugging Face.",
    downloadText: "Download (139 MB)",
    noteParts: [
      "runs on this computer",
      "once it is downloaded",
      "reach your AI provider",
      "huggingface.co",
    ],
    halfText: "69.3 of 138.6 MB (50%)",
    readyText: "Ready: 139 MB on disk. Ask the assistant to transcribe a track.",
    cancelPercent: "14",
    partialText: "20.5 of 138.6 MB already downloaded",
    removeSubject: "Remove the speech model",
    removeSize: "139 MB",
    longReason:
      "downloading https://huggingface.co/onnx-community/whisper-base_timestamped/resolve/608c49e61301901684bc36cac8f74b95ff6b5a8e/onnx/encoder_model.onnx failed (HTTP 503): Service Unavailable",
  },
  {
    name: "speaker model",
    id: "speaker-diarization",
    statusKey: "speaker",
    rowTestId: "settings-speaker-model",
    prefix: "speaker-model",
    commands: speakerModelCommands,
    status: speakerModelStatus,
    bytes: SPEAKER_MODEL_BYTES,
    about: "Speaker diarization (pyannote, WeSpeaker), 33 MB, from Hugging Face.",
    downloadText: "Download (33 MB)",
    noteParts: [
      "runs on this computer",
      "once it is downloaded",
      "two files",
      "reach your AI provider",
      "huggingface.co",
    ],
    extra: async (page) => {
      // The licences the two files ask to be credited.
      const licences = page.getByTestId("speaker-model-licences");
      await expect(licences).toContainText("MIT");
      await expect(licences).toContainText("CC BY 4.0");
    },
    halfText: "16.3 of 32.5 MB (50%)",
    readyText: "Ready: 33 MB on disk. Ask the assistant who speaks when on a track.",
    cancelPercent: "61",
    partialText: "20.5 of 32.5 MB already downloaded",
    removeSubject: "Remove the speaker model",
    removeSize: "33 MB",
    longReason:
      "downloading https://huggingface.co/Wespeaker/wespeaker-voxceleb-resnet34-LM/resolve/f0c48c298fd835726c27956a5d617bad7115627e/voxceleb_resnet34_LM.onnx failed (HTTP 503): Service Unavailable",
  },
  {
    name: "stem model",
    id: "htdemucs",
    statusKey: "stem",
    rowTestId: "settings-stem-model",
    prefix: "stem-model",
    commands: stemModelCommands,
    status: stemModelStatus,
    bytes: STEM_MODEL_BYTES,
    about: "Stem separation (HT-Demucs v4), 171 MB, from Hugging Face.",
    downloadText: "Download (171 MB)",
    noteParts: [
      "runs on this computer",
      "once it is downloaded",
      "4 GB of free memory",
      "two files",
      "huggingface.co",
    ],
    extra: async (page) => {
      // The licences the two files ask to be credited.
      const licences = page.getByTestId("stem-model-licences");
      await expect(licences).toContainText("MIT");
      await expect(licences).toContainText("Meta");
      await expect(licences).toContainText("Intel");
    },
    halfText: "85.4 of 170.7 MB (50%)",
    readyText: "Ready: 171 MB on disk. Ask the assistant to separate the stems of a track.",
    cancelPercent: "11",
    partialText: "20.5 of 170.7 MB already downloaded",
    removeSubject: "Remove the stem model",
    removeSize: "171 MB",
    longReason:
      "downloading https://huggingface.co/Intel/demucs-openvino/resolve/efb3d2bc54f93899b13b72b12c57001e19f09ae2/htdemucs_v4/htdemucs_fwd.onnx.data failed (HTTP 503): Service Unavailable",
  },
];

const PROGRESS = "ml-model-progress";

async function openSettings(m: ModelRow, app: App, extra = {}) {
  await app.boot({ ...readyToLoad(), ...m.commands(), ...extra });
  const page = app.page;
  await expect(page.getByTestId("empty-state")).toBeVisible();
  await page.getByTestId("open-settings-button").click();
  await expect(page.getByTestId("settings-card")).toBeVisible();
  const row = page.getByTestId(m.rowTestId);
  await expect(row).toBeVisible();
  return { page, row, card: page.getByTestId("settings-card") };
}

/** `modelStatuses` with only this row's status set. */
function startingAt(m: ModelRow, status: Status) {
  return modelStatuses({ [m.statusKey]: status });
}

for (const m of MODELS) {
  const id = (part: string) => `${m.prefix}-${part}`;

  test.describe(m.name, () => {
    test(`${m.name}: opening Settings shows the model, its size and source, and downloads nothing`, async ({
      app,
    }) => {
      const { row, page } = await openSettings(m, app);

      await expect(row).toHaveAttribute("data-state", "not_downloaded");
      await expect(page.getByTestId(id("about"))).toHaveText(m.about);
      await expect(page.getByTestId(id("download"))).toHaveText(m.downloadText);
      // Said in the row: what it is for, the one request, what leaves the computer.
      const note = page.getByTestId(id("note"));
      for (const part of m.noteParts) await expect(note).toContainText(part);
      await m.extra?.(page);

      await app.settle();
      // One status read for each row.
      expect(await app.requestsFor("ml_model_status")).toEqual(
        expect.arrayContaining([{ model: m.id }]),
      );
      expect(await app.requestsFor("ml_model_status")).toHaveLength(MODELS.length);
      for (const cmd of ["download_ml_model", "cancel_ml_model_download", "remove_ml_model"]) {
        expect(await app.requestsFor(cmd), `${cmd} was called`).toEqual([]);
      }
    });

    test(`${m.name}: Download starts it once, and the bar follows the events to Ready`, async ({
      app,
    }) => {
      const { row, page } = await openSettings(m, app);

      await page.getByTestId(id("download")).click();
      await expect(row).toHaveAttribute("data-state", "downloading");
      expect(await app.requestsFor("download_ml_model")).toEqual([{ model: m.id }]);

      // Half-way: the fill is half of its track.
      await app.emit(PROGRESS, m.status({ state: "downloading", received: m.bytes / 2 }));
      const track = page.getByTestId(id("progress"));
      await expect(track).toHaveAttribute("aria-valuenow", "50");
      await expect(page.getByTestId(id("status"))).toHaveText(m.halfText);
      const trackBox = await track.boundingBox();
      await expect
        .poll(async () => (await page.getByTestId(id("progress-fill")).boundingBox())?.width ?? 0)
        .toBeCloseTo((trackBox?.width ?? 0) / 2, -1);

      await app.emit(PROGRESS, m.status({ state: "ready" }));
      await expect(row).toHaveAttribute("data-state", "ready");
      await expect(page.getByTestId(id("status"))).toHaveText(m.readyText);
      await expect(page.getByTestId(id("remove"))).toBeVisible();
      // One press, one call.
      expect(await app.requestsFor("download_ml_model")).toHaveLength(1);
    });

    test(`${m.name}: Cancel asks the backend to stop, and Download then resumes`, async ({
      app,
    }) => {
      const { row, page } = await openSettings(m, app, {
        ...startingAt(m, m.status({ state: "downloading", received: 20_000_000 })),
        // Cancel answers the state at the time of the call: still 20 MB in.
        ...m.commands({ received: 20_000_000 }),
      });

      // A download already running when Settings opened is shown as it is.
      await expect(row).toHaveAttribute("data-state", "downloading");
      await expect(page.getByTestId(id("progress"))).toHaveAttribute(
        "aria-valuenow",
        m.cancelPercent,
      );

      await page.getByTestId(id("cancel")).click();
      expect(await app.requestsFor("cancel_ml_model_download")).toEqual([{ model: m.id }]);
      await expect(page.getByTestId(id("cancel"))).toHaveText("Cancelling…");
      await expect(page.getByTestId(id("cancel"))).toBeDisabled();
      // The answer to Cancel did not move the bar back to 0.
      await expect(page.getByTestId(id("progress"))).toHaveAttribute(
        "aria-valuenow",
        m.cancelPercent,
      );

      // The thread stopped; what it had received is kept.
      await app.emit(PROGRESS, m.status({ state: "not_downloaded", bytes: 20_500_000 }));
      await expect(row).toHaveAttribute("data-state", "not_downloaded");
      await expect(page.getByTestId(id("status"))).toContainText(m.partialText);
      await expect(page.getByTestId(id("download"))).toHaveText("Resume download");
      expect(await app.requestsFor("download_ml_model")).toEqual([]);
    });

    test(`${m.name}: Remove asks first, and removes only when told yes`, async ({ app }) => {
      const { row, page } = await openSettings(
        m,
        app,
        startingAt(m, m.status({ state: "ready" })),
      );
      await expect(row).toHaveAttribute("data-state", "ready");

      // No: nothing is removed.
      const asked: string[] = [];
      page.once("dialog", async (dialog) => {
        asked.push(dialog.message());
        await dialog.dismiss();
      });
      await page.getByTestId(id("remove")).click();
      await expect.poll(() => asked.length).toBe(1);
      expect(asked[0]).toContain(m.removeSubject);
      expect(asked[0]).toContain(m.removeSize);
      expect(await app.requestsFor("remove_ml_model")).toEqual([]);
      await expect(row).toHaveAttribute("data-state", "ready");

      // Yes: it goes.
      page.once("dialog", (dialog) => void dialog.accept());
      await page.getByTestId(id("remove")).click();
      await expect(row).toHaveAttribute("data-state", "not_downloaded");
      expect(await app.requestsFor("remove_ml_model")).toEqual([{ model: m.id }]);
      await expect(page.getByTestId(id("download"))).toBeVisible();
    });

    test(`${m.name}: a failed download shows its reason and Retry starts it again`, async ({
      app,
    }) => {
      const reason = "downloading https://huggingface.co/x failed (HTTP 503): Service Unavailable";
      const { row, page } = await openSettings(
        m,
        app,
        startingAt(m, m.status({ state: "failed", reason })),
      );

      await expect(row).toHaveAttribute("data-state", "failed");
      await expect(page.getByTestId(id("status"))).toContainText("The download failed:");
      await expect(page.getByTestId(id("status"))).toContainText("HTTP 503");

      await page.getByTestId(id("retry")).click();
      await expect(row).toHaveAttribute("data-state", "downloading");
      expect(await app.requestsFor("download_ml_model")).toEqual([{ model: m.id }]);
    });

    for (const size of [
      { label: "the default window", width: 1280, height: 775 },
      { label: "the smallest window", width: 1024, height: 640 },
    ] as const) {
      test(`${m.name}: the row fits inside the dialog in ${size.label}, in every state`, async ({
        app,
      }) => {
        const { row, page, card } = await openSettings(m, app);
        await page.setViewportSize({ width: size.width, height: size.height });

        const states = [
          m.status({ state: "not_downloaded" }),
          m.status({ state: "not_downloaded", bytes: 41_200_000 }),
          // Files as big as the model that were not verified.
          m.status({ state: "not_downloaded", bytes: m.bytes }),
          m.status({ state: "downloading", received: 41_200_000 }),
          m.status({ state: "ready" }),
          m.status({ state: "failed", reason: m.longReason, bytes: 12_000_000 }),
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
  });
}
