/**
 * S7. The mix stays playable (#607).
 *
 * Play, switch to the Graph tab, switch back, play again.
 *
 * The transport plays `render_preview`'s file, an entry in the project's
 * preview cache. The Graph tab unmounts the timeline and coming back loads
 * that file again. Before #607 the cache could evict the entry meanwhile and
 * the user got "Could not load the mix" on a mix they had just played. Now
 * the backend holds the entry the window plays, and a load that finds nothing
 * asks for the mix once more and plays what comes back.
 *
 * This shot exercises the second line, as `e2e/mix-evicted.spec.ts` does: on
 * the Graph tab the mix's file is removed (a cache cleared by hand), so the
 * load on the way back gets the asset route's 404, the app asks
 * `render_preview` again, once, and the scripted backend writes the file and
 * answers. Then Play plays it. Nothing on screen but the Play button and the
 * playhead says any of that happened, which is the point.
 */

import { rmSync } from "node:fs";

import { expect, test } from "../../../apps/desktop/e2e/fixtures";
import { deferred, projectWith } from "../../../apps/desktop/e2e/backend";

import { LENGTH_SEC, fadeIn, mix, stems, writeFixture } from "./lib/music";
import { shotAudio, writeShotWav } from "./lib/media";
import { writeNotes } from "./lib/notes";
import { SCALE, VIEWPORT, Stage } from "./lib/stage";
import { Studio, loadedTrack } from "./lib/studio";

test.use({ viewport: VIEWPORT, deviceScaleFactor: SCALE });

const NAME = "s7-mix-stays-playable";

test(NAME, async ({ app }) => {
  const page = app.page;
  const stage = await Stage.prepare(app);

  const s = stems();
  const faded = fadeIn(s.backing, 0, 4);
  const mixAfter = mix([{ samples: faded }, { samples: s.vocal, gainDb: -3 }]);
  const backing = writeFixture("Music/demo-song/backing.wav", s.backing);
  const vocal = writeFixture("Music/demo-song/vocal.wav", s.vocal);
  const fadedPath = writeFixture("Music/demo-song/.audiograph/derived/faded-backing.wav", faded);

  // The session after the edits of S1, with an abandoned try beside them.
  const studio = new Studio();
  studio.tracks = [
    loadedTrack(1, "Backing", backing, LENGTH_SEC, {
      audio_path: fadedPath,
      clips: [{ start_sec: 0, length_sec: LENGTH_SEC, source_path: fadedPath, volume_envelope: [] }],
    }),
    loadedTrack(2, "Vocal", vocal, LENGTH_SEC, { gain_db: -3 }),
  ];
  studio.commit("load", { agoSec: 460 });
  studio.commit("load", { agoSec: 440 });
  const fade = studio.commit("fade", { agoSec: 300 });
  studio.commit("reverb", { agoSec: 240 });
  studio.commit("set_track_gain", { parent: fade, agoSec: 120 });
  const head = studio.head;
  const mixFile = () => writeFixture(`Music/demo-song/.audiograph/previews/${head}.wav`, mixAfter);
  studio.mixes.set(head, mixFile());

  await app.boot({ ...projectWith(studio.tracks, head), ...studio.answers() });
  await expect(page.getByTestId("timeline-lane")).toHaveCount(2);
  await app.settle();
  await page.evaluate(() => document.fonts.ready);

  const play = page.getByTestId("play-pause-button");
  const label = () => play.getAttribute("aria-label");
  const mixLoaded = () =>
    page.locator("[data-testid=timeline-mix-player] audio").evaluateAll((els) => (els as HTMLAudioElement[]).filter((a) => a.src).length);

  await stage.start(NAME);
  await stage.wait(800);

  // Play.
  await stage.click(play);
  await stage.until(async () => (await label()) === "Pause");
  stage.mark("playing");
  await stage.wait(3300);

  // To the Graph tab. The timeline, and its player, go.
  await stage.click(page.getByTestId("tab-graph"));
  await expect(page.getByTestId("graph-bubble")).toHaveCount(studio.nodes.length);
  stage.mark("graph tab");
  // The cache evicts the mix while the user is here.
  rmSync(studio.mixes.get(head)!);
  await app.become({ render_preview: deferred("re-render") });
  await stage.wait(1800);

  // And back.
  await stage.click(page.getByTestId("tab-timeline"));
  stage.mark("timeline tab");
  // The load found no file, so the app asks for the mix again, once.
  await stage.until(async () => (await app.requestsFor("render_preview")).length === 2);
  await stage.wait(450);
  mixFile();
  await app.release("re-render", studio.mixes.get(head)!);
  await stage.until(async () => (await mixLoaded()) === 1);
  await expect(page.getByTestId("timeline-mix-error")).toHaveCount(0);
  await stage.wait(700);

  // Play again.
  await stage.click(play);
  await stage.until(async () => (await label()) === "Pause");
  stage.mark("playing again");
  await stage.wait(4000);
  await stage.click(play, { moveMs: 300 });
  stage.mark("paused");
  await stage.wait(500);

  await expect(page.getByTestId("timeline-mix-error")).toHaveCount(0);
  const done = await stage.finish();
  const wav = writeShotWav(NAME, shotAudio(done.spans, () => mixAfter, done.seconds));
  writeNotes(NAME, { ...done, wav, renderPreviewCalls: await app.requestsFor("render_preview") });
  expect(done.spans.length).toBe(2);
  // The first sound stops when the timeline, and its player, leave with the tab.
  const graphTab = done.marks.find((m) => m.label === "graph tab")!.t;
  expect(done.spans[0].footageEnd).toBeGreaterThan(graphTab - 0.5);
  expect(done.spans[0].footageEnd).toBeLessThan(graphTab + 0.3);
  // One render for the first Play, and one for the mix that was gone: no other.
  expect(await app.requestsFor("render_preview")).toEqual([{ node: head }, { node: head }]);
});
