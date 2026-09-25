/**
 * A label typed into the lane is the head the app works from, and is
 * still there when the project is opened again (#232, #273).
 *
 * `add_marker` appends a node and returns its id. That id used to be
 * dropped: the timeline went on working from the head before the label,
 * `view.json` saved that older head, and reopening the project moved the
 * store back to it — the label survived as a node in the graph and was
 * gone from the lane, and Ctrl+Z undid the edit before it.
 *
 * These were source greps (`headIsApplied.test.ts`), because `App.tsx`
 * could not be rendered in a test. It can here: the real app, in a real
 * browser, with only the IPC boundary replaced.
 */

import { fixturePath } from "./audio-fixtures";
import { nodeId, ok, projectWith, selecting, sessionWith, toneTrack } from "./backend";
import { expect, test, type App } from "./fixtures";

const BEFORE = nodeId(1);
const LABELLED = nodeId(7);

/**
 * The label `add_marker` records, as `list_markers` then serialises it:
 * an `Annotation` with its kind flattened in.
 */
const verse = (timeSec: number) => ({
  id: "1a2b3c4d-5e6f-4a7b-8c9d-0e1f2a3b4c5d",
  name: "verse",
  kind: "marker",
  time_sec: timeSec,
});

/** A returning user with one track and no labels yet. */
async function openProject(app: App): Promise<void> {
  await app.boot({
    ...projectWith([toneTrack()], BEFORE),
    ...selecting(),
    render_preview: ok(fixturePath("tone3s")),
  });
  await expect(app.page.getByTestId("ruler")).toContainText("0:03");
}

/**
 * Double-click the label lane and name the label, as a user does. The
 * backend answers as `add_marker` does: the new head, then the
 * `marker-changed` event, after which `list_markers` holds the label.
 * Returns the time the app asked for.
 */
async function addLabel(app: App, name: string): Promise<number> {
  const page = app.page;
  await app.become({ add_marker: ok(LABELLED) });
  page.once("dialog", (d) => void d.accept(name));
  const lane = page.getByTestId("label-lane-track");
  const box = (await lane.boundingBox())!;
  await lane.dblclick({ position: { x: box.width / 2, y: box.height / 2 } });

  await expect.poll(() => app.requestsFor("add_marker")).toHaveLength(1);
  const [{ time }] = (await app.requestsFor("add_marker")) as { time: number; name: string }[];
  await app.become({ ...sessionWith([toneTrack()]), list_markers: ok([verse(time)]) });
  await app.emit("marker-changed");
  return time;
}

test.describe("a label added in the lane", () => {
  test("is the head everything after it works from", async ({ app }) => {
    await openProject(app);
    await addLabel(app, "verse");
    expect(await app.requestsFor("add_marker")).toEqual([
      { time: expect.any(Number), name: "verse" },
    ]);
    await expect(app.page.getByTestId("label-chip")).toHaveAttribute("data-label-name", "verse");

    // The next edit — here, a render — starts from the label's node.
    await app.page.getByTestId("render-preview-button").click();
    await expect
      .poll(() => app.requestsFor("render_preview"), { message: "a render of the labelled head" })
      .toEqual([{ node: LABELLED }]);

    // And the view saved for next time names it, not the head before.
    await expect
      .poll(async () => {
        const saves = (await app.requestsFor("save_view_state")) as { view: { head: string } }[];
        return saves.at(-1)?.view.head;
      }, { message: "the head view.json keeps" })
      .toBe(LABELLED);
  });

  test("is still in the lane when the project is opened again", async ({ app }) => {
    await openProject(app);
    const time = await addLabel(app, "verse");
    await expect
      .poll(async () => {
        const saves = (await app.requestsFor("save_view_state")) as { view: { head: string } }[];
        return saves.at(-1)?.view.head;
      })
      .toBe(LABELLED);
    const saves = (await app.requestsFor("save_view_state")) as { view: unknown }[];
    const saved = saves.at(-1)!.view;

    // Quit, and open it again: the backend restores `HEAD` from disk —
    // the labelled node — and reads back exactly the view this session
    // wrote.
    await app.boot({
      ...projectWith([toneTrack()], LABELLED),
      list_markers: ok([verse(time)]),
      get_view_state: ok(saved),
      render_preview: ok(fixturePath("tone3s")),
    });
    await expect(app.page.getByTestId("ruler")).toContainText("0:03");
    await expect(app.page.getByTestId("label-chip")).toHaveAttribute("data-label-name", "verse");
    await app.settle();

    // Nothing moved the store off the label's node: that move, to the
    // head before it, is what lost labels on reopen.
    expect(await app.requestsFor("set_head_to")).toEqual([]);
    await app.page.getByTestId("render-preview-button").click();
    await expect
      .poll(() => app.requestsFor("render_preview"))
      .toEqual([{ node: LABELLED }]);
  });
});
