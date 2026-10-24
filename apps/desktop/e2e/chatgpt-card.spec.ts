/**
 * The ChatGPT plan's card in Settings → Account, in a real browser (#474 PR 6).
 *
 * The backend is replaced at the IPC boundary and the spec raises
 * `chatgpt://status` as the backend does, so what is checked is what the window
 * draws from it: the card goes Signed out → Signing in → Signed in, Sign out
 * returns it to Signed out, and the other states (session ended, plan usage
 * declined) are drawn from the status alone. Nothing here signs in anywhere.
 *
 * With `E2E_SHOTS_DIR` set, each state's card is also saved there as a PNG.
 */

import { mkdirSync } from "node:fs";
import { join } from "node:path";

import {
  CHATGPT_SIGNED_IN,
  CHATGPT_SIGNED_OUT,
  DURABLE_SAVE,
  chatgptCommands,
  chatgptModelList,
  deferred,
  ok,
  readyToLoad,
} from "./backend";
import { expect, test, type App } from "./fixtures";

test.use({ viewport: { width: 1280, height: 900 } });

const SHOTS = process.env.E2E_SHOTS_DIR;

async function shot(app: App, name: string) {
  if (!SHOTS) return;
  mkdirSync(SHOTS, { recursive: true });
  await app.page.getByTestId("settings-card").screenshot({ path: join(SHOTS, `settings-${name}.png`) });
}

async function openChatGptCard(app: App) {
  await app.boot({
    ...readyToLoad(),
    ...chatgptCommands(CHATGPT_SIGNED_OUT),
    // Held open for as long as the person is in the browser.
    chatgpt_sign_in: deferred("sign-in"),
    set_active_provider: ok(DURABLE_SAVE),
    set_active_model: ok(DURABLE_SAVE),
    set_effort_for: ok(DURABLE_SAVE),
  });
  const page = app.page;
  await expect(page.getByTestId("empty-state")).toBeVisible();
  await page.getByTestId("open-settings-button").click();
  await expect(page.getByTestId("settings-card")).toBeVisible();
  // The radio is visually hidden; its label is the target.
  await page.getByTestId("settings-provider-chatgpt").locator("xpath=..").click();
  const card = page.getByTestId("settings-chatgpt-card");
  await expect(card).toHaveAttribute("data-state", "signed_out");
  return { page, card };
}

test("the card goes Signed out, Signing in, Signed in, and Sign out returns it to Signed out", async ({
  app,
}) => {
  const { page, card } = await openChatGptCard(app);

  // Signed out: a text-only button, and what it does.
  await expect(page.getByTestId("chatgpt-continue")).toHaveText("Continue with ChatGPT");
  await expect(card).toContainText("Uses your ChatGPT plan's usage instead of an API key");
  await expect(page.getByTestId("chatgpt-eligibility")).toContainText("workspace plans");
  // The key, address and model fields are not here.
  await expect(page.getByTestId("settings-key-input")).toHaveCount(0);
  await expect(page.getByTestId("settings-base-url-input")).toHaveCount(0);
  await shot(app, "1-signed-out");
  // Choosing the plan while signed out activates nothing.
  expect(await app.requestsFor("set_active_provider")).toEqual([]);

  // Signing in: the backend opens the browser, and says so on the event.
  await page.getByTestId("chatgpt-continue").click();
  await expect.poll(() => app.requestsFor("chatgpt_sign_in")).toEqual([{ intent: "sign_in" }]);
  await app.emit("chatgpt://status", {
    ...CHATGPT_SIGNED_OUT,
    state: "signing_in",
  });
  await expect(card).toHaveAttribute("data-state", "signing_in");
  await expect(card).toContainText("Finish signing in in your browser…");
  await expect(page.getByTestId("chatgpt-cancel")).toBeVisible();
  await shot(app, "2-signing-in");

  // Signed in: the sign-in resolves, and the event follows.
  await app.become({ list_models_for: ok(chatgptModelList()) });
  await app.release("sign-in", CHATGPT_SIGNED_IN);
  await app.emit("chatgpt://status", CHATGPT_SIGNED_IN);
  await expect(card).toHaveAttribute("data-state", "signed_in");
  await expect(page.getByTestId("chatgpt-account")).toHaveText("Signed in as person@example.com");
  await expect(page.getByTestId("chatgpt-using-plan")).toHaveText("Using ChatGPT plan");
  // The sign-in made the plan the active provider.
  await expect.poll(() => app.requestsFor("set_active_provider")).toEqual([{ provider: "chatgpt" }]);
  // The account's models, and the first saved for a person who chose none.
  const models = page.getByTestId("chatgpt-model-select");
  await expect(models.locator("option")).toHaveText([
    "GPT-6.1 Sol (gpt-6.1-sol)",
    "GPT-6 Luna (gpt-6-luna)",
  ]);
  await expect.poll(() => app.requestsFor("set_active_model")).toEqual([
    { provider: "chatgpt", model: "gpt-6.1-sol" },
  ]);
  // The chosen model lists low, medium and high, so those are offered.
  const effort = page.getByTestId("chatgpt-effort-select");
  await expect(effort.locator("option")).toHaveText(["Default (model's own)", "Low", "Medium", "High"]);
  await shot(app, "3-signed-in");

  // A model that lists no levels has no picker.
  await models.selectOption("gpt-6-luna");
  await expect(effort).toHaveCount(0);
  await models.selectOption("gpt-6.1-sol");
  await effort.selectOption("high");
  await expect.poll(() => app.requestsFor("set_effort_for")).toEqual([
    { provider: "chatgpt", effort: "high" },
  ]);

  // Manage usage asks the backend to open the page and gives it no address.
  await page.getByTestId("chatgpt-manage-usage").click();
  await expect.poll(() => app.requestsFor("open_chatgpt_usage")).toHaveLength(1);
  expect((await app.requestsFor("open_chatgpt_usage"))[0] ?? {}).toEqual({});

  // Sign out returns it to Signed out, with the disconnect line.
  await page.getByTestId("chatgpt-sign-out").click();
  await expect(card).toHaveAttribute("data-state", "signed_out");
  await expect(page.getByTestId("chatgpt-end-notice")).toContainText(
    "Signing out here does not disconnect edytlab from ChatGPT",
  );
  await expect(page.getByTestId("chatgpt-continue")).toBeVisible();
  await shot(app, "4-signed-out-again");
  expect(await app.requestsFor("chatgpt_sign_out")).toHaveLength(1);
});

test("a session that ended, and plan usage that was declined, are drawn from the status", async ({
  app,
}) => {
  const { page, card } = await openChatGptCard(app);

  await app.emit("chatgpt://status", {
    ...CHATGPT_SIGNED_OUT,
    state: "session_ended",
    email: "person@example.com",
    reason: {
      code: "refresh_token_unusable",
      message: "OpenAI no longer accepts the saved sign-in.",
    },
  });
  await expect(card).toHaveAttribute("data-state", "session_ended");
  await expect(card).toContainText("Your ChatGPT session ended.");
  await expect(page.getByTestId("chatgpt-reason")).toContainText("no longer accepts");
  await expect(page.getByTestId("chatgpt-continue")).toHaveText("Continue with ChatGPT");
  await shot(app, "5-session-ended");

  await app.emit("chatgpt://status", {
    ...CHATGPT_SIGNED_IN,
    state: "plan_disabled",
    reason: { code: "plan_usage_declined", message: "Plan usage was not granted." },
  });
  await expect(card).toHaveAttribute("data-state", "plan_disabled");
  await expect(page.getByTestId("chatgpt-continue")).toHaveText("Turn plan usage back on");
  await shot(app, "6-plan-disabled");
  await page.getByTestId("chatgpt-continue").click();
  await expect
    .poll(() => app.requestsFor("chatgpt_sign_in"))
    .toEqual([{ intent: "enable_plan_usage" }]);
  await app.emit("chatgpt://status", { ...CHATGPT_SIGNED_OUT, state: "signing_in" });
  await app.release("sign-in", CHATGPT_SIGNED_OUT);
});

test("a token that will not survive a restart is said to be so", async ({ app }) => {
  const { page, card } = await openChatGptCard(app);
  await app.become({ list_models_for: ok(chatgptModelList()) });
  await app.emit("chatgpt://status", {
    ...CHATGPT_SIGNED_IN,
    tokenStorage: {
      kind: "kernel_keyring",
      persistent: false,
      reason: "Platform secure storage failure: DBus error",
    },
  });
  await expect(card).toHaveAttribute("data-state", "signed_in");
  await expect(page.getByTestId("chatgpt-storage-notice")).toContainText("Secret Service");
  await shot(app, "7-session-only-storage");
});
