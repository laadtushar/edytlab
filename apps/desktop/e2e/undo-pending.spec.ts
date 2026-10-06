/**
 * A slow undo says so, and a second press does not stack another (#373).
 *
 * Undo onto history whose audio was swept to keep the project under its
 * cap (#98) rebuilds that audio before `set_head_to` answers — seconds,
 * on long history. The test holds `set_head_to` to stand in for that.
 */

import { fixturePath } from "./audio-fixtures";
import { deferred, nodeId, ok, projectWith, sessionNode, toneTrack } from "./backend";
import { expect, test } from "./fixtures";

const PARENT = nodeId(1);
const HEAD = nodeId(2);

test("a slow undo shows it is restoring history, once", async ({ app }) => {
  await app.boot({
    ...projectWith([toneTrack()], HEAD),
    render_preview: ok(fixturePath("tone3s")),
    // `get_node` answers the head, whose parent is where undo goes.
    get_node: ok(sessionNode(HEAD, PARENT)),
    set_head_to: deferred("rebuild"),
  });
  const page = app.page;
  await expect(page.getByTestId("ruler")).toContainText("0:03");
  const state = page.getByTestId("status-bar-state");
  await expect(state).toContainText("ready");

  await page.keyboard.press("Control+z");
  await expect.poll(() => app.requestsFor("set_head_to")).toEqual([{ nodeId: PARENT }]);
  await expect(state).toContainText("restoring history");

  // Pressed again while the first is still rebuilding: nothing new.
  await page.keyboard.press("Control+z");
  await app.settle();
  expect(await app.requestsFor("set_head_to")).toHaveLength(1);
  expect(await app.requestsFor("get_node")).toHaveLength(1);

  await app.release("rebuild", PARENT);
  await expect(state).toContainText("ready");
  await expect(state).not.toContainText("restoring history");
});

test("an ordinary undo never says it is restoring history", async ({ app }) => {
  await app.boot({
    ...projectWith([toneTrack()], HEAD),
    render_preview: ok(fixturePath("tone3s")),
    get_node: ok(sessionNode(HEAD, PARENT)),
    set_head_to: ok(PARENT),
  });
  const page = app.page;
  await expect(page.getByTestId("ruler")).toContainText("0:03");

  // Record every state word the bar shows, from before the press on.
  await page.evaluate(() => {
    const seen: string[] = [];
    (window as unknown as { seenStates: string[] }).seenStates = seen;
    const el = document.querySelector('[data-testid="status-bar-state"]')!;
    new MutationObserver(() => seen.push(el.textContent ?? "")).observe(el, {
      subtree: true,
      childList: true,
      characterData: true,
    });
  });
  await page.keyboard.press("Control+z");
  await expect.poll(() => app.requestsFor("set_head_to")).toEqual([{ nodeId: PARENT }]);
  await app.settle();
  await page.waitForTimeout(400);

  const seen = await page.evaluate(
    () => (window as unknown as { seenStates: string[] }).seenStates,
  );
  expect(seen.filter((s) => s.includes("restoring history"))).toEqual([]);
});
