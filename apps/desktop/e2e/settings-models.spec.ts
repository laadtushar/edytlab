/**
 * Settings' model picker says which list it is showing (#468).
 *
 * Anthropic's models are fetched live with the saved key. With none, or
 * when the fetch fails, the backend answers with edytlab's built-in list
 * and says so; the picker must not present that as Anthropic's own list.
 * jsdom cannot see whether the note is drawn inside the card, and a
 * `<datalist>` is only real in a browser, so this runs in Chromium.
 *
 * The fake backend's `list_models_for` is `anthropic_fallback_models()`
 * with the notice "no API key saved" (see `providerCommands`).
 */

import { ok, readyToLoad } from "./backend";
import { expect, test, type App } from "./fixtures";

/** The built-in list, in the order `anthropic_fallback_models()` has it. */
const FALLBACK_IDS = [
  "claude-sonnet-5-5",
  "claude-haiku-5-5",
  "claude-opus-5-5",
  "claude-fable-5-1",
];

async function openSettings(app: App) {
  await app.boot(readyToLoad());
  const page = app.page;
  await expect(page.getByTestId("empty-state")).toBeVisible();
  await page.getByTestId("open-settings-button").click();
  await expect(page.getByTestId("settings-card")).toBeVisible();
  return { page, card: page.getByTestId("settings-card") };
}

test("with no key to fetch the list with, the picker offers the built-in models and says so", async ({
  app,
}) => {
  const { page, card } = await openSettings(app);

  const note = page.getByTestId("settings-model-fallback");
  await expect(note).toBeVisible();
  await expect(note).toContainText("built-in list");
  await expect(note).toContainText("no API key saved");
  // Inside the dialog, not clipped by it.
  const noteBox = await note.boundingBox();
  const cardBox = await card.boundingBox();
  expect(noteBox && cardBox).toBeTruthy();
  expect(noteBox!.x).toBeGreaterThanOrEqual(cardBox!.x);
  expect(noteBox!.x + noteBox!.width).toBeLessThanOrEqual(cardBox!.x + cardBox!.width + 0.5);
  expect(noteBox!.y + noteBox!.height).toBeLessThanOrEqual(cardBox!.y + cardBox!.height + 0.5);

  // A live list's hint is not shown beside it.
  await expect(page.getByTestId("settings-model-hint")).toHaveCount(0);

  // The datalist holds exactly the built-in models, in order.
  const ids = await page
    .getByTestId("settings-model-datalist")
    .locator("option")
    .evaluateAll((options) => options.map((o) => o.getAttribute("value")));
  expect(ids).toEqual(FALLBACK_IDS);
});

test("a list the provider answered with is described by its size, with no fallback note", async ({
  app,
}) => {
  await app.boot({
    ...readyToLoad(),
    list_models_for: ok({
      models: [
        {
          id: "claude-sonnet-5-5",
          display_name: "Claude Sonnet 5.5 (default)",
          context_length: 1_000_000,
          provider_hint: null,
        },
        {
          id: "claude-haiku-5-5",
          display_name: "Claude Haiku 5.5",
          context_length: 1_000_000,
          provider_hint: null,
        },
      ],
      source: "live",
      notice: null,
    }),
  });
  const page = app.page;
  await expect(page.getByTestId("empty-state")).toBeVisible();
  await page.getByTestId("open-settings-button").click();

  await expect(page.getByTestId("settings-model-hint")).toContainText("2 models available");
  await expect(page.getByTestId("settings-model-fallback")).toHaveCount(0);
});

// A saved model that is no longer in Anthropic's own list may have been
// retired. Settings says so and offers the default: one click, and the
// backend is pointed at it.
test("a saved model missing from Anthropic's list is flagged, and the default is one click away", async ({
  app,
}) => {
  await app.boot({
    ...readyToLoad(),
    get_active_model: ok("claude-opus-4-1-20250805"),
    set_active_model: ok(null),
    list_models_for: ok({
      models: [
        {
          id: "claude-sonnet-5-5",
          display_name: "Claude Sonnet 5.5 (default)",
          context_length: 1_000_000,
          provider_hint: null,
        },
        {
          id: "claude-haiku-5-5",
          display_name: "Claude Haiku 5.5",
          context_length: 1_000_000,
          provider_hint: null,
        },
      ],
      source: "live",
      notice: null,
    }),
  });
  const page = app.page;
  await expect(page.getByTestId("empty-state")).toBeVisible();
  await page.getByTestId("open-settings-button").click();

  const warning = page.getByTestId("settings-model-unlisted");
  await expect(warning).toBeVisible();
  await expect(warning).toContainText("claude-opus-4-1-20250805");
  await expect(warning).toContainText("claude-sonnet-5-5");
  expect(await app.requestsFor("set_active_model")).toEqual([]);

  await page.getByTestId("settings-model-use-default").click();

  await expect(page.getByTestId("settings-model-input")).toHaveValue("claude-sonnet-5-5");
  await expect(warning).toHaveCount(0);
  expect(await app.requestsFor("set_active_model")).toEqual([
    { provider: "anthropic", model: "claude-sonnet-5-5" },
  ]);
});
