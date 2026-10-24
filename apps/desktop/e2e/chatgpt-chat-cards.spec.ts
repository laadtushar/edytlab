/**
 * The cards Chat draws for a turn that failed on the ChatGPT plan (#474 PR 6),
 * in a real browser.
 *
 * `send_message` rejects with the text the backend writes
 * (`plan_usage_text` in `chatgpt.rs`): a marker with the failure's code and
 * OpenAI's request ID, then a sentence. The window picks the card by the code.
 * With `E2E_SHOTS_DIR` set, each card is also saved there as a PNG.
 */

import { mkdirSync } from "node:fs";
import { join } from "node:path";

import { fixturePath } from "./audio-fixtures";
import {
  CHATGPT_SIGNED_IN,
  chatgptCommands,
  nodeId,
  ok,
  projectWith,
  reject,
  toneTrack,
} from "./backend";
import { expect, test, type App } from "./fixtures";

test.use({ viewport: { width: 1280, height: 800 } });

const HEAD = nodeId(2);
const SHOTS = process.env.E2E_SHOTS_DIR;

async function ask(app: App, answer: ReturnType<typeof reject>, extra: Record<string, unknown> = {}) {
  await app.boot({
    ...projectWith([toneTrack()], HEAD),
    ...chatgptCommands(),
    render_preview: ok(fixturePath("tone3s")),
    send_message: answer,
    ...extra,
  });
  const page = app.page;
  await expect(page.getByTestId("ruler")).toContainText("0:03");
  const box = page.getByTestId("chat-form").locator("textarea");
  await box.fill("make it louder");
  await box.press("Enter");
  return page;
}

async function shot(app: App, testId: string, name: string) {
  if (!SHOTS) return;
  mkdirSync(SHOTS, { recursive: true });
  await app.page.getByTestId(testId).screenshot({ path: join(SHOTS, `chat-${name}.png`) });
}

test("a usage limit is a card with Manage usage, the request ID and no Retry", async ({ app }) => {
  const page = await ask(
    app,
    reject(
      "[plan-usage code=usage_limit request-id=req_abc123] Your ChatGPT plan's usage limit has been reached.",
    ),
    { open_chatgpt_usage: ok(null) },
  );
  const card = page.getByTestId("plan-usage-limit-card");
  await expect(card).toBeVisible();
  await expect(card).toContainText("usage limit for apps like edytlab has been reached");
  await expect(card).toContainText("Signing in again or retrying won't restore usage.");
  await expect(page.getByTestId("plan-request-id")).toHaveText("Request ID: req_abc123");
  await shot(app, "plan-usage-limit-card", "usage-limit");
  // Not a refused key, and nothing to retry.
  await expect(page.getByTestId("chat-error")).toHaveCount(0);
  await expect(card.getByRole("button", { name: /retry|try again/i })).toHaveCount(0);

  await page.getByTestId("plan-manage-usage").click();
  await expect.poll(() => app.requestsFor("open_chatgpt_usage")).toHaveLength(1);

  // Nothing sends the message again by itself.
  await app.settle();
  expect(await app.requestsFor("send_message")).toHaveLength(1);
  expect(await app.requestsFor("chatgpt_sign_in")).toEqual([]);
});

test("a session that ended is a card, and Continue signs in and sends what the backend kept", async ({
  app,
}) => {
  const page = await ask(
    app,
    reject("[plan-usage code=session_ended request-id=req_end1] Your ChatGPT session ended."),
    {
      chatgpt_sign_in: ok(CHATGPT_SIGNED_IN),
      chatgpt_take_pending_message: ok({
        text: "make it louder",
        resumeText: "make it louder",
        disabledTools: ["render_preview"],
        progressed: false,
      }),
    },
  );
  const card = page.getByTestId("plan-session-ended-card");
  await expect(card).toBeVisible();
  await expect(card).toContainText(
    "Your ChatGPT session ended. Continue with ChatGPT to finish this message.",
  );
  await shot(app, "plan-session-ended-card", "session-ended");

  // The next send is accepted.
  await app.become({ send_message: ok(null) });
  await page.getByTestId("plan-continue").click();
  await expect.poll(() => app.requestsFor("send_message")).toHaveLength(2);
  const sends = await app.requestsFor("send_message");
  expect(sends[1]).toEqual({ text: "make it louder", disabledTools: ["render_preview"] });
  expect(await app.requestsFor("chatgpt_sign_in")).toEqual([{ intent: "sign_in" }]);
  expect(await app.requestsFor("chatgpt_take_pending_message")).toHaveLength(1);
  await expect(card).toHaveCount(0);
  // The message is in the log once, not twice.
  await expect(page.getByText("make it louder")).toHaveCount(1);
});

test("a turn that got further than sending says the assistant will carry on", async ({ app }) => {
  const page = await ask(
    app,
    reject("[plan-usage code=session_ended] Your ChatGPT session ended."),
    {
      chatgpt_sign_in: ok(CHATGPT_SIGNED_IN),
      chatgpt_take_pending_message: ok({
        text: "make it louder",
        resumeText: "Carry on from where you stopped.",
        disabledTools: [],
        progressed: true,
      }),
    },
  );
  await expect(page.getByTestId("plan-session-ended-card")).toBeVisible();
  await app.become({ send_message: ok(null) });
  await page.getByTestId("plan-continue").click();
  await expect(page.getByTestId("chat-resume-note")).toHaveText(
    "Signed in. The assistant will carry on from where it stopped.",
  );
  const sends = await app.requestsFor("send_message");
  expect(sends[1]).toEqual({ text: "Carry on from where you stopped.", disabledTools: [] });
  await shot(app, "chat-resume-note", "resume-note");
});

test("the other plan failures are one sentence and the request ID, never a bad key", async ({ app }) => {
  const page = await ask(
    app,
    reject("[plan-usage code=not_eligible request-id=req_nope] unauthorized: this workspace"),
  );
  const card = page.getByTestId("plan-error-card");
  await expect(card).toHaveAttribute("data-code", "not_eligible");
  await expect(page.getByTestId("plan-error-sentence")).toHaveText(
    "ChatGPT plan usage isn't available for this account or workspace.",
  );
  await expect(page.getByTestId("plan-request-id")).toHaveText("Request ID: req_nope");
  await expect(page.getByTestId("chat-error")).toHaveCount(0);
  await expect(page.getByTestId("chat-error-open-settings")).toHaveCount(0);
  await shot(app, "plan-error-card", "not-eligible");
});
