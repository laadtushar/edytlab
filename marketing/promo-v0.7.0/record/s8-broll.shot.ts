/**
 * S8. B-roll, for backgrounds. No chat is used and none is shown in use: the
 * assistant pane is the empty first-run one, and the shots are meant to be
 * cropped to the left pane (x 0 to 1240 of 1600 CSS pixels).
 *
 *  - S8a: a six-track session, zoomed in, panned slowly from the start of the
 *    piece to its end, then down the lanes, with labels in the label lane.
 *  - S8b: the History graph with branches (the abandoned tries and the line
 *    of edits), drawn by the app's own graph view, zoomed in a little and
 *    panned by dragging, as a user would.
 */

import { expect, test } from "../../../apps/desktop/e2e/fixtures";
import { ok, projectWith } from "../../../apps/desktop/e2e/backend";

import { buildHistory } from "./lib/history";
import { LENGTH_SEC, stems, writeFixture } from "./lib/music";
import { writeNotes } from "./lib/notes";
import { SCALE, VIEWPORT, Stage } from "./lib/stage";
import { Studio, loadedTrack, uuid } from "./lib/studio";

test.use({ viewport: VIEWPORT, deviceScaleFactor: SCALE });

const easeInOut = (t: number) => (t < 0.5 ? 2 * t * t : 1 - (-2 * t + 2) ** 2 / 2);

test("s8a-broll-timeline", async ({ app }) => {
  const NAME = "s8a-broll-timeline";
  const page = app.page;
  const stage = await Stage.prepare(app);

  const s = stems();
  const studio = new Studio();
  const lanes: [string, keyof typeof s][] = [
    ["Drums", "drums"],
    ["Bass", "bass"],
    ["Keys", "keys"],
    ["Pad", "pad"],
    ["Lead", "lead"],
    ["Vocal", "vocal"],
  ];
  studio.tracks = lanes.map(([name, stem], i) =>
    loadedTrack(i + 1, name, writeFixture(`Music/demo-song/${name.toLowerCase()}.wav`, s[stem]), LENGTH_SEC),
  );
  for (let i = 0; i < lanes.length; i++) studio.commit("load", { agoSec: 900 - i * 20 });
  const marker = (i: number, name: string, time_sec: number) => ({ id: uuid(100 + i), name, kind: "marker" as const, time_sec });
  const markers = [marker(0, "intro", 0.4), marker(1, "verse", 4), marker(2, "chorus", 8), marker(3, "bridge", 12), marker(4, "outro", 15)];

  await app.boot({ ...projectWith(studio.tracks, studio.head), ...studio.answers(), list_markers: ok(markers) });
  await expect(page.getByTestId("timeline-lane")).toHaveCount(lanes.length);
  await app.settle();
  await page.evaluate(() => document.fonts.ready);

  await stage.start(NAME);
  await stage.wait(900);

  // Zoom in twice: the piece is now wider than the pane.
  await stage.click(page.getByTestId("zoom-in-btn"), { moveMs: 600 });
  await stage.wait(300);
  await stage.click(page.getByTestId("zoom-in-btn"), { moveMs: 150 });
  await stage.wait(700);
  const hscroll = page.getByTestId("timeline-hscroll");
  const maxX = await hscroll.evaluate((el) => el.scrollWidth - el.clientWidth);
  expect(maxX).toBeGreaterThan(300);

  // Along the piece, slowly: scrollLeft set by the clock, as a steady hand on
  // a trackpad would, so the lanes, the ruler and the labels move together.
  const along = async (ms: number, from: number, to: number) => {
    const steps = Math.round(ms / 33.333);
    for (let i = 1; i <= steps; i++) {
      await hscroll.evaluate((el, x) => (el.scrollLeft = x), from + (to - from) * easeInOut(i / steps));
      await stage.tick();
    }
  };
  stage.mark("pan starts");
  await along(7200, 0, maxX);
  stage.mark("pan ends");
  await stage.wait(500);

  // Then down the lanes.
  const body = page.getByTestId("timeline-body");
  const maxY = await body.evaluate((el) => el.scrollHeight - el.clientHeight);
  if (maxY > 40) {
    const steps = Math.round(3600 / 33.333);
    for (let i = 1; i <= steps; i++) {
      await body.evaluate((el, y) => (el.scrollTop = y), maxY * easeInOut(i / steps));
      await stage.tick();
    }
  }
  await stage.wait(700);

  const done = await stage.finish();
  writeNotes(NAME, { ...done, lanes: lanes.map((l) => l[0]), scrolledXPx: maxX, scrolledYPx: maxY, crop: "left pane: x 0..1240, y 52..972 (CSS px)" });
});

test("s8b-broll-graph", async ({ app }) => {
  const NAME = "s8b-broll-graph";
  const page = app.page;
  const stage = await Stage.prepare(app);

  const s = stems();
  const studio = new Studio();
  studio.tracks = [
    loadedTrack(1, "Drums", writeFixture("Music/demo-song/drums.wav", s.drums), LENGTH_SEC),
    loadedTrack(2, "Vocal", writeFixture("Music/demo-song/vocal.wav", s.vocal), LENGTH_SEC),
  ];
  const history = buildHistory(studio);
  studio.name(history.head, "chorus");

  await app.boot({ ...projectWith(studio.tracks, studio.head), ...studio.answers() });
  await expect(page.getByTestId("timeline-lane")).toHaveCount(2);
  await app.settle();
  await page.getByTestId("tab-graph").click();
  await expect(page.getByTestId("graph-bubble")).toHaveCount(studio.nodes.length);
  await app.settle();
  await page.evaluate(() => document.fonts.ready);

  await stage.start(NAME);
  await stage.wait(1700);

  // Zoom in a little with the wheel over the graph, then drag it along.
  const root = page.getByTestId("graph-view-root");
  const box = (await root.boundingBox())!;
  const centre = { x: box.x + box.width / 2, y: box.y + box.height / 2 };
  await stage.moveTo(centre.x, centre.y, 700);
  await stage.wait(200);
  for (let i = 0; i < 6; i++) {
    await page.mouse.wheel(0, -60);
    await stage.wait(110);
  }
  await stage.wait(900);
  stage.mark("drag starts");
  await page.mouse.down();
  await stage.wait(80);
  const from = { ...centre };
  const steps = Math.round(6800 / 33.333);
  for (let i = 1; i <= steps; i++) {
    const e = easeInOut(i / steps);
    await page.mouse.move(from.x - 150 * e, from.y - 230 * e);
    await stage.tick();
  }
  await page.mouse.up();
  stage.mark("drag ends");
  await stage.wait(2000);

  const done = await stage.finish();
  writeNotes(NAME, { ...done, nodes: studio.nodes.length, crop: "left pane: x 0..1240, y 52..972 (CSS px)" });
});
