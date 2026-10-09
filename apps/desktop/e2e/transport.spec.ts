/**
 * Play, pause and place the playhead with the mouse (#425).
 *
 * The transport was keyboard-only. Space toggled it and Home/End and the
 * arrows moved it; nothing on screen did either, and a click on a lane
 * only cleared the selection.
 *
 * Read from the two `<audio>` elements the mix plays through, in real
 * Chromium media playback, and from the playheads as drawn. jsdom plays
 * nothing and lays nothing out, so a button that flipped its own label,
 * or a click that moved one lane's line, would pass there.
 */

import type { Page } from "@playwright/test";

import { fixturePath, fixtureSeconds } from "./audio-fixtures";
import { nodeId, ok, readyToLoad, selecting, sessionWith, trackFor } from "./backend";
import { expect, test, type App } from "./fixtures";
import type { TrackSummary } from "../src/lib/tauri-bridge";
import { dragSelect, laneSurface } from "./timeline";

const SESSION = fixtureSeconds("tone3s");

const long = () => trackFor(fixturePath("tone3s"), SESSION, { name: "Long" });
/** One second of tone at zero: a second lane, shorter than the session. */
const short = () =>
  trackFor(fixturePath("tone1s"), 1, {
    name: "Short",
    id: "5a6b7c8d-9e0f-4a1b-8c2d-3e4f5a6b7c8d",
  });

/**
 * Within a few pixels of where the pointer went down, at fit: the pane is
 * several hundred pixels across three seconds.
 */
const SLACK = 0.02;

/** The mix the transport plays: a render of the head. */
const RENDER = { render_preview: ok(fixturePath("tone3s")) };

async function load(app: App, tracks: TrackSummary[]): Promise<void> {
  await app.boot({ ...readyToLoad(), ...selecting(), ...RENDER });
  await app.become({ ...sessionWith(tracks), ...RENDER });
  await app.emit("agent://node-created", { node_id: nodeId(1) });
  await expect(app.page.getByTestId("timeline-lane-waveform")).toHaveCount(tracks.length);
  await expect(app.page.getByTestId("ruler")).toContainText("0:03");
}

/** Render the mix, and wait for the transport to hold it. */
async function preview(page: Page): Promise<void> {
  await page.getByTestId("render-preview-button").click();
  await expect(page.getByTestId("play-pause-button")).toBeEnabled();
}

/** The mix as the browser has it: the side that holds a file. */
function mix(page: Page): Promise<{ paused: boolean; ended: boolean; time: number }> {
  return page.locator("[data-testid=timeline-mix-player] audio").evaluateAll((els) => {
    const loaded = (els as HTMLAudioElement[]).filter((a) => a.src);
    if (loaded.length !== 1) throw new Error(`${loaded.length} mix players hold a file`);
    const a = loaded[0];
    return { paused: a.paused, ended: a.ended, time: a.currentTime };
  });
}

/**
 * Paused by the gesture just made, not stopped by the mix running out.
 *
 * The mix is three seconds long and an assertion waits five, so a
 * gesture that failed to pause — or paused and played again — would
 * still find the mix paused, and the button on Play, once it ended.
 */
async function expectPausedMidway(page: Page): Promise<void> {
  const now = await mix(page);
  expect(now.paused, "the mix is still playing").toBe(true);
  expect(now.ended, "the mix played to its end instead of pausing").toBe(false);
}

/** Click a lane at a fraction of its width; returns the time under the pointer. */
async function clickLane(page: Page, lane: number, at: number): Promise<{ x: number; sec: number }> {
  const box = await laneSurface(page, lane).boundingBox();
  if (!box) throw new Error("the lane has no box");
  const x = box.x + box.width * at;
  await page.mouse.click(x, box.y + box.height / 2);
  return { x, sec: at * SESSION };
}

/** Every lane's playhead sits on `sec`, drawn at `x`. */
async function expectPlayheadsAt(page: Page, count: number, x: number, sec: number): Promise<void> {
  await expect(async () => {
    const heads = await page.getByTestId("timeline-playhead").all();
    expect(heads).toHaveLength(count);
    for (const head of heads) {
      const at = Number(await head.getAttribute("data-playhead-sec"));
      expect(Math.abs(at - sec), `a playhead reads ${at}`).toBeLessThanOrEqual(SLACK);
      expect(Math.abs((await head.boundingBox())!.x - x)).toBeLessThanOrEqual(2);
    }
  }).toPass();
}

test.describe("the play button", () => {
  test("waits for a mix, then plays it and pauses it", async ({ app }) => {
    await load(app, [long()]);
    const page = app.page;
    const button = page.getByTestId("play-pause-button");

    // The lanes are only pictures; before a preview there is nothing to play.
    await expect(button).toBeVisible();
    await expect(button).toBeDisabled();

    await preview(page);
    await expect(button).toHaveAccessibleName("Play");

    await button.click();
    await expect(button).toHaveAccessibleName("Pause");
    await expect.poll(async () => (await mix(page)).time, { message: "the mix plays" }).toBeGreaterThan(0.2);
    expect((await mix(page)).paused).toBe(false);

    await button.click();
    await expect(button).toHaveAccessibleName("Play");
    await expectPausedMidway(page);
  });

  test("Space still toggles it, and toggles it once with the button focused", async ({ app }) => {
    await load(app, [long()]);
    const page = app.page;
    const button = page.getByTestId("play-pause-button");
    await preview(page);

    // Clicked, the button keeps focus — and Space on a focused button is
    // also a click. Toggled twice, it would be playing again.
    await button.click();
    await expect(button).toHaveAccessibleName("Pause");
    await page.keyboard.press("Space");
    await expect(button).toHaveAccessibleName("Play");
    await expectPausedMidway(page);

    // And from nowhere in particular, as before: the button follows.
    await button.evaluate((el) => (el as HTMLButtonElement).blur());
    await page.keyboard.press("Space");
    await expect(button).toHaveAccessibleName("Pause");
    expect((await mix(page)).paused).toBe(false);
    await page.keyboard.press("Space");
    await expect(button).toHaveAccessibleName("Play");
    await expectPausedMidway(page);
  });
});

test.describe("a click on a lane", () => {
  test("puts every lane's playhead, and the mix, where it landed", async ({ app }) => {
    await load(app, [long(), short()]);
    const page = app.page;
    await preview(page);

    // On the second lane, which cannot select, and past the end of its
    // own audio: the session's time, not the lane's.
    const { x, sec } = await clickLane(page, 1, 0.5);

    await expectPlayheadsAt(page, 2, x, sec);
    const played = await mix(page);
    expect(Math.abs(played.time - sec)).toBeLessThanOrEqual(SLACK);
    expect(played.paused, "a click while stopped does not start playback").toBe(true);
  });

  test("clears the selection, where a drag still makes one", async ({ app }) => {
    await load(app, [long()]);
    const page = app.page;
    await preview(page);

    const sel = await dragSelect(app, 0.2, 0.4);
    expect(sel.end - sel.start).toBeGreaterThan(0.5);
    await expect(page.getByTestId("timeline-selection-overlay")).toHaveCount(1);
    // A drag selects; it does not move the playhead.
    expect((await mix(page)).time).toBe(0);

    const { x, sec } = await clickLane(page, 0, 0.7);

    await expect(page.getByTestId("timeline-selection-overlay")).toHaveCount(0);
    await expect
      .poll(async () => {
        const sent = (await app.requestsFor("set_selection_context")) as { range: unknown }[];
        return sent[sent.length - 1].range;
      })
      .toBeNull();
    await expectPlayheadsAt(page, 1, x, sec);
  });

  test("while playing, seeks and plays on from there", async ({ app }) => {
    await load(app, [long()]);
    const page = app.page;
    const button = page.getByTestId("play-pause-button");
    await preview(page);

    // Every seek the media element starts, with whether it was playing.
    // `seeking` rather than `seeked`: by the time a seek has finished,
    // playback has already carried the position on past where it landed.
    await page.locator("[data-testid=timeline-mix-player] audio").evaluateAll((els) => {
      const seeks: { time: number; paused: boolean }[] = [];
      (window as unknown as { __SEEKS__: typeof seeks }).__SEEKS__ = seeks;
      for (const el of els as HTMLAudioElement[]) {
        el.addEventListener("seeking", () => seeks.push({ time: el.currentTime, paused: el.paused }));
      }
    });
    const seeks = () =>
      page.evaluate(() => (window as unknown as { __SEEKS__: { time: number; paused: boolean }[] }).__SEEKS__);

    await button.click();
    await expect.poll(async () => (await mix(page)).time).toBeGreaterThan(0.1);
    const before = (await mix(page)).time;

    const { sec } = await clickLane(page, 0, 0.7);
    expect(before, "playback alone could reach the click's time").toBeLessThan(sec - 0.5);

    await expect.poll(async () => (await seeks()).length).toBeGreaterThanOrEqual(1);
    const [seek] = await seeks();
    // At or just after the click's time — never before it.
    expect(seek.time).toBeGreaterThanOrEqual(sec - SLACK);
    expect(seek.time).toBeLessThanOrEqual(sec + 0.25);
    expect(seek.paused, "the click stopped playback").toBe(false);
    await expect(button).toHaveAccessibleName("Pause");
    // Plays on from the new place...
    await expect.poll(async () => (await mix(page)).time).toBeGreaterThan(sec + 0.1);
    // ...to the end of the mix, where the button goes back to Play.
    await expect(button).toHaveAccessibleName("Play", { timeout: 5_000 });
    expect(await mix(page)).toMatchObject({ paused: true, ended: true });
  });
});
