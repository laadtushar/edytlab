/**
 * OpenAI's ChatGPT logo where the app offers or shows the ChatGPT plan
 * (#474), in a real browser: the four places load the bundled files under the
 * shipped Content Security Policy, the logo sits level with its label, and the
 * file on each surface is the white or black one that surface needs.
 *
 * jsdom draws nothing, so "the image loaded" (its natural size is not zero),
 * "no CSP violation" and "centred on the text" are only checkable here.
 * With `E2E_SHOTS_DIR` set, each place is also saved there as a PNG.
 */

import { mkdirSync } from "node:fs";
import { join } from "node:path";

import { fixturePath } from "./audio-fixtures";
import {
  CHATGPT_SIGNED_IN,
  CHATGPT_SIGNED_OUT,
  DURABLE_SAVE,
  chatgptCommands,
  chatgptModelList,
  nodeId,
  ok,
  projectWith,
  readyToLoad,
  reject,
  toneTrack,
} from "./backend";
import { expect, test, type App } from "./fixtures";
import type { Locator, Page } from "@playwright/test";

test.use({ viewport: { width: 1280, height: 800 } });

const SHOTS = process.env.E2E_SHOTS_DIR;

async function shot(target: Locator, name: string) {
  if (!SHOTS) return;
  mkdirSync(SHOTS, { recursive: true });
  await target.screenshot({ path: join(SHOTS, `${name}.png`) });
}

/** Watches for the browser refusing anything on policy grounds. */
function watchPolicy(page: Page): string[] {
  const refused: string[] = [];
  page.on("console", (m) => {
    if (/content security policy|refused to (load|connect)/i.test(m.text())) refused.push(m.text());
  });
  return refused;
}

/** The image decoded: a file that did not load has no natural size. */
async function loaded(logo: Locator) {
  await expect
    .poll(() => logo.evaluate((el) => (el as HTMLImageElement).complete && (el as HTMLImageElement).naturalWidth > 0))
    .toBe(true);
}

/** The logo's vertical centre is on its neighbour text's, within 2 px. */
async function levelWith(logo: Locator, text: Locator) {
  const a = await logo.boundingBox();
  const b = await text.boundingBox();
  expect(a).not.toBeNull();
  expect(b).not.toBeNull();
  expect(Math.abs(a!.y + a!.height / 2 - (b!.y + b!.height / 2))).toBeLessThanOrEqual(2);
}

/**
 * The logo sits level with the first line of a block that wraps: its centre is
 * on the centre of that line, whose height is the block's computed line height.
 */
async function levelWithFirstLine(logo: Locator, block: Locator) {
  const a = (await logo.boundingBox())!;
  const b = (await block.boundingBox())!;
  const lineHeight = await block.evaluate((el) => parseFloat(getComputedStyle(el).lineHeight));
  expect(lineHeight).toBeGreaterThan(0);
  expect(Math.abs(a.y + a.height / 2 - (b.y + lineHeight / 2))).toBeLessThanOrEqual(3);
}

async function openSettingsCard(app: App, status: Record<string, unknown>) {
  await app.boot({
    ...readyToLoad(),
    ...chatgptCommands(status),
    list_models_for: ok(chatgptModelList()),
    set_active_provider: ok(DURABLE_SAVE),
    set_active_model: ok(DURABLE_SAVE),
    set_effort_for: ok(DURABLE_SAVE),
  });
  const page = app.page;
  await expect(page.getByTestId("empty-state")).toBeVisible();
  await page.getByTestId("open-settings-button").click();
  await expect(page.getByTestId("settings-card")).toBeVisible();
  return page;
}

test("Settings: the provider list and the Continue with ChatGPT button carry OpenAI's logo", async ({ app }) => {
  const refused = watchPolicy(app.page);
  const page = await openSettingsCard(app, CHATGPT_SIGNED_OUT);

  // The provider list: the ChatGPT plan entry only, white on the dark chip.
  const entry = page.getByTestId("settings-provider-chatgpt").locator("xpath=..");
  const listed = entry.getByTestId("chatgpt-logo");
  await loaded(listed);
  await expect(listed).toHaveAttribute("data-logo", "white");
  await levelWith(listed, entry);
  await expect(page.getByTestId("settings-provider-picker").locator("img")).toHaveCount(1);
  await shot(page.getByTestId("settings-provider-picker"), "1-settings-provider-list");

  // Choosing it shows the card, with the sign-in button in the approved format.
  await entry.click();
  const card = page.getByTestId("settings-chatgpt-card");
  await expect(card).toHaveAttribute("data-state", "signed_out");
  const button = page.getByTestId("chatgpt-continue");
  await expect(button).toHaveText("Continue with ChatGPT");
  const logo = button.getByTestId("chatgpt-logo");
  await loaded(logo);
  // Dark text on the violet fill: the black logo, 18 px, square.
  await expect(logo).toHaveAttribute("data-logo", "black");
  const box = await logo.boundingBox();
  expect(box!.width).toBeCloseTo(18, 0);
  expect(box!.height).toBeCloseTo(18, 0);
  // It sits inside the button, to the left of the label, level with it.
  const button_ = (await button.boundingBox())!;
  expect(box!.x).toBeGreaterThanOrEqual(button_.x);
  expect(box!.x + box!.width).toBeLessThan(button_.x + button_.width / 2);
  await levelWith(logo, button);
  await shot(card, "2-settings-continue-with-chatgpt");

  expect(refused).toEqual([]);
});

test("Settings: signed in, the Using ChatGPT plan badge carries the white logo", async ({ app }) => {
  const page = await openSettingsCard(app, CHATGPT_SIGNED_IN);
  await page.getByTestId("settings-provider-chatgpt").locator("xpath=..").click();
  const badge = page.getByTestId("chatgpt-using-plan");
  await expect(badge).toHaveText("Using ChatGPT plan");
  const logo = badge.getByTestId("chatgpt-logo");
  await loaded(logo);
  await expect(logo).toHaveAttribute("data-logo", "white");
  await levelWith(logo, badge);
});

test("Chat: Using ChatGPT plan above the composer, only while the plan is active and signed in", async ({ app }) => {
  const refused = watchPolicy(app.page);
  await app.boot({
    ...projectWith([toneTrack()], nodeId(2)),
    ...chatgptCommands(CHATGPT_SIGNED_IN),
    get_active_provider: ok("chatgpt"),
    render_preview: ok(fixturePath("tone3s")),
  });
  const page = app.page;
  await expect(page.getByTestId("ruler")).toContainText("0:03");
  const indicator = page.getByTestId("plan-indicator");
  await expect(indicator).toContainText("Using ChatGPT plan");
  await expect(indicator).toContainText("Manage usage");
  const logo = indicator.getByTestId("chatgpt-logo");
  await loaded(logo);
  await expect(logo).toHaveAttribute("data-logo", "white");
  await levelWith(logo, indicator.getByTestId("plan-indicator-text"));
  // It is above the message box.
  const above = (await indicator.boundingBox())!;
  const input = (await page.getByRole("textbox", { name: "Message" }).boundingBox())!;
  expect(above.y + above.height).toBeLessThanOrEqual(input.y);
  await shot(page.getByTestId("chat-form"), "3-chat-using-chatgpt-plan");

  // Signing out takes it away.
  await app.emit("chatgpt://status", CHATGPT_SIGNED_OUT);
  await expect(indicator).toHaveCount(0);
  expect(refused).toEqual([]);
});

test("Chat: no Using ChatGPT plan line while another provider is active", async ({ app }) => {
  await app.boot({
    ...projectWith([toneTrack()], nodeId(2)),
    ...chatgptCommands(CHATGPT_SIGNED_IN),
    get_active_provider: ok("anthropic"),
  });
  await expect(app.page.getByTestId("chat-form")).toBeVisible();
  await expect(app.page.getByTestId("plan-indicator")).toHaveCount(0);
});

for (const [name, text, card] of [
  [
    "4-chat-usage-limit-card",
    "[plan-usage code=usage_limit request-id=req_abc123] Your ChatGPT plan's usage limit has been reached.",
    "plan-usage-limit-card",
  ],
  [
    "5-chat-session-ended-card",
    "[plan-usage code=session_ended request-id=req_end1] Your ChatGPT session ended.",
    "plan-session-ended-card",
  ],
] as const) {
  test(`Chat: the ${card} has the logo in its header`, async ({ app }) => {
    const refused = watchPolicy(app.page);
    await app.boot({
      ...projectWith([toneTrack()], nodeId(2)),
      ...chatgptCommands(CHATGPT_SIGNED_IN),
      get_active_provider: ok("chatgpt"),
      render_preview: ok(fixturePath("tone3s")),
      send_message: reject(text),
    });
    const page = app.page;
    await expect(page.getByTestId("ruler")).toContainText("0:03");
    const box = page.getByTestId("chat-form").locator("textarea");
    await box.fill("make it louder");
    await box.press("Enter");
    const cardEl = page.getByTestId(card);
    await expect(cardEl).toBeVisible();
    const logos = cardEl.getByTestId("chatgpt-logo");
    const first = logos.first();
    await loaded(first);
    await expect(first).toHaveAttribute("data-logo", "white");
    await levelWithFirstLine(first, cardEl.locator("p").first());
    if (card === "plan-session-ended-card") {
      // And the Continue with ChatGPT button, with the black logo on its fill.
      const cont = page.getByTestId("plan-continue");
      await expect(cont).toHaveText("Continue with ChatGPT");
      await loaded(cont.getByTestId("chatgpt-logo"));
      await expect(cont.getByTestId("chatgpt-logo")).toHaveAttribute("data-logo", "black");
      await levelWith(cont.getByTestId("chatgpt-logo"), cont);
    }
    await shot(cardEl, name);
    expect(refused).toEqual([]);
  });
}
