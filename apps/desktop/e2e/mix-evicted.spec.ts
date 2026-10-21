/**
 * A mix the preview cache evicted plays again after a tab switch (#607).
 *
 * The transport plays `render_preview`'s file, an entry in the project's
 * bounded preview cache. The Graph and Transcript tabs unmount the
 * timeline, and coming back loads the file again. The backend now holds the
 * entry the window plays, so the agent's renders cannot evict it; this is
 * the second line, for a file that is gone anyway (a cache cleared by hand,
 * or one held by nothing after Save As): the load that finds nothing asks
 * for the mix again, once, and plays what comes back instead of leaving
 * "Could not load the mix" on a mix the user was just playing.
 *
 * Run in real Chromium: the 404 is the asset route's answer for a file
 * that is not there, as Tauri's asset protocol gives one, and WaveSurfer's
 * own message for it is what the app reads. jsdom has neither.
 *
 * The mix here is a file of this test's own, copied from a fixture, so it
 * can be taken away and put back (the render "writes it again" once the
 * test says so, by holding the answer until it has).
 */

import { copyFileSync, mkdirSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";

import type { Page } from "@playwright/test";

import { FIXTURE_DIR, fixturePath } from "./audio-fixtures";
import { deferred, nodeId, ok, projectWith, toneTrack } from "./backend";
import { expect, test, type App } from "./fixtures";

const HEAD = nodeId(1);

/** The mix as the browser has it: the side that holds a file, once one does. */
function mix(page: Page): Promise<{ loaded: number; paused: boolean; time: number }> {
  return page.locator("[data-testid=timeline-mix-player] audio").evaluateAll((els) => {
    const loaded = (els as HTMLAudioElement[]).filter((a) => a.src);
    const a = loaded[0];
    return { loaded: loaded.length, paused: a ? a.paused : true, time: a ? a.currentTime : 0 };
  });
}

/** Play, until it is part of the way through the mix. */
async function expectPlaying(page: Page): Promise<void> {
  await expect(page.getByTestId("play-pause-button")).toHaveAccessibleName("Pause");
  await expect.poll(async () => (await mix(page)).time, { message: "the mix plays" }).toBeGreaterThan(0.2);
  expect((await mix(page)).paused).toBe(false);
}

/** Pause, so teardown finds the app quiet. */
async function stop(page: Page): Promise<void> {
  const button = page.getByTestId("play-pause-button");
  if ((await button.getAttribute("aria-label")) === "Pause") await button.click();
}

/**
 * Open a project and play its head from a mix file of this test's own,
 * then stop. The file stands for the cache entry `render_preview` names.
 */
async function playedOnce(app: App) {
  const mixFile = join(
    FIXTURE_DIR,
    "evictable",
    `mix-${process.pid}-${test.info().parallelIndex}-${Date.now()}.wav`,
  );
  const restore = () => {
    mkdirSync(dirname(mixFile), { recursive: true });
    copyFileSync(fixturePath("tone3s"), mixFile);
  };
  restore();
  await app.boot({
    ...projectWith([toneTrack()], HEAD),
    // `get_graph`: a load, as the Graph tab draws it.
    get_graph: ok({
      nodes: [{ id: HEAD, parent: null, label: "load", tool: "load", created_at: "2026-10-20T00:00:00Z" }],
      head: HEAD,
    }),
    render_preview: ok(mixFile),
  });
  const page = app.page;
  await expect(page.getByTestId("ruler")).toContainText("0:03");

  await page.getByTestId("play-pause-button").click();
  await expectPlaying(page);
  expect(await app.requestsFor("render_preview")).toEqual([{ node: HEAD }]);
  await stop(page);
  await expect(page.getByTestId("play-pause-button")).toHaveAccessibleName("Play");

  return { page, mixFile, restore };
}

/** Away to the Graph tab and back, which unmounts the timeline and mounts it again. */
async function switchTabsAndBack(page: Page): Promise<void> {
  await page.getByTestId("tab-graph").click();
  await expect(page.getByTestId("graph-bubble")).toHaveCount(1);
  await page.getByTestId("tab-timeline").click();
}

test.describe("a mix the cache evicted", () => {
  test("is rendered again on the way back to the timeline, and plays", async ({ app }) => {
    const { page, mixFile, restore } = await playedOnce(app);
    try {
      // The cache evicts the entry while the user is on another tab.
      rmSync(mixFile);
      // The render writes it again, when the test lets it.
      await app.become({ render_preview: deferred("re-render") });

      await switchTabsAndBack(page);

      // The load found nothing, so the app asked for the mix once more,
      // for the node it shows, and nothing says it failed meanwhile.
      await expect
        .poll(() => app.requestsFor("render_preview"), { message: "the mix is asked for again" })
        .toEqual([{ node: HEAD }, { node: HEAD }]);
      await expect(page.getByTestId("timeline-mix-error")).toHaveCount(0);
      restore();
      await app.release("re-render", mixFile);

      // It loads the file that is there again, and plays it.
      await expect.poll(async () => (await mix(page)).loaded, { message: "the mix loaded" }).toBe(1);
      await expect(page.getByTestId("timeline-mix-error")).toHaveCount(0);
      await page.getByTestId("play-pause-button").click();
      await expectPlaying(page);
      expect(await app.requestsFor("render_preview"), "one render for the loss, and no other").toHaveLength(2);
      await stop(page);
    } finally {
      rmSync(mixFile, { force: true });
    }
  });

  test("that the render does not bring back is an error, after one try", async ({ app }) => {
    const { page, mixFile } = await playedOnce(app);
    try {
      // Taken away, and the render answers the same name without writing it.
      rmSync(mixFile);

      await switchTabsAndBack(page);

      await expect(page.getByTestId("timeline-mix-error")).toContainText("Could not load the mix");
      // Settled: the app has stopped asking. A loop would not have.
      await app.settle();
      expect(await app.requestsFor("render_preview"), "the one try, and no more").toEqual([
        { node: HEAD },
        { node: HEAD },
      ]);
      await expect(page.getByTestId("timeline-mix-error")).toBeVisible();
    } finally {
      rmSync(mixFile, { force: true });
    }
  });

  test("is not rendered again while the file is there", async ({ app }) => {
    const { page, mixFile } = await playedOnce(app);
    try {
      await switchTabsAndBack(page);

      await expect.poll(async () => (await mix(page)).loaded, { message: "the mix loaded again" }).toBe(1);
      await app.settle();
      expect(await app.requestsFor("render_preview"), "a file that is there is loaded, not rendered").toEqual([
        { node: HEAD },
      ]);
      await expect(page.getByTestId("timeline-mix-error")).toHaveCount(0);
    } finally {
      rmSync(mixFile, { force: true });
    }
  });
});
