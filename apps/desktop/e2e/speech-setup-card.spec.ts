/**
 * The card under a `transcribe` call that could not run (#384), in a real
 * browser.
 *
 * jsdom cannot see where the card is drawn or that its button takes the user
 * to the row that fixes it. The backend never sends the window a tool's error
 * text: it turns the marker that begins `transcribe`'s error into a view on
 * `agent://tool-call-end` (`ai::ToolView::SpeechSetup`, from
 * `agent_loop::extract_error_view`), so this raises that event as the backend
 * does and checks what the window draws from it. The assistant's words,
 * whatever they are, draw nothing.
 *
 * Nothing here downloads: the card sends the user to Settings, and the only
 * thing that downloads is the button in the speech model's row, which this
 * spec does not press.
 */

import { fixturePath } from "./audio-fixtures";
import {
  deferred,
  nodeId,
  ok,
  projectWith,
  speechModelCommands,
  toneTrack,
} from "./backend";
import { expect, test, type App } from "./fixtures";

test.use({ viewport: { width: 1280, height: 800 } });

const HEAD = nodeId(2);

async function askToTranscribe(app: App) {
  await app.boot({
    ...projectWith([toneTrack()], HEAD),
    ...speechModelCommands(),
    render_preview: ok(fixturePath("tone3s")),
    // Held open for the whole turn, as in the real app.
    send_message: deferred("send"),
  });
  const page = app.page;
  await expect(page.getByTestId("ruler")).toContainText("0:03");
  const box = page.getByTestId("chat-form").locator("textarea");
  await box.fill("transcribe track 0");
  await box.press("Enter");
  await expect.poll(() => app.requestsFor("send_message")).toHaveLength(1);
  await app.emit("agent://tool-call", { name: "transcribe", id: "t1" });
  await expect(page.getByTestId("tool-badge")).toHaveAttribute("data-status", "running");
}

test("a speech model that is not downloaded is a card under the call, and Open Settings shows the row that fixes it", async ({
  app,
}) => {
  await askToTranscribe(app);
  const page = app.page;

  await app.emit("agent://tool-call-end", {
    id: "t1",
    ok: false,
    not_run: false,
    view: { type: "speech_setup", problem: "model_not_downloaded" },
  });
  const card = page.getByTestId("speech-setup-card");
  await expect(card).toBeVisible();
  await expect(card).toContainText("The speech model is not downloaded");
  await expect(card).toContainText("Settings → Account");
  await expect(card).not.toContainText(/install/i);
  // It is under the call's badge, in the same column.
  const badge = await page.getByTestId("tool-badge").boundingBox();
  const cardBox = await card.boundingBox();
  expect(cardBox!.y).toBeGreaterThan(badge!.y);

  // Nothing has been asked for yet: the card downloads nothing.
  expect(await app.requestsFor("download_ml_model")).toEqual([]);

  await page.getByTestId("speech-setup-open-settings").click();
  await expect(page.getByTestId("settings-card")).toBeVisible();
  const row = page.getByTestId("settings-speech-model");
  await expect(row).toBeVisible();
  await expect(row).toHaveAttribute("data-state", "not_downloaded");
  await expect(page.getByTestId("speech-model-download")).toBeVisible();
  // Opening Settings from the card is not pressing Download.
  expect(await app.requestsFor("download_ml_model")).toEqual([]);
  await app.release("send", null);
});

test("the assistant's words about the model draw no card", async ({ app }) => {
  await askToTranscribe(app);
  const page = app.page;

  // The call failed for a reason that is not a setup problem...
  await app.emit("agent://tool-call-end", { id: "t1", ok: false, not_run: false });
  // ...and the assistant says what the old card was searched for.
  await app.emit("agent://text-delta", {
    text: "The speech model is not downloaded. Whisper model not found: set WHISPER_MODEL_PATH.",
  });
  await app.emit("agent://done", {});

  await expect(
    page.getByTestId("message-bubble").filter({ hasText: "Whisper model not found" }),
  ).toBeVisible();
  await expect(page.getByTestId("speech-setup-card")).toHaveCount(0);
  await expect(page.getByTestId("tool-badge")).toHaveAttribute("data-status", "error");
  await app.release("send", null);
});

test("a build with no runtime gets its card without a Settings button", async ({ app }) => {
  await askToTranscribe(app);
  const page = app.page;

  await app.emit("agent://tool-call-end", {
    id: "t1",
    ok: false,
    not_run: false,
    view: { type: "speech_setup", problem: "runtime_unavailable" },
  });
  const card = page.getByTestId("speech-setup-card");
  await expect(card).toContainText("ORT_DYLIB_PATH");
  await expect(page.getByTestId("speech-setup-open-settings")).toHaveCount(0);
  await app.release("send", null);
});
