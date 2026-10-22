/**
 * S3. The History graph keeps up (#602).
 *
 * The Graph tab is open. Two requests to the scripted assistant:
 *
 *  1. "Name this version chorus." The agent's `name_node` sets a label and moves
 *     no head and makes no node, so no `node-created` follows; the window hears
 *     of it when the turn ends (`agent://done`), and the label is on the head's
 *     bubble then, with nothing else done (before #602 it waited for the next
 *     edit).
 *  2. "Compact the history, keep the last 8." The agent's `compact_session`
 *     first reports (a dry run), then applies. A compaction makes no node
 *     either; the window re-reads the graph when the call ends (`useSession`
 *     `forgetRemoved`), and the removed nodes leave the graph. What goes is
 *     what `plan_compaction` removes: the abandoned tries and the tail beyond
 *     the last 8 nodes of the line.
 */

import { expect, test } from "../../../apps/desktop/e2e/fixtures";
import { projectWith } from "../../../apps/desktop/e2e/backend";

import { buildHistory, compact } from "./lib/history";
import { LENGTH_SEC, stems, writeFixture } from "./lib/music";
import { writeNotes } from "./lib/notes";
import { SCALE, VIEWPORT, Stage } from "./lib/stage";
import { Studio, agentTurn, holdNextSend, loadedTrack, sent } from "./lib/studio";

test.use({ viewport: VIEWPORT, deviceScaleFactor: SCALE });

const NAME = "s3-history-graph";
const KEEP = 8;

test(NAME, async ({ app }) => {
  const page = app.page;
  const stage = await Stage.prepare(app);

  const s = stems();
  const studio = new Studio();
  studio.tracks = [
    loadedTrack(1, "Drums", writeFixture("Music/demo-song/drums.wav", s.drums), LENGTH_SEC),
    loadedTrack(2, "Vocal", writeFixture("Music/demo-song/vocal.wav", s.vocal), LENGTH_SEC),
  ];
  const history = buildHistory(studio);
  const total = studio.nodes.length;

  await app.boot({ ...projectWith(studio.tracks, studio.head), ...studio.answers() });
  await expect(page.getByTestId("timeline-lane")).toHaveCount(2);
  await app.settle();
  await page.getByTestId("tab-graph").click();
  await expect(page.getByTestId("graph-bubble")).toHaveCount(total);
  await app.settle();
  await page.evaluate(() => document.fonts.ready);
  await expect(page.getByTestId("graph-bubble").filter({ hasText: "chorus" })).toHaveCount(0);

  await stage.start(NAME);
  await stage.wait(1000);

  const box = page.getByTestId("chat-form").locator("textarea");
  const ask = async (text: string, n: number) => {
    await stage.click(box);
    await stage.moveTo(1470, 560, 450);
    await stage.wait(200);
    await stage.type(text, 0.7);
    await stage.wait(400);
    await holdNextSend(app, `send-${n}`);
    await stage.press("Enter", 60);
    await sent(app, n);
  };

  // 1. Name the version.
  await ask("Name this version chorus", 1);
  stage.mark("request 1 sent");
  await agentTurn(stage, studio, "send-1", [
    {
      tool: "name_node", // { node_id: <head>, label: "chorus" }
      runMs: 450,
      effect: () => studio.name(studio.head, "chorus"),
    },
    { say: "Named it chorus." },
  ]);
  stage.mark("label set");
  await expect(page.getByTestId("graph-bubble").filter({ hasText: "chorus" })).toHaveCount(1);
  await stage.wait(2000);

  // 2. Compact the history.
  await ask(`Compact the history, keep the last ${KEEP} edits`, 2);
  stage.mark("request 2 sent");
  let removed: string[] = [];
  await agentTurn(stage, studio, "send-2", [
    { tool: "compact_session", runMs: 650 }, // { keep_last: 8 }: a dry run, changes nothing
    { pause: 500 },
    {
      tool: "compact_session", // { keep_last: 8, apply: true }
      runMs: 800,
      effect: () => {
        removed = compact(studio, history.chain, KEEP);
      },
    },
    { say: `Removed ${total - KEEP} nodes of history. The last ${KEEP} on the current chain are untouched.` },
  ]);
  stage.mark("compaction done");
  await expect(page.getByTestId("graph-bubble")).toHaveCount(KEEP);
  await stage.wait(2400);

  const done = await stage.finish();
  writeNotes(NAME, { ...done, nodesBefore: total, nodesAfter: KEEP, removed: total - KEEP, requests: await app.requestsFor("send_message") });
  expect(removed.length).toBe(total - KEEP);
});
