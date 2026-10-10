/**
 * The Settings dialog fits the window it opens in, and Save is on screen.
 *
 * The release candidate of e2b4c30 could not be set up on a Linux box
 * with no Secret Service. The window opens 1280 x 800, which leaves the
 * webview 775 px tall, and the first-run card was 874: it is centred in a
 * `fixed inset-0` container and is `overflow-hidden` with no height cap,
 * so it ran from y = -49 to 825 with Save & Continue at 776-808, below
 * the bottom edge. Nothing scrolled, so a new user could not save a key.
 * Two notes had grown it past the window: the session-only storage
 * warning (#477, 74 px) and the "Showing edytlab's built-in list" note
 * under the model field (#504, 30 px). Either alone was already a card
 * taller than the 1024 x 640 a window may be dragged down to.
 *
 * jsdom has no layout, so none of this can be seen there. These run the
 * real frontend in Chromium, with the backend answering what a machine
 * with no Secret Service answers, and check boxes against the window:
 *
 * - the card is inside it, with a margin, and Save is in it and clicks;
 * - Tab walks every field into view and ends on Save, and a field the
 *   Tab reaches is never left under the footer;
 * - the same in the Settings panel, on every tab.
 *
 * `tauri.conf.json` gives the window a size, 1280 x 800, and no minimum
 * at all, so the smallest window is whatever the user drags it to. 1024 x
 * 640 is the smallest this is designed for. A window is taller than its
 * webview: the release candidate's 800 px window had a 775 px viewport, 25
 * px of title bar, so the smallest is checked at 640 and again at 615.
 */

import type { Locator, Page } from "@playwright/test";

import {
  firstRun,
  readyToLoad,
  savingSettings,
  sessionOnlyKeychain,
  settingsTabs,
} from "./backend";
import { expect, test, type App } from "./fixtures";

const WINDOWS = [
  { label: "the default window (1280x800)", width: 1280, height: 775 },
  { label: "the smallest window (1024x640)", width: 1024, height: 640 },
  { label: "the smallest window under a title bar (1024x615 viewport)", width: 1024, height: 615 },
] as const;

/** The card keeps at least this far from every edge of the window. */
const MARGIN = 8;

const KEY = "sk-ant-test";

interface Span {
  top: number;
  bottom: number;
}

async function spanOf(locator: Locator, what: string): Promise<Span> {
  const box = await locator.boundingBox();
  if (!box) throw new Error(`${what} has no box: it is not rendered`);
  return { top: box.y, bottom: box.y + box.height };
}

function show(s: Span): string {
  return `y ${s.top.toFixed(1)}..${s.bottom.toFixed(1)}`;
}

/** The webview's height, which is what the dialog has to fit in. */
function heightOf(page: Page): number {
  const size = page.viewportSize();
  if (!size) throw new Error("the page has no viewport size");
  return size.height;
}

/** `what` lies inside the window, `margin` px clear of its top and bottom. */
async function expectInWindow(page: Page, locator: Locator, what: string, margin = 0) {
  const span = await spanOf(locator, what);
  const h = heightOf(page);
  expect(
    span.top >= margin && span.bottom <= h - margin,
    `${what} spans ${show(span)}, but the window is ${h} px tall` +
      (margin ? ` (and it should keep ${margin} px clear of the edges)` : ""),
  ).toBe(true);
}

/**
 * First launch of a machine with no Secret Service, with the dialog up
 * and both notes that made it tall on screen, in the shipped face (the
 * line breaks, and so the heights, are the app's, not a fallback's).
 */
async function openFirstRun(app: App) {
  await app.boot({ ...firstRun(), ...sessionOnlyKeychain(), ...savingSettings() });
  const page = app.page;
  const dialog = page.getByRole("dialog", { name: "Settings" });
  await expect(dialog).toHaveAttribute("data-mode", "blocking");
  await expect(page.getByTestId("settings-storage-warning")).toBeVisible();
  // `list_models_for` with no key: the built-in list, said so under the field.
  await expect(page.getByTestId("settings-model-fallback")).toBeVisible();
  await expect(page.getByTestId("settings-effort-select")).toBeVisible();
  await page.evaluate(() => document.fonts.load('400 14px "Geist Variable"'));
  return {
    page,
    dialog,
    card: page.getByTestId("settings-card"),
    save: page.getByTestId("settings-save-button"),
  };
}

/** The same machine, a returning user, the dialog opened from the gear. */
async function openPanel(app: App) {
  await app.boot({
    ...readyToLoad(),
    ...sessionOnlyKeychain(),
    ...savingSettings(),
    ...settingsTabs(),
  });
  const page = app.page;
  await expect(page.getByTestId("empty-state")).toBeVisible();
  await page.evaluate(() => document.fonts.load('400 14px "Geist Variable"'));
  await page.getByTestId("open-settings-button").click();
  const dialog = page.getByRole("dialog", { name: "Settings" });
  await expect(dialog).toHaveAttribute("data-mode", "panel");
  await expect(page.getByTestId("settings-storage-warning")).toBeVisible();
  await expect(page.getByTestId("settings-model-fallback")).toBeVisible();
  return {
    page,
    dialog,
    card: page.getByTestId("settings-card"),
    save: page.getByTestId("settings-save-button"),
  };
}

for (const win of WINDOWS) {
  test.describe(win.label, () => {
    test.use({ viewport: { width: win.width, height: win.height } });

    test("first run: the card fits, Save & Continue is on screen, and clicking it saves", async ({
      app,
    }) => {
      const { page, dialog, card, save } = await openFirstRun(app);

      await expectInWindow(page, save, "Save & Continue");
      await expect(save).toBeInViewport({ ratio: 1 });
      await expectInWindow(page, card, "the Welcome card", MARGIN);
      await expectInWindow(
        page,
        page.getByRole("heading", { name: "Welcome to edytlab" }),
        "the Welcome heading",
      );

      // A real click: Playwright scrolls what a user could scroll, checks
      // that the button is hit where it is clicked, and clicks there.
      await page.getByTestId("settings-key-input").fill(KEY);
      await save.click();

      await expect(dialog).toHaveCount(0);
      expect(await app.requestsFor("set_api_key_for")).toEqual([
        { provider: "anthropic", key: KEY },
      ]);
    });

    test("first run: Tab brings each field into view and ends on Save & Continue", async ({
      app,
    }) => {
      const { page, dialog, card, save } = await openFirstRun(app);
      const height = heightOf(page);

      await page.getByTestId("settings-key-input").fill(KEY);

      // Where the focused element is, and where the footer starts. A field
      // under the footer is focused but cannot be seen or typed into.
      const focused = () =>
        page.evaluate(() => {
          const el = document.activeElement as HTMLElement;
          const r = el.getBoundingClientRect();
          const footer = document
            .querySelector('[data-testid="settings-footer"]')
            ?.getBoundingClientRect();
          return {
            id: el.dataset.testid ?? el.tagName,
            top: r.top,
            bottom: r.bottom,
            footerTop: footer ? footer.top : null,
            inFooter: !!el.closest('[data-testid="settings-footer"]'),
          };
        });

      const visited: string[] = [];
      for (let i = 0; i < 12; i += 1) {
        await page.keyboard.press("Tab");
        const at = await focused();
        visited.push(at.id);
        expect(
          at.top >= 0 && at.bottom <= height,
          `${at.id} has focus at y ${at.top.toFixed(1)}..${at.bottom.toFixed(1)}, ` +
            `outside the ${height} px window (visited: ${visited.join(" > ")})`,
        ).toBe(true);
        if (!at.inFooter && at.footerTop !== null) {
          expect(
            at.bottom <= at.footerTop + 0.5,
            `${at.id} has focus under the footer: it ends at y ${at.bottom.toFixed(1)}, ` +
              `the footer starts at ${at.footerTop.toFixed(1)}`,
          ).toBe(true);
        }
        // The dialog stays where it is. A card taller than the window is
        // reached by sliding the page under it, which is how Save used to
        // come into view for a Tab and not for a pointer (#392 has the
        // same fault sideways).
        await expectInWindow(page, card, `the card with ${at.id} focused`, MARGIN);
        if (at.id === "settings-save-button") break;
      }
      expect(
        await page.evaluate(() => document.scrollingElement?.scrollTop ?? 0),
        "Tab scrolled the whole page to reach a field",
      ).toBe(0);

      // Every field the Tab passes, in the order the form draws them.
      expect(visited).toEqual([
        "A", // How to get a key
        "settings-test-button",
        "settings-base-url-input",
        "settings-model-input",
        "settings-effort-select",
        "settings-save-button",
      ]);
      await expect(save).toBeFocused();
      await expect(save).toBeInViewport({ ratio: 1 });

      await page.keyboard.press("Enter");
      await expect(dialog).toHaveCount(0);
      expect(await app.requestsFor("set_api_key_for")).toEqual([
        { provider: "anthropic", key: KEY },
      ]);
    });

    test("panel: the card fits and Save, Clear key and Close are on screen", async ({ app }) => {
      const { page, dialog, card, save } = await openPanel(app);

      await expectInWindow(page, save, "Save & Continue");
      await expect(save).toBeInViewport({ ratio: 1 });
      await expectInWindow(page, page.getByTestId("settings-clear-button"), "Clear key");
      await expectInWindow(page, card, "the Settings card", MARGIN);
      await expectInWindow(page, page.getByRole("heading", { name: "Settings" }), "the heading");
      await expectInWindow(page, page.getByTestId("settings-close"), "Close");
      await expectInWindow(page, page.getByTestId("settings-tabs"), "the tab strip");

      await page.getByTestId("settings-key-input").fill(KEY);
      await save.click();
      await expect(dialog).toHaveCount(0);
      expect(await app.requestsFor("set_api_key_for")).toEqual([
        { provider: "anthropic", key: KEY },
      ]);
    });

    test("panel: every tab fits, and what is at the end of it can be scrolled to", async ({
      app,
    }) => {
      const { page, card } = await openPanel(app);

      // The last thing on each tab: the control a user has to reach.
      const END_OF: Record<string, string> = {
        account: "settings-save-button",
        project: "project-save",
        memory: "memory-save-project",
        skills: "skills-editor",
        agents: "agent-profiles-editor",
        mcp: "mcp-servers-editor",
        plugins: "plugin-install-btn",
      };

      for (const [tab, endId] of Object.entries(END_OF)) {
        // Before the click: a strip above the window cannot be clicked.
        await expectInWindow(page, card, `the card before the ${tab} tab`, MARGIN);
        await expectInWindow(page, page.getByTestId("settings-tabs"), `the tab strip for ${tab}`);
        await page.getByTestId(`settings-tab-${tab}`).click();
        // A tab opens at its top, whatever the one before it was scrolled to.
        expect(
          await page.getByTestId("settings-body").evaluate((el) => el.scrollTop),
          `the ${tab} tab opened scrolled down`,
        ).toBe(0);
        const end = page.getByTestId(endId);
        await expect(end, `the ${tab} tab shows ${endId}`).toBeVisible();

        await expectInWindow(page, card, `the card on the ${tab} tab`, MARGIN);

        // The wheel, as a user has it: over the dialog, not a scrollIntoView
        // that would move an `overflow-hidden` box no one can scroll. It
        // scrolls smoothly, so the end is waited for rather than assumed.
        const cardBox = await card.boundingBox();
        await page.mouse.move(cardBox!.x + cardBox!.width / 2, cardBox!.y + cardBox!.height / 2);
        await page.mouse.wheel(0, 4_000);

        const h = heightOf(page);
        const deadline = Date.now() + 5_000;
        let span = await spanOf(end, `${endId} on ${tab}`);
        while ((span.top < 0 || span.bottom > h) && Date.now() < deadline) {
          await page.waitForTimeout(100);
          span = await spanOf(end, `${endId} on ${tab}`);
        }
        expect(
          span.top >= 0 && span.bottom <= h,
          `${endId} on the ${tab} tab sits at ${show(span)} after scrolling to the end, ` +
            `but the window is ${h} px tall`,
        ).toBe(true);
      }
    });
  });
}
