/**
 * Undo follows the path the user took, not the node's first parent (#398).
 *
 * A node's id is a hash of its state, so an edit that returns to an
 * earlier state (mute, then unmute) lands on a node that already exists,
 * and that node keeps the parent it had the first time. Undo asked for
 * that parent, so from the unmuted state it skipped the mute and went on
 * to take back the load before it.
 *
 * The fake backend reproduces what the real store does: the unmute
 * answers with the head the project started on, and `get_node` reports
 * that node's *original* parent.
 */

import { fixturePath } from "./audio-fixtures";
import { nodeId, ok, projectWith, sessionNode, toneTrack } from "./backend";
import { expect, test } from "./fixtures";

const BEFORE_LOAD = nodeId(1); // the parent the store recorded for START
const START = nodeId(2); // two tracks loaded
const MUTED = nodeId(3);

test("mute, unmute, then undo brings the mute back before anything older", async ({ app }) => {
  await app.boot({
    ...projectWith([toneTrack()], START),
    render_preview: ok(fixturePath("tone3s")),
    get_node: ok(sessionNode(START, BEFORE_LOAD)),
    set_track_muted: ok(MUTED),
    set_head_to: ok(MUTED),
  });
  const page = app.page;
  await expect(page.getByTestId("ruler")).toContainText("0:03");

  await page.getByTestId("timeline-lane-mute").click();
  await expect.poll(() => app.requestsFor("set_track_muted")).toHaveLength(1);

  // Unmuting reaches the state the project started in: the same node.
  await app.become({ set_track_muted: ok(START) });
  await page.getByTestId("timeline-lane-mute").click();
  await expect.poll(() => app.requestsFor("set_track_muted")).toHaveLength(2);
  await app.settle();

  // First undo: the muted state, from the trail, with no need to ask the
  // graph for a parent.
  await app.become({ set_head_to: ok(MUTED) });
  await page.keyboard.press("Control+z");
  await expect.poll(() => app.requestsFor("set_head_to")).toEqual([{ nodeId: MUTED }]);
  await app.settle();
  expect(await app.requestsFor("get_node")).toEqual([]);

  // Second undo: the state before the mute, which is START again.
  await app.become({ set_head_to: ok(START) });
  await page.keyboard.press("Control+z");
  await expect.poll(() => app.requestsFor("set_head_to")).toEqual([{ nodeId: MUTED }, { nodeId: START }]);
  await app.settle();
  expect(await app.requestsFor("get_node")).toEqual([]);

  // The trail is spent: now the stored parent is the right answer.
  await app.become({ set_head_to: ok(BEFORE_LOAD) });
  await page.keyboard.press("Control+z");
  await expect
    .poll(() => app.requestsFor("set_head_to"))
    .toEqual([{ nodeId: MUTED }, { nodeId: START }, { nodeId: BEFORE_LOAD }]);
  expect(await app.requestsFor("get_node")).toEqual([{ id: START }]);
});

test("redo goes back along the same path", async ({ app }) => {
  await app.boot({
    ...projectWith([toneTrack()], START),
    render_preview: ok(fixturePath("tone3s")),
    get_node: ok(sessionNode(START, BEFORE_LOAD)),
    set_track_muted: ok(MUTED),
    set_head_to: ok(START),
  });
  const page = app.page;
  await expect(page.getByTestId("ruler")).toContainText("0:03");

  await page.getByTestId("timeline-lane-mute").click();
  await expect.poll(() => app.requestsFor("set_track_muted")).toHaveLength(1);
  await app.settle();

  await page.keyboard.press("Control+z"); // back to START
  await expect.poll(() => app.requestsFor("set_head_to")).toEqual([{ nodeId: START }]);
  await app.settle();

  await app.become({ set_head_to: ok(MUTED) });
  await page.keyboard.press("Control+y"); // forward to MUTED
  await expect.poll(() => app.requestsFor("set_head_to")).toEqual([{ nodeId: START }, { nodeId: MUTED }]);
  await app.settle();

  // And undo again returns to START: redo put the head it left on the trail.
  await app.become({ set_head_to: ok(START) });
  await page.keyboard.press("Control+z");
  await expect
    .poll(() => app.requestsFor("set_head_to"))
    .toEqual([{ nodeId: START }, { nodeId: MUTED }, { nodeId: START }]);
});
