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
 * that node's *original* parent (crates/session/tests/linear_ops.rs,
 * `a_state_reached_again_keeps_its_first_parent`, holds the store to it).
 *
 * Also here: what the path has to do besides undo. Redo is part of it
 * (a new edit after an undo starts a new branch, so there is nothing to
 * redo), opening another project starts a new path, and the history graph
 * draws the path the user took. "Set as head" in the graph is a step on
 * that path too (#453): it goes through the app, so undo comes back from
 * it.
 *
 * The path is saved with the project (#453), so it survives a restart.
 * A restart here is `app.restart`: the page is loaded again and the
 * backend is told what the project holds, which is the head it ended on
 * and the path the app asked it to keep. Nothing else carries over.
 */

import { fixturePath, fixtureSeconds } from "./audio-fixtures";
import type { SavedTrail } from "../src/lib/headTrail";

import {
  nodeId,
  ok,
  projectWith,
  reject,
  sessionNode,
  sessionWith,
  toneTrack,
  trackFor,
} from "./backend";
import { expect, test, type App } from "./fixtures";

const BEFORE_LOAD = nodeId(1); // the parent the store recorded for START
const START = nodeId(2); // two tracks loaded
const MUTED = nodeId(3);
const DUPLICATED = nodeId(4);
const SOLOED = nodeId(5);

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

test("duplicate, remove the copy, then undo brings the copy back", async ({ app }) => {
  // The removal returns to the state before the duplicate: the same node,
  // START, whose stored parent is the load before it. Undo by parent could
  // never bring the copy back.
  const copy = trackFor(fixturePath("tone3s"), fixtureSeconds("tone3s"), {
    name: "Track 1 copy",
    id: "9a8b7c6d-5e4f-4a3b-8c2d-1e0f9a8b7c6d",
  });
  await app.boot({
    ...projectWith([toneTrack()], START),
    render_preview: ok(fixturePath("tone3s")),
    get_node: ok(sessionNode(START, BEFORE_LOAD)),
    duplicate_track: ok(DUPLICATED),
    remove_track: ok(START),
    set_head_to: ok(DUPLICATED),
  });
  const page = app.page;
  const lanes = page.getByTestId("timeline-lane");
  await expect(lanes).toHaveCount(1);

  await app.become(sessionWith([toneTrack(), copy]));
  await page.getByTestId("track-menu-btn-0").click();
  await page.getByTestId("track-duplicate-0").click();
  await expect.poll(() => app.requestsFor("duplicate_track")).toEqual([{ track: 0 }]);
  await expect(lanes).toHaveCount(2);

  await app.become(sessionWith([toneTrack()]));
  await page.getByTestId("track-menu-btn-1").click();
  await page.getByTestId("track-remove-1").click();
  await expect.poll(() => app.requestsFor("remove_track")).toEqual([{ track: 1 }]);
  await expect(lanes).toHaveCount(1);
  await app.settle();

  // Undo: the duplicate, with its copy.
  await app.become(sessionWith([toneTrack(), copy]));
  await page.keyboard.press("Control+z");
  await expect.poll(() => app.requestsFor("set_head_to")).toEqual([{ nodeId: DUPLICATED }]);
  await expect(lanes).toHaveCount(2);
  expect(await app.requestsFor("get_node")).toEqual([]);
});

test("a new edit after an undo leaves nothing to redo", async ({ app }) => {
  await app.boot({
    ...projectWith([toneTrack()], START),
    render_preview: ok(fixturePath("tone3s")),
    // The store's answer for the node undo leaves, for a build that asks.
    get_node: ok(sessionNode(MUTED, START)),
    set_track_muted: ok(MUTED),
    set_track_soloed: ok(SOLOED),
    set_head_to: ok(START),
  });
  const page = app.page;
  await expect(page.getByTestId("ruler")).toContainText("0:03");

  await page.getByTestId("timeline-lane-mute").click();
  await expect.poll(() => app.requestsFor("set_track_muted")).toHaveLength(1);
  await app.settle();

  await page.keyboard.press("Control+z");
  await expect.poll(() => app.requestsFor("set_head_to")).toEqual([{ nodeId: START }]);
  await app.settle();

  // A different edit now: the muted state is no longer ahead of the user.
  await page.getByTestId("timeline-lane-solo").click();
  await expect.poll(() => app.requestsFor("set_track_soloed")).toHaveLength(1);
  await app.settle();

  await app.become({ set_head_to: ok(MUTED) });
  await page.keyboard.press("Control+y");
  await app.settle();
  expect(await app.requestsFor("set_head_to")).toEqual([{ nodeId: START }]);
});

test("opening another project forgets the old project's path", async ({ app }) => {
  const A0 = nodeId(0xa0);
  const A1 = nodeId(0xa1);
  const B0 = nodeId(0xb0);
  const B1 = nodeId(0xb1);
  const other = "/home/user/Music/other";
  await app.boot({
    ...projectWith([toneTrack()], A1),
    render_preview: ok(fixturePath("tone3s")),
    get_node: ok(sessionNode(A1, A0)),
    set_head_to: ok(A0),
  });
  const page = app.page;
  await expect(page.getByTestId("ruler")).toContainText("0:03");

  // Undo in project A: its redo list now holds A1.
  await page.keyboard.press("Control+z");
  await expect.poll(() => app.requestsFor("set_head_to")).toEqual([{ nodeId: A0 }]);
  await app.settle();

  // Open project B, which holds one node after its first.
  await app.become({
    "plugin:dialog|open": ok(other),
    open_project: ok({ path: other, head: B1 }),
    get_view_state: ok({}),
    ...sessionWith([toneTrack()]),
    get_node: ok(sessionNode(B1, B0)),
    set_head_to: ok(B0),
  });
  await page.getByTestId("open-project-button").click();
  await expect.poll(() => app.requestsFor("open_project")).toEqual([{ path: other }]);
  await app.settle();

  // Project A's redo does not carry over: nothing is sent, and certainly
  // not a node project B does not have.
  await page.keyboard.press("Control+y");
  await app.settle();
  expect(await app.requestsFor("set_head_to")).toEqual([{ nodeId: A0 }]);

  // And undo is B's own: its head's parent, not project A's trail.
  await page.keyboard.press("Control+z");
  await expect
    .poll(() => app.requestsFor("set_head_to"))
    .toEqual([{ nodeId: A0 }, { nodeId: B0 }]);
});

test("the history graph draws the path taken", async ({ app }) => {
  await app.boot({
    ...projectWith([toneTrack()], START),
    render_preview: ok(fixturePath("tone3s")),
    set_track_muted: ok(MUTED),
  });
  const page = app.page;
  await expect(page.getByTestId("ruler")).toContainText("0:03");

  await page.getByTestId("timeline-lane-mute").click();
  await expect.poll(() => app.requestsFor("set_track_muted")).toHaveLength(1);
  await app.become({ set_track_muted: ok(START) });
  await page.getByTestId("timeline-lane-mute").click();
  await expect.poll(() => app.requestsFor("set_track_muted")).toHaveLength(2);
  await app.settle();

  // The store holds one node per state: the unmute is START again, and
  // MUTED's parent is START. The graph has no edge from MUTED back.
  const at = (n: number) => `2026-10-09T00:0${n}:00Z`;
  await app.become({
    get_graph: ok({
      nodes: [
        { id: BEFORE_LOAD, parent: null, label: "load", tool: "load", created_at: at(0) },
        { id: START, parent: BEFORE_LOAD, label: "load", tool: "load", created_at: at(1) },
        { id: MUTED, parent: START, label: "mute", tool: "set_track_muted", created_at: at(2) },
      ],
      head: START,
    }),
  });
  await page.getByTestId("tab-graph").click();
  await expect(page.getByTestId("graph-bubble")).toHaveCount(3);

  // The step the parent edges do not contain: unmuting.
  await expect(page.getByTestId(`rf__edge-path:${MUTED}->${START}`)).toHaveCount(1);
  // And the edge that was walked on the way there is marked.
  await expect(page.getByTestId(`rf__edge-${START}->${MUTED}`)).toHaveClass(/graph-path-step/);
  // The edge nobody walked is not.
  await expect(page.getByTestId(`rf__edge-${BEFORE_LOAD}->${START}`)).not.toHaveClass(
    /graph-path-step/,
  );
});

test("Set as head in the graph is a step: undo comes back from it, redo goes again (#453)", async ({
  app,
}) => {
  // The graph used to call `set_head_to` itself. The app never learned of
  // the jump, so Ctrl+Z from there went to where the head was before it
  // (START), not back to the node the user jumped from (MUTED).
  await app.boot({
    ...projectWith([toneTrack()], START),
    render_preview: ok(fixturePath("tone3s")),
    set_track_muted: ok(MUTED),
    // The store's answer for the node undo leaves, for a build that asks.
    get_node: ok(sessionNode(START, BEFORE_LOAD)),
  });
  const page = app.page;
  await expect(page.getByTestId("ruler")).toContainText("0:03");

  // The head is MUTED and the path so far is [START].
  await page.getByTestId("timeline-lane-mute").click();
  await expect.poll(() => app.requestsFor("set_track_muted")).toHaveLength(1);
  await app.settle();

  const at = (n: number) => `2026-10-09T00:0${n}:00Z`;
  await app.become({
    get_graph: ok({
      nodes: [
        { id: BEFORE_LOAD, parent: null, label: "load", tool: "load", created_at: at(0) },
        { id: START, parent: BEFORE_LOAD, label: "load", tool: "load", created_at: at(1) },
        { id: MUTED, parent: START, label: "mute", tool: "set_track_muted", created_at: at(2) },
      ],
      head: MUTED,
    }),
    set_head_to: ok(BEFORE_LOAD),
  });
  await page.getByTestId("tab-graph").click();
  await expect(page.getByTestId("graph-bubble")).toHaveCount(3);

  // Jump to the oldest node from its context menu.
  const oldest = page.locator(`[data-testid="graph-bubble"][data-node-id="${BEFORE_LOAD}"]`);
  await oldest.click({ button: "right" });
  await page.getByRole("menuitem", { name: "Set as head" }).click();
  await expect.poll(() => app.requestsFor("set_head_to")).toEqual([{ nodeId: BEFORE_LOAD }]);

  // The head ring moved, and the jump is on the path: a step MUTED ->
  // BEFORE_LOAD that the parent edges do not contain.
  await expect(oldest).toHaveAttribute("data-is-head", "true");
  await expect(page.getByTestId(`rf__edge-path:${MUTED}->${BEFORE_LOAD}`)).toHaveCount(1);
  await app.settle();

  // Undo comes back from the jump: to MUTED, which the path holds, and
  // not to START, where the head was before it. No parent lookup either.
  await app.become({ set_head_to: ok(MUTED) });
  await page.keyboard.press("Control+z");
  await expect
    .poll(() => app.requestsFor("set_head_to"))
    .toEqual([{ nodeId: BEFORE_LOAD }, { nodeId: MUTED }]);
  expect(await app.requestsFor("get_node")).toEqual([]);
  await app.settle();

  // And redo goes to the jump again.
  await app.become({ set_head_to: ok(BEFORE_LOAD) });
  await page.keyboard.press("Control+y");
  await expect
    .poll(() => app.requestsFor("set_head_to"))
    .toEqual([{ nodeId: BEFORE_LOAD }, { nodeId: MUTED }, { nodeId: BEFORE_LOAD }]);
});

/** What the app asked `save_head_trail` to keep, newest last. */
async function savedPaths(app: App): Promise<SavedTrail[]> {
  const calls = (await app.requestsFor("save_head_trail")) as { trail: SavedTrail }[];
  return calls.map((c) => c.trail);
}

test("the path survives a restart: mute, unmute, close, open, undo brings the mute back (#453)", async ({
  app,
}) => {
  // The path used to live in the webview alone. After a restart undo asked
  // the node for its parent, which skips the mute (#398).
  await app.boot({
    ...projectWith([toneTrack()], START),
    render_preview: ok(fixturePath("tone3s")),
    set_track_muted: ok(MUTED),
  });
  const page = app.page;
  await expect(page.getByTestId("ruler")).toContainText("0:03");

  await page.getByTestId("timeline-lane-mute").click();
  await expect.poll(() => app.requestsFor("set_track_muted")).toHaveLength(1);
  await app.become({ set_track_muted: ok(START) });
  await page.getByTestId("timeline-lane-mute").click();
  await expect.poll(() => app.requestsFor("set_track_muted")).toHaveLength(2);
  await app.settle();

  // Each move saved the whole path with the project; the last one ends at
  // the head the project is on.
  const saves = await savedPaths(app);
  expect(saves).toEqual([
    { head: MUTED, trail: [START], redo: [] },
    { head: START, trail: [START, MUTED], redo: [] },
  ]);

  // The app is closed and opened again. The project is as it was left:
  // the same head, and the file the app saved.
  await app.restart({
    ...projectWith([toneTrack()], START),
    render_preview: ok(fixturePath("tone3s")),
    get_head_trail: ok(saves.at(-1)),
    // The store's answer for the node undo leaves, for a build that asks.
    get_node: ok(sessionNode(START, BEFORE_LOAD)),
    set_head_to: ok(MUTED),
  });
  await expect(page.getByTestId("ruler")).toContainText("0:03");
  await app.settle();

  // First undo: the mute, from the saved path, with no need to ask the
  // graph for a parent.
  await page.keyboard.press("Control+z");
  await expect.poll(() => app.requestsFor("set_head_to")).toEqual([{ nodeId: MUTED }]);
  await app.settle();
  expect(await app.requestsFor("get_node")).toEqual([]);

  // Second undo: the state before the mute, which is START again.
  await app.become({ set_head_to: ok(START) });
  await page.keyboard.press("Control+z");
  await expect
    .poll(() => app.requestsFor("set_head_to"))
    .toEqual([{ nodeId: MUTED }, { nodeId: START }]);
  await app.settle();
  expect(await app.requestsFor("get_node")).toEqual([]);

  // The path is spent: now the stored parent is the right answer.
  await app.become({ set_head_to: ok(BEFORE_LOAD) });
  await page.keyboard.press("Control+z");
  await expect
    .poll(() => app.requestsFor("set_head_to"))
    .toEqual([{ nodeId: MUTED }, { nodeId: START }, { nodeId: BEFORE_LOAD }]);
  expect(await app.requestsFor("get_node")).toEqual([{ id: START }]);
});

test("the redo list survives a restart too (#453)", async ({ app }) => {
  await app.boot({
    ...projectWith([toneTrack()], START),
    render_preview: ok(fixturePath("tone3s")),
    set_track_muted: ok(MUTED),
    set_head_to: ok(START),
  });
  const page = app.page;
  await expect(page.getByTestId("ruler")).toContainText("0:03");

  await page.getByTestId("timeline-lane-mute").click();
  await expect.poll(() => app.requestsFor("set_track_muted")).toHaveLength(1);
  await app.settle();
  await page.keyboard.press("Control+z"); // back to START, with the mute to redo
  await expect.poll(() => app.requestsFor("set_head_to")).toEqual([{ nodeId: START }]);
  await app.settle();

  const saved = (await savedPaths(app)).at(-1);
  expect(saved).toEqual({ head: START, trail: [], redo: [MUTED] });

  await app.restart({
    ...projectWith([toneTrack()], START),
    render_preview: ok(fixturePath("tone3s")),
    get_head_trail: ok(saved),
    set_head_to: ok(MUTED),
  });
  await expect(page.getByTestId("ruler")).toContainText("0:03");
  await app.settle();

  await page.keyboard.press("Control+y");
  await expect.poll(() => app.requestsFor("set_head_to")).toEqual([{ nodeId: MUTED }]);
});

test("a saved path that ends at another head is not used (#453)", async ({ app }) => {
  // The save for the last move was lost: the file ends one move behind
  // the store. It is not the way to this head, so undo asks for the parent.
  await app.boot({
    ...projectWith([toneTrack()], START),
    render_preview: ok(fixturePath("tone3s")),
    get_head_trail: ok({ head: MUTED, trail: [START], redo: [] }),
    get_node: ok(sessionNode(START, BEFORE_LOAD)),
    set_head_to: ok(BEFORE_LOAD),
  });
  const page = app.page;
  await expect(page.getByTestId("ruler")).toContainText("0:03");
  await app.settle();

  await page.keyboard.press("Control+z");
  await expect.poll(() => app.requestsFor("set_head_to")).toEqual([{ nodeId: BEFORE_LOAD }]);
  expect(await app.requestsFor("get_node")).toEqual([{ id: START }]);
});

test("a project that cannot read its path still opens, and undo uses the parent (#453)", async ({
  app,
}) => {
  await app.boot({
    ...projectWith([toneTrack()], START),
    render_preview: ok(fixturePath("tone3s")),
    get_head_trail: reject("could not read the project"),
    get_node: ok(sessionNode(START, BEFORE_LOAD)),
    set_head_to: ok(BEFORE_LOAD),
  });
  const page = app.page;
  await expect(page.getByTestId("ruler")).toContainText("0:03");
  await app.settle();

  await page.keyboard.press("Control+z");
  await expect.poll(() => app.requestsFor("set_head_to")).toEqual([{ nodeId: BEFORE_LOAD }]);
});

test("opening another project takes the path saved with that project (#453)", async ({ app }) => {
  const B0 = nodeId(0xb0);
  const B1 = nodeId(0xb1);
  const BM = nodeId(0xb2); // B1 muted
  const other = "/home/user/Music/other";
  await app.boot({
    ...projectWith([toneTrack()], START),
    render_preview: ok(fixturePath("tone3s")),
  });
  const page = app.page;
  await expect(page.getByTestId("ruler")).toContainText("0:03");
  await app.settle();

  // Project B was closed after a mute and an unmute: B1 again, with BM on
  // its path and B0 as its stored parent.
  await app.become({
    "plugin:dialog|open": ok(other),
    open_project: ok({ path: other, head: B1 }),
    get_view_state: ok({}),
    get_head_trail: ok({ head: B1, trail: [B1, BM], redo: [] }),
    ...sessionWith([toneTrack()]),
    get_node: ok(sessionNode(B1, B0)),
    set_head_to: ok(BM),
  });
  await page.getByTestId("open-project-button").click();
  await expect.poll(() => app.requestsFor("open_project")).toEqual([{ path: other }]);
  await app.settle();

  await page.keyboard.press("Control+z");
  await expect.poll(() => app.requestsFor("set_head_to")).toEqual([{ nodeId: BM }]);
  expect(await app.requestsFor("get_node")).toEqual([]);
});

test("opening a project saves nothing: the path is only read (#453)", async ({ app }) => {
  await app.boot({
    ...projectWith([toneTrack()], START),
    render_preview: ok(fixturePath("tone3s")),
    get_head_trail: ok({ head: START, trail: [START, MUTED], redo: [] }),
  });
  await expect(app.page.getByTestId("ruler")).toContainText("0:03");
  await app.settle();

  expect(await app.requestsFor("save_head_trail")).toEqual([]);
});
