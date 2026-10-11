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
import {
  nodeId,
  ok,
  projectWith,
  selecting,
  sessionWith,
  toneTrack,
} from "./backend";
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
 * Double-click the label lane and type the label's name into the box
 * that opens there, as a user does. The backend answers as `add_marker`
 * does: the new head, then the `marker-changed` event, after which
 * `list_markers` holds the label. Returns the time the app asked for.
 */
async function addLabel(app: App, name: string): Promise<number> {
  const page = app.page;
  await app.become({ add_marker: ok(LABELLED) });
  const lane = page.getByTestId("label-lane-track");
  const box = (await lane.boundingBox())!;
  await lane.dblclick({ position: { x: box.width / 2, y: box.height / 2 } });
  const input = page.getByTestId("label-draft-input");
  await expect(input).toBeFocused();
  await input.fill(name);
  await input.press("Enter");

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

  /**
   * Ctrl+Z after a label undoes the label, not the edit before it — the
   * other half of #232. With the label's head dropped, undo went to the
   * parent of the head *before* the label and stepped back one edit too
   * far. Redo then comes back to the label.
   *
   * Undo returns along the path taken (#398): the head the project
   * opened on, then the label's node, are the steps the user took, so
   * undo needs nothing from the graph and asks no node for its parent.
   * This test used to assert that `get_node` call. It pinned how undo
   * found its target rather than where it went, and with the path taken
   * the call is no longer made — what the user sees is the `set_head_to`
   * sequence below and the head the next render works from.
   *
   * The lane shows the labels of the head undo and redo land on, although
   * `set_head_to` emits no `marker-changed` (#453): the chip goes with
   * the undo and comes back with the redo.
   */
  test("is what undo takes back, and redo restores", async ({ app }) => {
    await openProject(app);
    const time = await addLabel(app, "verse");
    await expect(app.page.getByTestId("label-chip")).toHaveAttribute("data-label-name", "verse");

    // `set_head_to` answers the id it moved to.
    await app.become({
      set_head_to: ok(BEFORE),
      list_markers: ok([]),
    });
    await app.page.keyboard.press("Control+z");
    await expect.poll(() => app.requestsFor("set_head_to")).toEqual([{ nodeId: BEFORE }]);
    // The label went with the head. No event said so: the lane read again
    // because the head moved.
    await expect(app.page.getByTestId("label-chip")).toHaveCount(0);

    await app.become({ set_head_to: ok(LABELLED), list_markers: ok([verse(time)]) });
    await app.page.keyboard.press("Control+Shift+z");
    await expect
      .poll(() => app.requestsFor("set_head_to"))
      .toEqual([{ nodeId: BEFORE }, { nodeId: LABELLED }]);
    await expect(app.page.getByTestId("label-chip")).toHaveAttribute("data-label-name", "verse");

    // And the head the app works from is the label's again.
    await app.page.getByTestId("render-preview-button").click();
    await expect.poll(() => app.requestsFor("render_preview")).toEqual([{ node: LABELLED }]);
  });
});

/**
 * The name is typed in the lane. It used to be asked for with
 * `window.prompt`, which a macOS webview answers with `null`, so there a
 * double-click on the lane did nothing at all. A dialog here would be the
 * same bug waiting to come back, so the test fails on any.
 */
test.describe("naming a label in the lane", () => {
  test("is done in the lane itself, with no dialog", async ({ app }) => {
    await openProject(app);
    const dialogs: string[] = [];
    app.page.on("dialog", (d) => {
      dialogs.push(d.message());
      void d.dismiss();
    });

    await addLabel(app, "verse");

    expect(dialogs).toEqual([]);
    expect(await app.requestsFor("add_marker")).toEqual([
      { time: expect.any(Number), name: "verse" },
    ]);
  });

  test("is abandoned with Escape, and adds nothing", async ({ app }) => {
    await openProject(app);
    const lane = app.page.getByTestId("label-lane-track");
    const box = (await lane.boundingBox())!;
    await lane.dblclick({ position: { x: box.width / 3, y: box.height / 2 } });
    const input = app.page.getByTestId("label-draft-input");
    await input.fill("nope");
    await input.press("Escape");

    await expect(input).toHaveCount(0);
    await app.settle();
    expect(await app.requestsFor("add_marker")).toEqual([]);
  });
});

/**
 * The ruler adds a marker too, and used to ask for its name with
 * `window.prompt` as well (#546): in a macOS webview, which answers a
 * prompt with `null`, a click on the ruler did nothing. Now the name is
 * typed into the lane's own box, opened at the point clicked. Any dialog
 * is dismissed, which is the answer that webview gives, and recorded.
 */
test.describe("a marker added from the ruler", () => {
  /** Click the ruler `fraction` of the way across its ticks. */
  async function clickRuler(app: App, fraction: number): Promise<{ x: number }> {
    const box = (await app.page.getByTestId("ruler").boundingBox())!;
    // The strip begins with a 132px spacer; the ticks are the rest.
    const x = 132 + (box.width - 132) * fraction;
    await app.page.getByTestId("ruler").click({ position: { x, y: box.height / 2 } });
    return { x: box.x + x };
  }

  test("is named in the lane's box where it was clicked, with no dialog", async ({ app }) => {
    await openProject(app);
    const dialogs: string[] = [];
    app.page.on("dialog", (d) => {
      dialogs.push(d.message());
      void d.dismiss();
    });
    await app.become({ add_marker: ok(LABELLED) });

    const { x } = await clickRuler(app, 0.5);

    // The box is open and focused, over the point clicked, and nothing has
    // been added: the name is still to be typed.
    const input = app.page.getByTestId("label-draft-input");
    await expect(input).toBeFocused();
    const draft = (await app.page.getByTestId("label-draft").boundingBox())!;
    expect(Math.abs(draft.x - x)).toBeLessThan(4);
    expect(await app.requestsFor("add_marker")).toEqual([]);

    await input.fill("verse");
    await input.press("Enter");

    // One `add_marker`, at the point clicked: half way across 3 seconds.
    await expect.poll(() => app.requestsFor("add_marker")).toHaveLength(1);
    const [sent] = (await app.requestsFor("add_marker")) as { time: number; name: string }[];
    expect(sent.name).toBe("verse");
    expect(Math.abs(sent.time - 1.5)).toBeLessThan(0.1);

    // Its node is the head the next edit starts from.
    await app.become({ ...sessionWith([toneTrack()]), list_markers: ok([verse(sent.time)]) });
    await app.emit("marker-changed");
    await expect(app.page.getByTestId("label-chip")).toHaveAttribute("data-label-name", "verse");
    await app.page.getByTestId("render-preview-button").click();
    await expect.poll(() => app.requestsFor("render_preview")).toEqual([{ node: LABELLED }]);

    expect(dialogs).toEqual([]);
  });

  test("is abandoned with Escape, and adds nothing", async ({ app }) => {
    await openProject(app);
    app.page.on("dialog", (d) => void d.dismiss());

    await clickRuler(app, 0.3);
    const input = app.page.getByTestId("label-draft-input");
    await expect(input).toBeFocused();
    await input.fill("nope");
    await input.press("Escape");

    await expect(input).toHaveCount(0);
    await app.settle();
    expect(await app.requestsFor("add_marker")).toEqual([]);
  });
});

/** A region label is drawn with its extent, on the ruler's axis. */
test.describe("a region label in the lane", () => {
  const region = {
    id: "9a8b7c6d-5e4f-4a3b-8c2d-1e0f9a8b7c6d",
    name: "verse",
    kind: "region",
    start_sec: 0.75,
    end_sec: 2.25,
  };

  test("is drawn from its start to its end", async ({ app }) => {
    await app.boot({
      ...projectWith([toneTrack()], BEFORE),
      ...selecting(),
      list_markers: ok([region]),
      render_preview: ok(fixturePath("tone3s")),
    });
    await expect(app.page.getByTestId("ruler")).toContainText("0:03");

    const lane = (await app.page.getByTestId("label-lane-track").boundingBox())!;
    const bar = app.page.getByTestId("label-extent");
    await expect(bar).toHaveCount(1);
    const b = (await bar.boundingBox())!;
    // 3 s of audio across the lane: 0.75 s in, 1.5 s long.
    expect(Math.abs((b.x - lane.x) / lane.width - 0.25)).toBeLessThan(0.01);
    expect(Math.abs(b.width / lane.width - 0.5)).toBeLessThan(0.01);
  });

  test("is moved whole: the lane sends one time and the backend keeps the length", async ({
    app,
  }) => {
    await app.boot({
      ...projectWith([toneTrack()], BEFORE),
      ...selecting(),
      list_markers: ok([region]),
      render_preview: ok(fixturePath("tone3s")),
      update_marker: ok(LABELLED),
    });
    await expect(app.page.getByTestId("ruler")).toContainText("0:03");

    const chip = app.page.getByTestId("label-chip-button");
    const from = (await chip.boundingBox())!;
    const lane = (await app.page.getByTestId("label-lane-track").boundingBox())!;
    await app.page.mouse.move(from.x + 4, from.y + from.height / 2);
    await app.page.mouse.down();
    await app.page.mouse.move(lane.x + lane.width * 0.6, from.y + from.height / 2, { steps: 5 });
    await app.page.mouse.up();

    await expect.poll(() => app.requestsFor("update_marker")).toHaveLength(1);
    const [sent] = (await app.requestsFor("update_marker")) as { id: string; time: number }[];
    // One time, never start and end: a region is moved by its start.
    expect(Object.keys(sent).sort()).toEqual(["id", "time"]);
    expect(sent.id).toBe(region.id);
    expect(Math.abs(sent.time - 1.8)).toBeLessThan(0.1);
  });
});

/** Rename and delete in the lane reach the session as the edits they are. */
test.describe("a label in the lane", () => {
  test("is renamed in place and deleted with a right-click", async ({ app }) => {
    await app.boot({
      ...projectWith([toneTrack()], BEFORE),
      ...selecting(),
      list_markers: ok([verse(1.5)]),
      render_preview: ok(fixturePath("tone3s")),
      update_marker: ok(LABELLED),
      remove_marker: ok(nodeId(8)),
    });
    await expect(app.page.getByTestId("ruler")).toContainText("0:03");

    await app.page.getByTestId("label-chip-button").dblclick();
    const input = app.page.getByTestId("label-chip-input");
    await input.fill("chorus");
    await input.press("Enter");
    await expect
      .poll(() => app.requestsFor("update_marker"))
      .toEqual([{ id: verse(1.5).id, name: "chorus" }]);

    await app.page.getByTestId("label-chip-button").click({ button: "right" });
    await expect
      .poll(() => app.requestsFor("remove_marker"))
      .toEqual([{ id: verse(1.5).id }]);
  });
});
