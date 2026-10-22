/**
 * S2. Faders that stay put (#596).
 *
 * Four ArrowRight taps on a lane's gain slider. A tap moves the slider on
 * key-down and commits it on key-up (`onKeyUp` -> `set_track_gain`), and the
 * session is then read back (`list_tracks`). Before #596 a read-back that
 * landed between the next tap's key-down and key-up put the slider back one
 * step and the tap was lost.
 *
 * This shot sets up that order on purpose, as `e2e/lane-readback.spec.ts`
 * does: each read-back is held, and let go 100 ms into the next tap, after its
 * key-down and before its key-up, with the session as it was after the tap
 * before. The keys are real key events on the real slider; only the backend's
 * timing is scripted. The readout steps +0.5, +1.0, +1.5, +2.0 dB and never
 * goes back.
 */

import { expect, test } from "../../../apps/desktop/e2e/fixtures";
import { deferred, ok, projectWith } from "../../../apps/desktop/e2e/backend";

import { LENGTH_SEC, stems, writeFixture } from "./lib/music";
import { writeNotes } from "./lib/notes";
import { SCALE, VIEWPORT, Stage } from "./lib/stage";
import { Studio, hid, loadedTrack } from "./lib/studio";

test.use({ viewport: VIEWPORT, deviceScaleFactor: SCALE });

const NAME = "s2-faders-stay-put";

test(NAME, async ({ app }) => {
  const page = app.page;
  const stage = await Stage.prepare(app);

  const s = stems();
  const studio = new Studio();
  studio.tracks = [
    loadedTrack(1, "Keys", writeFixture("Music/demo-song/keys.wav", s.keys), LENGTH_SEC),
    loadedTrack(2, "Bass", writeFixture("Music/demo-song/bass.wav", s.bass), LENGTH_SEC),
    loadedTrack(3, "Drums", writeFixture("Music/demo-song/drums.wav", s.drums), LENGTH_SEC),
  ];
  studio.commit("load", { agoSec: 300 });
  studio.commit("load", { agoSec: 280 });
  studio.commit("load", { agoSec: 260 });
  const withGain = (db: number) => studio.tracks.map((t, i) => (i === 0 ? { ...t, gain_db: db } : t));

  await app.boot({ ...projectWith(studio.tracks, studio.head), ...studio.answers() });
  await expect(page.getByTestId("timeline-lane")).toHaveCount(3);
  await expect(page.getByTestId("ruler")).toContainText("0:15");
  await app.settle();
  await page.evaluate(() => document.fonts.ready);

  const gain = page.getByTestId("timeline-lane-gain").first();
  const readout = page.getByTestId("timeline-lane-gain-readout").first();

  await stage.start(NAME);
  await stage.wait(900);

  // Take hold of the slider: a click on its thumb, which leaves the value alone.
  const box = (await gain.boundingBox())!;
  const thumbX = box.x + 8 + (box.width - 16) * ((0 + 60) / 84);
  await stage.moveTo(thumbX, box.y + box.height / 2);
  await stage.wait(120);
  await page.mouse.down();
  await stage.wait(80);
  await page.mouse.up();
  await expect(gain).toBeFocused();
  stage.mark("slider focused");
  await stage.wait(700);

  const commits: string[] = [];
  const HOLD = 160;
  const GAP = 380;
  const SKEW = 100; // how far into the next tap the held read-back is let go
  for (let n = 1; n <= 4; n++) {
    await page.keyboard.down("ArrowRight");
    if (n === 1) {
      await stage.wait(HOLD);
    } else {
      // The read-back of the tap before lands now: key-down has moved the
      // slider, key-up has not committed it.
      await stage.wait(SKEW);
      await app.release(commits[n - 2], withGain(0.5 * (n - 1)));
      await stage.wait(HOLD - SKEW);
    }
    // The commit on key-up asks the backend to set the gain, then reads the
    // session back; that read is held.
    const held = `read-back of tap ${n}`;
    commits.push(held);
    await app.become({
      set_track_gain: ok(hid(`gain-${n}`)),
      list_tracks: deferred(held),
    });
    await page.keyboard.up("ArrowRight");
    stage.mark(`tap ${n}: ${(0.5 * n).toFixed(1)} dB`);
    await stage.wait(GAP);
  }
  // The last read-back, and the session as it now is.
  await app.become({ list_tracks: ok(withGain(2)) });
  await app.release(commits[3], withGain(2));
  await stage.wait(1800);

  await expect(readout).toHaveText("+2.0 dB");
  const done = await stage.finish();
  writeNotes(NAME, { ...done, sets: await app.requestsFor("set_track_gain") });
  expect(await app.requestsFor("set_track_gain")).toEqual([
    { track: 0, gainDb: 0.5 },
    { track: 0, gainDb: 1 },
    { track: 0, gainDb: 1.5 },
    { track: 0, gainDb: 2 },
  ]);
});
