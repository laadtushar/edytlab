/**
 * A lane control the user has set is not undone by a track list that was
 * already on its way (#596).
 *
 * Found by the native suite, in a real webview: an ArrowRight tap moves the
 * gain slider on key-down and commits it on key-up, and the read-back of
 * the tap before can land between the two. The slider jumped back one step,
 * and key-up then committed the value the session already held, which sends
 * nothing: a tap lost. A mute click shows the same transient.
 *
 * Here the order is fixed rather than raced: the read-back of the first
 * commit is held, the second tap's key goes down, and only then is the
 * read-back let go. `App.laneControls.test.tsx` holds the same order in
 * jsdom; this is the real key events, in a real browser.
 */

import { fixturePath } from "./audio-fixtures";
import { deferred, nodeId, ok, projectWith, toneTrack } from "./backend";
import { expect, test } from "./fixtures";

const HEAD = nodeId(2);

/** The tone track as the session lists it with `gain` dB on it. */
const withGain = (gain: number) => [{ ...toneTrack(), gain_db: gain }];

test("a key tap is not lost when the read-back of the tap before lands before its key-up", async ({
  app,
}) => {
  await app.boot({
    ...projectWith(withGain(0), HEAD),
    render_preview: ok(fixturePath("tone3s")),
    set_track_gain: ok(nodeId(3)),
  });
  const page = app.page;
  const gain = page.getByTestId("timeline-lane-gain");
  await expect(page.getByTestId("ruler")).toContainText("0:03");

  // From here the read-back of the next commit is held until it is let go.
  await app.become({ list_tracks: deferred("read-back of tap 1") });
  await gain.focus();
  await page.keyboard.press("ArrowRight"); // tap 1: +0.5 dB, committed
  await expect.poll(() => app.requestsFor("set_track_gain")).toEqual([{ track: 0, gainDb: 0.5 }]);
  await expect(gain).toHaveValue("0.5");

  // Tap 2: the key goes down and the slider moves; the key is not up yet.
  await page.keyboard.down("ArrowRight");
  await expect(gain).toHaveValue("1");

  // The read-back of tap 1 lands now, with the session as it was then.
  // The session answers the next read with tap 2's value.
  await app.become({ list_tracks: ok(withGain(1)) });
  await app.release("read-back of tap 1", withGain(0.5));
  await app.settle();
  await expect(gain).toHaveValue("1");
  await expect(page.getByTestId("timeline-lane-gain-readout")).toContainText("+1.0 dB");

  // Key-up: it carries 1, which the session does not hold yet, so it goes out.
  await page.keyboard.up("ArrowRight");
  await expect
    .poll(() => app.requestsFor("set_track_gain"))
    .toEqual([
      { track: 0, gainDb: 0.5 },
      { track: 0, gainDb: 1 },
    ]);
  await app.settle();
  await expect(gain).toHaveValue("1");
});

test("a mute is not shown undone by a track list read before the click", async ({ app }) => {
  await app.boot({
    ...projectWith(withGain(0), HEAD),
    render_preview: ok(fixturePath("tone3s")),
    set_track_gain: ok(nodeId(3)),
  });
  const page = app.page;
  const mute = page.getByTestId("timeline-lane-mute");
  await expect(page.getByTestId("ruler")).toContainText("0:03");

  // A read of the track list on its way: here, the read-back of a gain edit.
  await app.become({ list_tracks: deferred("earlier read") });
  await page.getByTestId("timeline-lane-gain").focus();
  await page.keyboard.press("ArrowRight");
  await expect.poll(() => app.requestsFor("set_track_gain")).toHaveLength(1);

  // The click is shown at once; the session has not been told yet.
  await app.become({ set_track_muted: deferred("mute reaches the session") });
  await mute.click();
  await expect(mute).toHaveAttribute("aria-pressed", "true");
  await expect.poll(() => app.requestsFor("set_track_muted")).toEqual([{ track: 0, muted: true }]);

  // The earlier read lands: the session before the click.
  await app.release("earlier read", withGain(0.5));
  await app.settle();
  await expect(mute).toHaveAttribute("aria-pressed", "true");

  // The mute reaches the session, and its own read-back follows.
  await app.become({ list_tracks: ok([{ ...toneTrack(), gain_db: 0.5, muted: true }]) });
  await app.release("mute reaches the session", nodeId(4));
  await app.settle();
  await expect(mute).toHaveAttribute("aria-pressed", "true");
  await expect(mute).toHaveText("muted");
});
