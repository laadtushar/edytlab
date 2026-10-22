/**
 * S4. Big sessions (#618): about 200 tracks.
 *
 * "make all the strings quieter". The session block in the agent's system
 * prompt stops listing tracks at its byte budget, and says how many it left
 * out and that `list_session` pages through them (#573, #618). The strings
 * are twelve tracks (148 to 159) past that cut, so the scripted assistant pages for them
 * (two `list_session` calls, `what: "tracks"`), then lowers each with
 * `set_track_gain`. That is 14 calls, within the 20 a turn allows
 * (`MAX_TOOL_CALLS_PER_TURN`).
 *
 * Afterwards the timeline is scrolled to the strings, where each lane's gain
 * reads -4.0 dB.
 */

import { expect, test } from "../../../apps/desktop/e2e/fixtures";
import { projectWith } from "../../../apps/desktop/e2e/backend";

import { orchestra } from "./lib/orchestra";
import { writeNotes } from "./lib/notes";
import { SCALE, VIEWPORT, Stage } from "./lib/stage";
import { Studio, agentTurn, holdNextSend, sent, type Beat } from "./lib/studio";

test.use({ viewport: VIEWPORT, deviceScaleFactor: SCALE });

const NAME = "s4-200-tracks";
const REQUEST = "make all the strings quieter";
const REPLY = "Done. All 12 string tracks (Violin I and II, Viola, Cello, Contrabass) are now 4 dB quieter.";

test(NAME, async ({ app }) => {
  test.setTimeout(900_000);
  const page = app.page;
  const stage = await Stage.prepare(app);

  const { tracks, strings } = orchestra();
  expect(tracks).toHaveLength(200);
  const studio = new Studio();
  studio.tracks = tracks;
  studio.commit("load", { agoSec: 600 });
  studio.commit("load", { agoSec: 590 });

  await app.boot({ ...projectWith(studio.tracks, studio.head), ...studio.answers() });
  await expect(page.getByTestId("timeline-lane")).toHaveCount(200, { timeout: 60_000 });
  await expect(page.getByTestId("ruler")).toContainText("0:06");
  await app.settle();
  await page.evaluate(() => document.fonts.ready);
  // Let the first lanes' waveforms draw.
  await page.waitForTimeout(1500);

  const body = page.getByTestId("timeline-body");
  const bodyBox = (await body.boundingBox())!;
  const over = { x: bodyBox.x + bodyBox.width * 0.6, y: bodyBox.y + bodyBox.height * 0.55 };

  await stage.start(NAME);
  await stage.wait(800);

  // A look down the list, so the size of it shows, and back.
  await stage.wheel(over, 9000, 1200);
  await stage.wait(100);
  await stage.wheel(over, -9000, 900);
  await stage.wait(400);

  const box = page.getByTestId("chat-form").locator("textarea");
  await stage.click(box);
  await stage.moveTo(1470, 560, 450);
  await stage.wait(200);
  await stage.type(REQUEST, 0.7);
  stage.mark("request typed");
  await stage.wait(400);
  await holdNextSend(app, "send-1");
  await stage.press("Enter", 60);
  stage.mark("request sent");
  await sent(app, 1);

  const beats: Beat[] = [
    { tool: "list_session", runMs: 650 }, // { what: "tracks", offset: <where the block stopped> }
    { tool: "list_session", runMs: 600 }, // the next page of the same list
    { pause: 250 },
  ];
  for (const i of strings) {
    beats.push({
      tool: "set_track_gain", // { track: i, db: -4 }
      runMs: 160,
      effect: () => {
        studio.tracks[i] = { ...studio.tracks[i], gain_db: -4 };
        studio.commit("set_track_gain");
      },
    });
  }
  beats.push({ pause: 250 }, { say: REPLY });
  await agentTurn(stage, studio, "send-1", beats, { thinkMs: 1000 });
  stage.mark("reply done");
  await expect(page.getByTestId("tool-badge")).toHaveCount(2 + strings.length);
  await stage.wait(900);

  // Down to the strings: where the first of them is, from the top of the lanes.
  const first = page.getByTestId("timeline-lane").nth(strings[0]);
  const target = await first.evaluate((el) => {
    const scroller = document.querySelector('[data-testid="timeline-body"]') as HTMLElement;
    return el.getBoundingClientRect().top - scroller.getBoundingClientRect().top + scroller.scrollTop;
  });
  const now = await body.evaluate((el) => el.scrollTop);
  await stage.wheel(over, target - 150 - now, 1900);
  stage.mark("strings in view");
  await stage.wait(2000);

  const lowered = await page.getByTestId("timeline-lane-gain-readout").evaluateAll((els) =>
    els.map((e) => e.textContent).filter((t) => t === "-4.0 dB").length,
  );
  const done = await stage.finish();
  writeNotes(NAME, { ...done, tracks: tracks.length, strings: strings.length, readoutsAtMinus4InDom: lowered });
  expect(await app.requestsFor("send_message")).toHaveLength(1);
});
