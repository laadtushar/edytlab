/**
 * Every Settings tab is inside the dialog, and reachable (#392).
 *
 * There are seven tabs and the dialog is 30rem wide. In WebKitGTK the
 * labels add up to about 305px and the strip needs about 497px of the
 * 438px it has, so Plugins sat past the right edge of an `overflow-hidden`
 * card: clipped, impossible to click, and, because the card was then the
 * nearest scroll container, keyboard focus on it slid the whole dialog
 * sideways. jsdom has no layout, so the unit test can only model that;
 * this checks the real boxes in Chromium.
 *
 * The window is the one `tauri.conf.json` opens (1280 x 800), not
 * Playwright's 1280 x 720. The narrow tests shrink it to 420, where the
 * card is 378px and the strip has to scroll.
 *
 * Project, Memory, Skills, Agents and MCP are focused here but never
 * clicked or entered: their editors call commands this fake backend does
 * not answer, and the fixture fails a test on any call it was not told
 * how to answer.
 */

import type { Locator } from "@playwright/test";

import { readyToLoad } from "./backend";
import { expect, test, type App } from "./fixtures";

test.use({ viewport: { width: 1280, height: 800 } });

const TAB_IDS = ["account", "project", "memory", "skills", "agents", "mcp", "plugins"] as const;

interface Box {
  x: number;
  y: number;
  width: number;
  height: number;
}

async function boxOf(locator: Locator): Promise<Box> {
  const box = await locator.boundingBox();
  if (!box) throw new Error("the element has no box: it is not rendered");
  return box;
}

/** Whether `inner` lies inside `outer` on both axes, give or take `slack` px. */
function inside(inner: Box, outer: Box, slack = 0.5): boolean {
  return (
    inner.x >= outer.x - slack &&
    inner.y >= outer.y - slack &&
    inner.x + inner.width <= outer.x + outer.width + slack &&
    inner.y + inner.height <= outer.y + outer.height + slack
  );
}

function show(b: Box): string {
  return `x ${b.x.toFixed(1)}..${(b.x + b.width).toFixed(1)}`;
}

/**
 * The page loaded, the shipped face loaded (the widths below are
 * measured in it, not in a fallback), and Settings open on Account.
 */
async function openSettings(app: App) {
  await app.boot(readyToLoad());
  const page = app.page;
  await expect(page.getByTestId("empty-state")).toBeVisible();
  await page.evaluate(() => document.fonts.load('400 14px "Geist Variable"'));
  await page.getByTestId("open-settings-button").click();
  await expect(page.getByTestId("settings-card")).toBeVisible();
  return {
    card: page.getByTestId("settings-card"),
    strip: page.getByTestId("settings-tabs"),
    tab: (id: (typeof TAB_IDS)[number]) => page.getByTestId(`settings-tab-${id}`),
  };
}

/** Open Settings, then shrink the window to one the tabs cannot fit in. */
async function openNarrow(app: App) {
  const parts = await openSettings(app);
  // After opening, not before: the header does not fit at 420 either.
  await app.page.setViewportSize({ width: 420, height: 800 });
  await expect(parts.strip).toHaveAttribute("data-overflow-end", "true");
  return parts;
}

const maskOf = (strip: Locator) =>
  strip.evaluate((el) => {
    const cs = getComputedStyle(el);
    return cs.maskImage || cs.webkitMaskImage || "none";
  });

const cardScroll = (card: Locator) => card.evaluate((el) => el.scrollLeft);

test("at the default window size every tab sits inside the dialog", async ({ app }) => {
  const { card, strip, tab } = await openSettings(app);

  const cardBox = await boxOf(card);
  for (const id of TAB_IDS) {
    const box = await boxOf(tab(id));
    expect(
      inside(box, cardBox),
      `${id}: ${show(box)} should be inside the dialog ${show(cardBox)}`,
    ).toBe(true);
  }

  const { scrollWidth, clientWidth } = await strip.evaluate((el) => ({
    scrollWidth: el.scrollWidth,
    clientWidth: el.clientWidth,
  }));
  expect(scrollWidth, "the strip scrolls although everything fits").toBeLessThanOrEqual(
    clientWidth,
  );
  await expect(strip).toHaveAttribute("data-overflow-end", "false");
  expect(await maskOf(strip), "no fade when nothing is cut off").toBe("none");

  await tab("plugins").click();
  await expect(app.page.getByTestId("plugins-panel")).toBeVisible();
  await expect(tab("plugins")).toHaveAttribute("aria-selected", "true");
  // A scrolled card is how the dialog slid sideways.
  expect(await cardScroll(card)).toBe(0);
});

test("in a narrow window, Tab reaches Plugins and the strip scrolls to show it", async ({
  app,
}) => {
  const { card, strip, tab } = await openNarrow(app);

  await expect(strip).toHaveAttribute("data-overflow-start", "false");
  await expect.poll(() => maskOf(strip)).not.toBe("none");

  // The premise: before scrolling, Plugins is cut off.
  const before = await boxOf(tab("plugins"));
  const stripBefore = await boxOf(strip);
  expect(
    inside(before, stripBefore),
    `Plugins ${show(before)} should start outside the strip ${show(stripBefore)}`,
  ).toBe(false);

  await tab("account").focus();
  for (let i = 0; i < 6; i += 1) await app.page.keyboard.press("Tab");
  await expect(tab("plugins")).toBeFocused();

  await expect
    .poll(async () => {
      const plugins = await boxOf(tab("plugins"));
      return inside(plugins, await boxOf(strip)) && inside(plugins, await boxOf(card));
    }, { message: "Plugins should be inside both the strip and the dialog" })
    .toBe(true);
  expect(await cardScroll(card), "focus slid the dialog, not the strip").toBe(0);

  await app.page.keyboard.press("Enter");
  await expect(app.page.getByTestId("plugins-panel")).toBeVisible();
  await expect(strip).toHaveAttribute("data-overflow-start", "true");
  await expect(strip).toHaveAttribute("data-overflow-end", "false");
});

test("in a narrow window, a sideways scroll brings Plugins within reach of the mouse", async ({
  app,
}) => {
  const { card, strip, tab } = await openNarrow(app);

  await strip.hover();
  await app.page.mouse.wheel(1000, 0);
  await expect(strip).toHaveAttribute("data-overflow-end", "false");

  const plugins = await boxOf(tab("plugins"));
  const stripBox = await boxOf(strip);
  expect(
    inside(plugins, stripBox),
    `Plugins ${show(plugins)} should be inside the strip ${show(stripBox)}`,
  ).toBe(true);

  await tab("plugins").click();
  await expect(app.page.getByTestId("plugins-panel")).toBeVisible();
  expect(await cardScroll(card)).toBe(0);
});

test("in a narrow window, a click on a tab half under the edge fade selects it", async ({
  app,
}) => {
  const { card, strip, tab } = await openNarrow(app);

  // Put Plugins' left edge 34px short of the strip's right edge, so the
  // tab is mostly under the 24px fade and the strip has room to move.
  await strip.evaluate((s) => {
    const p = s.querySelector('[data-testid="settings-tab-plugins"]')!;
    s.scrollLeft += p.getBoundingClientRect().left - (s.getBoundingClientRect().right - 34);
  });
  const stripBox = await boxOf(strip);
  await expect
    .poll(async () => Math.abs((await boxOf(tab("plugins"))).x - (stripBox.x + stripBox.width - 34)))
    .toBeLessThan(1);
  const pluginsBox = await boxOf(tab("plugins"));
  expect(
    pluginsBox.x < stripBox.x + stripBox.width &&
      pluginsBox.x + pluginsBox.width > stripBox.x + stripBox.width,
    `Plugins ${show(pluginsBox)} should cross the strip's right edge ${show(stripBox)}`,
  ).toBe(true);

  // The raw mouse, on purpose. `locator.click()` scrolls its target into
  // view first, which is exactly the thing that hides this bug: pressing
  // a tab focuses it, and if that scrolls the strip the tab slides out
  // from under the pointer before the button comes up.
  await app.page.mouse.click(
    stripBox.x + stripBox.width - 6,
    pluginsBox.y + pluginsBox.height / 2,
  );

  await expect(tab("plugins")).toHaveAttribute("aria-selected", "true");
  await expect(app.page.getByTestId("plugins-panel")).toBeVisible();
  await expect
    .poll(async () => inside(await boxOf(tab("plugins")), await boxOf(strip)))
    .toBe(true);
  expect(await cardScroll(card)).toBe(0);
});
