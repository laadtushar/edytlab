/**
 * Play renders the preview when there is none, then plays (#431).
 *
 * Playback plays the rendered preview mix, and the only thing that
 * rendered it was the Preview button. So the ordinary loop — open a
 * file, or let the assistant edit, then listen — was dead after every
 * edit: Space did nothing, the Play button was disabled, and nothing on
 * screen said that Preview was the missing step.
 *
 * Read from the `<audio>` elements the mix plays through and from the
 * playheads as drawn, in real Chromium media playback. jsdom plays
 * nothing, so a button that flipped its own label would pass there.
 *
 * Every test here presses Play (or Space) without ever pressing Preview,
 * except the ones that say otherwise.
 */

import type { Page } from "@playwright/test";

import { fixturePath, fixtureSeconds } from "./audio-fixtures";
import {
  deferred,
  nodeId,
  ok,
  readyToLoad,
  reject,
  selecting,
  sessionWith,
  trackFor,
} from "./backend";
import { expect, test, type App } from "./fixtures";

const TONE = fixturePath("tone3s");
const TONE_B = fixturePath("tone3sB");
const SESSION = fixtureSeconds("tone3s");

const track = () => trackFor(TONE, SESSION, { name: "Tone" });

/** Open a three-second file the way a user does: loaded, never previewed. */
async function open(app: App, answers: Parameters<App["boot"]>[0] = {}): Promise<void> {
  await app.boot({ ...readyToLoad(), ...selecting(), ...answers });
  await app.become({ ...sessionWith([track()]), ...answers });
  await app.emit("agent://node-created", { node_id: nodeId(1) });
  await expect(app.page.getByTestId("timeline-lane-waveform")).toHaveCount(1);
  await expect(app.page.getByTestId("ruler")).toContainText("0:03");
}

/** The mix as the browser has it: the side that holds a file, once one does. */
function mix(page: Page): Promise<{ loaded: number; paused: boolean; ended: boolean; time: number }> {
  return page.locator("[data-testid=timeline-mix-player] audio").evaluateAll((els) => {
    const loaded = (els as HTMLAudioElement[]).filter((a) => a.src);
    const a = loaded[0];
    return {
      loaded: loaded.length,
      paused: a ? a.paused : true,
      ended: a ? a.ended : false,
      time: a ? a.currentTime : 0,
    };
  });
}

/** Where the first lane's playhead is drawn, in session seconds. */
async function playheadSec(page: Page): Promise<number> {
  return Number(await page.getByTestId("timeline-playhead").first().getAttribute("data-playhead-sec"));
}

/** Playing, and part of the way through — not stopped by the mix running out. */
async function expectPlayingFromTheStart(page: Page): Promise<void> {
  const button = page.getByTestId("play-pause-button");
  await expect(button).toHaveAccessibleName("Pause");
  await expect.poll(async () => (await mix(page)).time, { message: "the mix plays" }).toBeGreaterThan(0.2);
  await expect.poll(() => playheadSec(page), { message: "the playhead advances" }).toBeGreaterThan(0.2);
  expect((await mix(page)).paused).toBe(false);
}

/** Pause, so teardown finds the app quiet. */
async function stop(page: Page): Promise<void> {
  const button = page.getByTestId("play-pause-button");
  if ((await button.getAttribute("aria-label")) === "Pause") await button.click();
}

test.describe("Play with no preview", () => {
  test("is offered, renders the head once, and plays from the start", async ({ app }) => {
    await open(app, { render_preview: ok(TONE) });
    const page = app.page;
    const button = page.getByTestId("play-pause-button");

    // Before anything is rendered: Play is there to press, and the status
    // bar says how to hear the session.
    await expect(button).toBeEnabled();
    await expect(button).toHaveAccessibleName("Play");
    await expect(page.getByTestId("status-bar-mix-missing")).toContainText("press Play or Preview");
    expect(await app.requestsFor("render_preview")).toEqual([]);

    await button.click();

    await expectPlayingFromTheStart(page);
    expect(await app.requestsFor("render_preview"), "one render, of the head").toEqual([{ node: nodeId(1) }]);
    await expect(page.getByTestId("status-bar-mix-missing")).toHaveCount(0);
    await expect(button).not.toHaveAttribute("aria-busy", "true");
    await stop(page);
  });

  test("Space does the same", async ({ app }) => {
    await open(app, { render_preview: ok(TONE) });
    const page = app.page;

    await page.keyboard.press("Space");

    await expectPlayingFromTheStart(page);
    expect(await app.requestsFor("render_preview")).toEqual([{ node: nodeId(1) }]);
    await stop(page);
  });

  test("after an edit, renders the edited head instead of playing the old mix", async ({ app }) => {
    await open(app, { render_preview: ok(TONE) });
    const page = app.page;
    const button = page.getByTestId("play-pause-button");

    // Listen once, as a user would: the mix for the first head is loaded.
    await button.click();
    await expectPlayingFromTheStart(page);
    await button.click();
    await expect(button).toHaveAccessibleName("Play");
    expect(await app.requestsFor("render_preview")).toEqual([{ node: nodeId(1) }]);

    // An edit lands. Its mix is another file; the first one is stale.
    await app.become({ ...sessionWith([track()]), render_preview: ok(TONE_B) });
    await app.emit("agent://node-created", { node_id: nodeId(2) });
    await expect(page.getByTestId("status-bar-mix-missing")).toContainText("press Play or Preview");

    await page.keyboard.press("Space");

    await expect
      .poll(() => app.requestsFor("render_preview"), { message: "a render of the edited head" })
      .toEqual([{ node: nodeId(1) }, { node: nodeId(2) }]);
    await expectPlayingFromTheStart(page);
    await stop(page);
  });

  test("shows that it is rendering, and a second press cancels the play", async ({ app }) => {
    await open(app, { render_preview: deferred("render") });
    const page = app.page;
    const button = page.getByTestId("play-pause-button");

    await button.click();

    // Rendering: busy, named for it, and not offering to start again.
    await expect(button).toHaveAttribute("aria-busy", "true");
    await expect(button).toHaveAccessibleName("Rendering…");
    await expect(button).toBeEnabled();
    await expect(page.getByTestId("status-bar-state")).toContainText("rendering");
    // The hint would be wrong now — something is already being done about it.
    await expect(page.getByTestId("status-bar-mix-missing")).toHaveCount(0);

    // Pressed again, it is a cancel, not a second render.
    await button.click();
    await expect(button).not.toHaveAttribute("aria-busy", "true");
    await expect(button).toHaveAccessibleName("Play");

    // The render still finishes and the mix loads; nothing plays.
    await app.release("render", TONE);
    await expect.poll(async () => (await mix(page)).loaded, { message: "the mix loaded" }).toBe(1);
    await expect(page.getByTestId("status-bar-state")).toContainText("ready");
    await expect(button).toHaveAccessibleName("Play");
    expect((await mix(page)).paused).toBe(true);
    expect((await mix(page)).time).toBe(0);
    expect(await app.requestsFor("render_preview"), "cancelling is not a second render").toEqual([
      { node: nodeId(1) },
    ]);
  });

  test("a failed render shows its error and plays nothing", async ({ app }) => {
    await open(app, { render_preview: reject("render failed: out of disk space") });
    const page = app.page;
    const button = page.getByTestId("play-pause-button");

    await button.click();

    await expect(page.getByTestId("render-error")).toContainText("out of disk space");
    await expect(button).toHaveAccessibleName("Play");
    await expect(button).not.toHaveAttribute("aria-busy", "true");
    expect((await mix(page)).loaded).toBe(0);
    // And it says how to try again, rather than going quiet.
    await expect(page.getByTestId("status-bar-mix-missing")).toBeVisible();

    // The next try works once the backend can render.
    await app.become({ render_preview: ok(TONE) });
    await button.click();
    await expectPlayingFromTheStart(page);
    await stop(page);
  });

  test("with a current mix, plays at once and does not render again", async ({ app }) => {
    await open(app, { render_preview: ok(TONE) });
    const page = app.page;
    const button = page.getByTestId("play-pause-button");

    // Preview is still its own control.
    await page.getByTestId("render-preview-button").click();
    await expect.poll(async () => (await mix(page)).loaded).toBe(1);
    await expect(page.getByTestId("status-bar-mix-missing")).toHaveCount(0);

    await button.click();

    await expectPlayingFromTheStart(page);
    expect(await app.requestsFor("render_preview"), "Preview's render, and no other").toEqual([
      { node: nodeId(1) },
    ]);
    await stop(page);
  });

  test("while Preview is rendering, Play waits for it and then plays", async ({ app }) => {
    await open(app, { render_preview: deferred("render") });
    const page = app.page;
    const button = page.getByTestId("play-pause-button");

    await page.getByTestId("render-preview-button").click();
    await expect(page.getByTestId("status-bar-state")).toContainText("rendering");
    await button.click();
    await expect(button).toHaveAttribute("aria-busy", "true");

    await app.release("render", TONE);

    await expectPlayingFromTheStart(page);
    expect(await app.requestsFor("render_preview"), "Play joined Preview's render").toEqual([
      { node: nodeId(1) },
    ]);
    await stop(page);
  });
});
