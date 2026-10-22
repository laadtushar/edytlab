/**
 * Accept B of an A/B compare reads the lanes again (#596, ADR 0001).
 *
 * Accept B moves the session head to side B. Nothing else reads the track
 * list on a head move, so the lanes went on showing side A's tracks, and a
 * lane control the user held stayed laid over them. A move the session
 * makes by itself lets go of what the user holds and reads the tracks
 * again before anything is drawn (`owned.clear()`, then `refreshTracks`).
 *
 * `App.sessionMoves.test.tsx` holds the same rule in jsdom; this is the
 * real bar, the real key events and the real order of the answers, in a
 * real browser.
 */

import { fixtureSeconds, fixturePath } from "./audio-fixtures";
import {
  acceptingB,
  comparing,
  deferred,
  nodeId,
  ok,
  projectWith,
  toneTrack,
  trackFor,
} from "./backend";
import { expect, test, type App } from "./fixtures";

const A = nodeId(1);
const B = nodeId(2);

/** Side A: the tone track as the session lists it with `gain` dB on it. */
const sideA = (gain = 0) => [{ ...toneTrack(), gain_db: gain }];

/** Side B: the same track, louder, and a second one. */
const sideB = () => [
  { ...toneTrack(), gain_db: 6 },
  trackFor(fixturePath("tone3sB"), fixtureSeconds("tone3sB"), {
    name: "Music",
    id: "9a8b7c6d-5e4f-4a3b-8c2d-1e0f9a8b7c6d",
  }),
];

/** Boot on side A with a comparison of the head and B ready to open. */
async function bootComparing(app: App) {
  await app.boot({
    ...projectWith(sideA(), A),
    // `get_graph`: a load, and one edit after it.
    get_graph: ok({
      nodes: [
        { id: A, parent: null, label: "load", tool: "load", created_at: "2026-09-25T00:00:00Z" },
        { id: B, parent: A, label: "gain", tool: "gain", created_at: "2026-09-25T00:01:00Z" },
      ],
      head: A,
    }),
    ...comparing({
      a_path: fixturePath("tone3s"),
      b_path: fixturePath("tone3sB"),
      a_warnings: [],
      b_warnings: [],
    }),
    ...acceptingB(B),
  });
  const page = app.page;
  await expect(page.getByTestId("ruler")).toContainText("0:03");
  await expect(page.getByTestId("timeline-lane-name")).toHaveText(["Track 1"]);

  // Compare the head with the edit after it, from the graph; the
  // comparison plays on the timeline.
  await page.getByTestId("tab-graph").click();
  await page.getByTestId("graph-bubble").nth(1).click({ button: "right" });
  await page.getByTestId("graph-context-menu").getByText("Compare with…").click();
  await expect(page.getByTestId("ab-compare-bar")).toBeVisible();
  await expect(page.getByTestId("ab-accept-b")).toBeEnabled();
  await page.getByTestId("tab-timeline").click();
  await expect(page.getByTestId("timeline-lane-name")).toHaveText(["Track 1"]);
}

test("Accept B shows side B's tracks in the lanes", async ({ app }) => {
  await bootComparing(app);
  const page = app.page;
  const reads = (await app.requestsFor("list_tracks")).length;

  // The session lists side B's tracks once it is on B.
  await app.become({ list_tracks: ok(sideB()) });
  await page.getByTestId("ab-accept-b").click();

  await expect(page.getByTestId("timeline-lane-name")).toHaveText(["Track 1", "Music"]);
  await expect(page.getByTestId("timeline-lane-gain-readout").first()).toContainText("+6.0 dB");
  await expect(page.getByTestId("status-bar-head")).toContainText(B.slice(0, 7));
  await expect(page.getByTestId("ab-compare-bar")).toBeHidden();
  expect(await app.requestsFor("accept_b")).toEqual([{ b: B }]);
  expect((await app.requestsFor("list_tracks")).length).toBeGreaterThan(reads);
});

test("a read-back still on its way when B is accepted does not bring side A's tracks back", async ({
  app,
}) => {
  await bootComparing(app);
  const page = app.page;
  const gain = page.getByTestId("timeline-lane-gain");

  // A tap on the gain slider, committed; its read-back is held.
  await app.become({
    set_track_gain: ok(nodeId(3)),
    list_tracks: deferred("read-back of the tap"),
  });
  await gain.focus();
  await page.keyboard.press("ArrowRight");
  await expect.poll(() => app.requestsFor("set_track_gain")).toEqual([{ track: 0, gainDb: 0.5 }]);
  await expect(page.getByTestId("timeline-lane-gain-readout")).toContainText("+0.5 dB");

  // B is accepted. The session lists side B's tracks from here on.
  await app.become({ list_tracks: ok(sideB()) });
  await page.getByTestId("ab-accept-b").click();
  await expect(page.getByTestId("timeline-lane-name")).toHaveText(["Track 1", "Music"]);
  await expect(page.getByTestId("timeline-lane-gain-readout").first()).toContainText("+6.0 dB");

  // The read-back of the tap lands now, with side A's list as it was.
  await app.release("read-back of the tap", sideA(0.5));
  await app.settle();
  await expect(page.getByTestId("timeline-lane-name")).toHaveText(["Track 1", "Music"]);
  await expect(page.getByTestId("timeline-lane-gain-readout").first()).toContainText("+6.0 dB");
});
