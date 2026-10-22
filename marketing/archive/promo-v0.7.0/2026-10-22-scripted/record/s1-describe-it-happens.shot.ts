/**
 * S1. Describe it, it happens.
 *
 * Two tracks. The user types a plain-English request into the real composer;
 * the scripted assistant answers with the two tool calls the real agent would
 * make for it, `fade` and `set_track_gain`; the timeline updates; the user
 * presses Play and the playhead runs while the mix plays.
 *
 * `fade` is destructive (`crates/tools/src/tool/fade.rs`): it writes the
 * faded audio to `.audiograph/derived/<hash>.wav` and points the clip at it,
 * so the backing lane redraws from the new file, the linear ramp the tool
 * applies. `set_track_gain` is absolute and changes the track's gain, which the
 * lane's slider shows. The mix the app plays after is the sum of those two
 * results, computed here from the same samples.
 */

import { createHash } from "node:crypto";

import { expect, test } from "../../../apps/desktop/e2e/fixtures";
import { projectWith } from "../../../apps/desktop/e2e/backend";

import { LENGTH_SEC, fadeIn, mix, stems, writeFixture } from "./lib/music";
import { shotAudio, writeShotWav } from "./lib/media";
import { SCALE, VIEWPORT, Stage } from "./lib/stage";
import { Studio, agentTurn, holdNextSend, loadedTrack, sent } from "./lib/studio";
import { writeNotes } from "./lib/notes";

test.use({ viewport: VIEWPORT, deviceScaleFactor: SCALE });

const NAME = "s1-describe-it-happens";
const REQUEST = "Fade the intro in over 4 seconds and bring the vocal down 3 dB";
const REPLY = "Done. The backing now fades in over the first 4 seconds, and the vocal is 3 dB quieter.";

test(NAME, async ({ app }) => {
  const page = app.page;
  const stage = await Stage.prepare(app);

  // The audio: a backing bed and a vocal line, synthesised in code.
  const s = stems();
  const backing = writeFixture("Music/demo-song/backing.wav", s.backing);
  const vocal = writeFixture("Music/demo-song/vocal.wav", s.vocal);
  const faded = fadeIn(s.backing, 0, 4);
  const hashOf = (a: Float32Array) => createHash("sha256").update(Buffer.from(a.buffer)).digest("hex");
  const fadedPath = writeFixture(`Music/demo-song/.audiograph/derived/${hashOf(faded)}.wav`, faded);

  const mixBefore = mix([{ samples: s.backing }, { samples: s.vocal }]);
  const mixAfter = mix([{ samples: faded }, { samples: s.vocal, gainDb: -3 }]);

  // The session: two files loaded, as the store holds it.
  const studio = new Studio();
  studio.tracks = [loadedTrack(1, "Backing", backing, LENGTH_SEC), loadedTrack(2, "Vocal", vocal, LENGTH_SEC)];
  studio.commit("load", { agoSec: 140 });
  studio.commit("load", { agoSec: 125 });
  const loaded = studio.head;
  studio.mixes.set(loaded, writeFixture(`Music/demo-song/.audiograph/previews/${loaded}.wav`, mixBefore));

  await app.boot({ ...projectWith(studio.tracks, studio.head), ...studio.answers() });
  await expect(page.getByTestId("ruler")).toContainText("0:15");
  await expect(page.getByTestId("timeline-lane")).toHaveCount(2);
  await app.settle();
  await page.evaluate(() => document.fonts.ready);

  await stage.start(NAME);
  await stage.wait(800);

  // The user describes the edit.
  const box = page.getByTestId("chat-form").locator("textarea");
  await stage.click(box);
  // Out of the way of the words, as a hand leaves the mouse.
  await stage.moveTo(1470, 560, 500);
  await stage.wait(250);
  await stage.type(REQUEST, 0.7);
  stage.mark("request typed");
  await stage.wait(450);
  await holdNextSend(app, "send-1");
  await stage.press("Enter", 60);
  stage.mark("request sent");
  await sent(app, 1);

  // The assistant's turn: the two calls the real agent makes, then a short reply.
  await agentTurn(stage, studio, "send-1", [
    {
      tool: "fade", // { track: 0, kind: "in", range: { start_sec: 0, end_sec: 4 } }
      effect: () => {
        studio.tracks[0] = {
          ...studio.tracks[0],
          audio_path: fadedPath,
          clips: [{ start_sec: 0, length_sec: LENGTH_SEC, source_path: fadedPath, volume_envelope: [] }],
        };
        const id = studio.commit("fade");
        studio.mixes.set(id, writeFixture(`Music/demo-song/.audiograph/previews/${id}.wav`, mix([{ samples: faded }, { samples: s.vocal }])));
      },
    },
    {
      tool: "set_track_gain", // { track: 1, db: -3 }
      effect: () => {
        studio.tracks[1] = { ...studio.tracks[1], gain_db: -3 };
        const id = studio.commit("set_track_gain");
        studio.mixes.set(id, writeFixture(`Music/demo-song/.audiograph/previews/${id}.wav`, mixAfter));
      },
    },
    { pause: 250 },
    { say: REPLY },
  ]);
  stage.mark("reply done");
  await expect(page.getByTestId("tool-badge")).toHaveCount(2);
  await expect(page.getByTestId("timeline-lane-gain-readout").nth(1)).toContainText("-3.0 dB");
  await stage.wait(1200);

  // Press Play.
  await stage.click(page.getByTestId("play-pause-button"));
  stage.mark("play pressed");
  await stage.until(async () => (await page.getByTestId("play-pause-button").getAttribute("aria-label")) === "Pause");
  stage.mark("playing");
  await stage.wait(5500);
  await stage.click(page.getByTestId("play-pause-button"), { moveMs: 250 });
  stage.mark("paused");
  await stage.wait(500);

  const done = await stage.finish();
  const wav = writeShotWav(NAME, shotAudio(done.spans, () => mixAfter, done.seconds));
  writeNotes(NAME, { ...done, wav, request: REQUEST, reply: REPLY });
  expect(done.spans.length).toBeGreaterThan(0);
  // The mix the app played is the one rendered after both edits.
  expect(await app.requestsFor("render_preview")).toEqual([{ node: studio.head }]);
});
