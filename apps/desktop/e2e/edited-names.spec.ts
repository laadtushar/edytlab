/**
 * An edited track keeps a name a person knows (#416).
 *
 * A destructive edit — reverse, stretch, normalise, a fade baked in —
 * writes its result under `.audiograph/derived/`, named by the hash of
 * its samples, and points the clip at it. The clip chip and the status
 * bar both named that file, so after reversing `music-8s-stereo.wav` the
 * chip read `703e2a7b…a370e.wav` and the bar the same in capitals, while
 * the lane header still said the track's name.
 *
 * Found in a native run of the real app; here it is the real frontend
 * with the backend answering as `list_tracks` does after such an edit.
 */

import { fixturePath, fixtureSeconds } from "./audio-fixtures";
import { nodeId, projectWith, sessionWith, trackFor } from "./backend";
import { expect, test } from "./fixtures";

/** What `load` names a track: the file's stem. */
const TRACK = "tone-3s";
/** Twelve hex digits in a row: a hash, which no name of ours contains. */
const HASH = /[0-9a-f]{12}/i;

test("an edited clip and the status bar name the track, not the derived file", async ({ app }) => {
  const seconds = fixtureSeconds("tone3s");
  await app.boot(
    projectWith([trackFor(fixturePath("tone3s"), seconds, { name: TRACK })], nodeId(1)),
  );
  const page = app.page;
  const chip = page.getByTestId("clip-chip-0");
  const file = page.getByTestId("status-bar-file");

  // Before the edit: the file the user loaded, by its own name.
  await expect(chip).toHaveText("tone-3s.wav");
  await expect(file).toHaveText("tone-3s.wav");

  // The edit lands: one clip, the whole of the derived file, at zero, so
  // `list_tracks` hands that same file to the lane.
  await app.become(
    sessionWith([
      trackFor(fixturePath("tone3sEdited"), fixtureSeconds("tone3sEdited"), { name: TRACK }),
    ]),
  );
  await app.emit("agent://node-created", { node_id: nodeId(2) });

  await expect(chip).toHaveText(TRACK);
  await expect(chip).toHaveAttribute("aria-label", `${TRACK}, 0.00 to 3.00 seconds`);
  await expect(file).toHaveText(TRACK);
  expect(await file.getAttribute("title")).not.toMatch(HASH);
  await expect(page.getByTestId("clip-strip")).not.toContainText(HASH);
  await expect(page.getByTestId("status-bar")).not.toContainText(HASH);
});
