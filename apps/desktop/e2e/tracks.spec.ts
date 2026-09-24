/**
 * The track list is refreshed in one place, and a reply can neither be an
 * error it is not nor overwrite a newer one (#341, #342).
 */

import {
  deferred,
  NO_SESSION,
  nodeId,
  ok,
  readyToLoad,
  reject,
  sessionWith,
  toneTrack,
} from "./backend";
import { expect, test } from "./fixtures";

/**
 * #341. A new folder gets a store with no `HEAD`, so `list_tracks`
 * answers `NoSession` — the ordinary state of a project with nothing in it
 * yet, which boot already treated as an empty list. "New project…" and
 * "Open project…" showed it as a failure instead: the user picked a folder
 * and was told to call `open_project` first, which they just had.
 */
test.describe("opening a project with nothing in it", () => {
  test("shows the empty state, not an error", async ({ app }) => {
    const folder = "/home/user/Music/fresh";
    await app.boot(readyToLoad());
    const page = app.page;
    await app.become({
      // The folder picker, answered with the chosen folder.
      "plugin:dialog|open": ok(folder),
      // `open_project` on an empty folder: a store with no head yet.
      open_project: ok({ path: folder, head: null }),
      // `ViewState::default()` serialises as `{}`.
      get_view_state: ok({}),
      list_tracks: reject(NO_SESSION),
    });
    await page.getByTestId("open-project-button").click();

    await expect(page.getByTestId("empty-state")).toBeVisible();
    await app.settle();
    await expect(page.getByTestId("render-error")).toHaveCount(0);
  });

  test("still reports a folder that cannot be opened", async ({ app }) => {
    const folder = "/home/user/Music/locked";
    await app.boot(readyToLoad());
    const page = app.page;
    await app.become({
      "plugin:dialog|open": ok(folder),
      open_project: reject("permission denied: /home/user/Music/locked"),
    });
    await page.getByTestId("open-project-button").click();

    await expect(page.getByTestId("render-error")).toContainText("permission denied");
  });
});

/**
 * #342. Boot lists the tracks once; an edit's `node-created` lists them
 * again. Nothing ordered the two replies, so a boot reply that arrived
 * second replaced the newer list with the older one — the timeline showed
 * the session from before the edit, with no error.
 */
test.describe("two track listings in flight", () => {
  test("the older reply does not replace the newer one", async ({ app }) => {
    await app.boot({ ...readyToLoad(), list_tracks: deferred("boot listing") });
    const page = app.page;

    // An edit lands while boot's listing is still out; its listing answers
    // at once, with the track the edit made.
    await app.become(sessionWith([toneTrack()]));
    await app.emit("agent://node-created", { node_id: nodeId(3) });
    await expect(page.getByTestId("ruler")).toContainText("0:03");

    // Boot's reply finally arrives, describing the state before the edit.
    await app.release("boot listing", []);

    await app.settle();
    await expect(page.getByTestId("ruler")).toContainText("0:03");
    await expect(page.getByTestId("empty-state")).toHaveCount(0);
  });
});
