/**
 * Opening a file loads it — no model involved (#321).
 *
 * A single picked or dropped file used to be sent to the agent as the
 * sentence "load this file: …", and the timeline drew the picked path
 * straight away. With no working model — offline, no key yet, or a
 * local model that cannot call tools — the waveform and "ready" showed
 * while the session stayed empty: every edit afterwards failed, and
 * nothing on screen said why.
 *
 * Every open path now goes through `batch_load`, the command that runs
 * the same `load` tool the agent uses. `send_message` is deliberately
 * left unanswered here: a call to it fails the test at teardown.
 */

import { fixturePath, fixtureSeconds } from "./audio-fixtures";
import { nodeId, ok, readyToLoad, reject, sessionWith, toneTrack } from "./backend";
import { expect, test } from "./fixtures";

const TONE = fixturePath("tone3s");

test.describe("opening one audio file", () => {
  test("loads it into the session without asking the agent", async ({ app }) => {
    const loaded = nodeId(5);
    await app.boot({
      ...readyToLoad(),
      // "Open Audio…" asks for several files, so the picker answers a list.
      "plugin:dialog|open": ok([TONE]),
      // `batch_load` answers `BatchLoadResult`.
      batch_load: ok({ tracks_loaded: 1, last_node_id: loaded }),
      render_preview: ok(TONE),
    });
    const page = app.page;
    await expect(page.getByTestId("empty-state")).toBeVisible();

    // Once loaded, the backend lists the file as a track.
    await app.become(sessionWith([toneTrack()]));
    await page.getByTestId("empty-state-open-button").click();

    await expect(page.getByTestId("ruler")).toContainText("0:03");
    expect(await app.requestsFor("batch_load")).toEqual([{ paths: [TONE] }]);

    // The node the load appended is the head the app works from.
    await page.getByTestId("render-preview-button").click();
    await expect
      .poll(() => app.requestsFor("render_preview"), { message: "a render of the loaded node" })
      .toEqual([{ node: loaded }]);
  });

  test("says which file did not load, and does not pretend it did", async ({ app }) => {
    await app.boot({
      ...readyToLoad(),
      "plugin:dialog|open": ok([TONE]),
      // What `batch_load` returns when the `load` tool refuses a file.
      batch_load: reject("load tool error: unsupported audio format"),
    });
    const page = app.page;
    await page.getByTestId("empty-state-open-button").click();

    const banner = page.getByTestId("render-error");
    await expect(banner).toContainText("tone-3s.wav");
    await expect(banner).toContainText("unsupported audio format");
    // No timeline, no waveform of a file the session does not hold.
    await expect(page.getByTestId("empty-state")).toBeVisible();
    await expect(page.getByTestId("ruler")).toHaveCount(0);
  });
});

test.describe("opening several files", () => {
  test("loads them all in one call, as tracks", async ({ app }) => {
    const second = fixturePath("tone3s");
    await app.boot({
      ...readyToLoad(),
      "plugin:dialog|open": ok([TONE, second]),
      batch_load: ok({ tracks_loaded: 2, last_node_id: nodeId(6) }),
    });
    const page = app.page;
    await app.become(sessionWith([toneTrack()]));
    await page.getByTestId("empty-state-open-button").click();

    await expect(page.getByTestId("ruler")).toContainText("0:03");
    expect(await app.requestsFor("batch_load")).toEqual([{ paths: [TONE, second] }]);
  });
});

// Keeps `fixtureSeconds` honest about the fixture the assertions assume.
test.beforeAll(() => {
  expect(fixtureSeconds("tone3s")).toBe(3);
});
