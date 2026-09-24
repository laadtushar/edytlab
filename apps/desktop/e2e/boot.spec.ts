/**
 * The app comes up in the states a user actually starts from.
 *
 * Asserted by what is on screen, found by booting the harness and
 * reading its page snapshot — not by what the components were expected
 * to show. The first draft of this file looked for the timeline's drop
 * hint, which a first launch never draws: it shows the empty-state
 * screen instead, with onboarding over it.
 */

import type { Page } from "@playwright/test";


import { fixturePath, fixtureSeconds } from "./audio-fixtures";
import {
  deferred,
  emptyTrack,
  firstRun,
  nodeId,
  ok,
  projectWith,
  readyToLoad,
  sessionWith,
  toneTrack,
  trackFor,
} from "./backend";
import { expect, test } from "./fixtures";

/**
 * Whether the lane's waveform has any drawn pixel.
 *
 * WaveSurfer only paints bars from decoded audio, so ink on its canvases
 * is proof the file was decoded. The ruler is not: with a clip, it takes
 * its length from the clip's metadata and reads the same whether decoding
 * worked or not. WaveSurfer's shadow root is `open`, which Playwright's
 * CSS locators pierce.
 */
function waveformHasInk(page: Page): Promise<boolean> {
  return page
    .getByTestId("timeline-lane-waveform")
    .locator("canvas")
    .evaluateAll((canvases) =>
      canvases.some((node) => {
        const canvas = node as HTMLCanvasElement;
        const ctx = canvas.width && canvas.height ? canvas.getContext("2d") : null;
        if (!ctx) return false;
        const { data } = ctx.getImageData(0, 0, canvas.width, canvas.height);
        for (let alpha = 3; alpha < data.length; alpha += 4) {
          if (data[alpha] !== 0) return true;
        }
        return false;
      }),
    );
}

test.describe("a first launch, with no project history and no key", () => {
  test("asks for a key over the empty state", async ({ app }) => {
    await app.boot(firstRun());
    const page = app.page;

    const onboarding = page.getByRole("dialog", { name: "Settings" });
    await expect(onboarding.getByRole("heading", { name: "Welcome to edytlab" })).toBeVisible();
    // Anthropic, because `AppState::new` starts there.
    await expect(onboarding.getByRole("textbox", { name: "Anthropic API key" })).toHaveValue("");
    // Nothing to save yet.
    await expect(onboarding.getByRole("button", { name: "Save & Continue" })).toBeDisabled();

    // The way in, behind it. Scoped to the empty state itself: the header
    // draws "New project…" and "Open project…" too, so a page-wide query
    // would still pass with these gone from the screen they belong on.
    const emptyState = page.getByTestId("empty-state");
    await expect(emptyState.getByRole("heading", { name: "edytlab", level: 1 })).toBeVisible();
    for (const name of ["New project…", "Open project…", "Start from template", "Open Audio…"]) {
      await expect(emptyState.getByRole("button", { name, exact: true })).toBeVisible();
    }
  });
});

test.describe("once the agent loads a file", () => {
  test("the lane decodes it in a real browser", async ({ app }) => {
    await app.boot(readyToLoad());
    const page = app.page;
    await expect(page.getByTestId("empty-state")).toBeVisible();

    // The agent's `load` tool wrote a session node; the backend now has
    // a track, and says so with the event it emits for every new node.
    await app.become(sessionWith([toneTrack()]));
    await app.emit("agent://node-created", { node_id: nodeId(1) });

    await expect(page.getByTestId("ruler")).toContainText("0:03");
    await expect.poll(() => waveformHasInk(page), { message: "the waveform was drawn" }).toBe(true);
    await expect(page.getByTestId("timeline-lane-error")).toHaveCount(0);
  });
});

/**
 * #332. The backend reopens the project and restores its head at every
 * launch, so a returning user's track is there before the first render —
 * and the app showed "Drop a file or pick one to begin" over it, because
 * nothing at boot turned the tracks it listed into a timeline.
 */
test.describe("a returning user", () => {
  test("boots into their project, with the track decoded", async ({ app }) => {
    await app.boot(
      projectWith([toneTrack()], nodeId(1)),
    );
    const page = app.page;

    await expect(page.getByTestId("empty-state")).toHaveCount(0);
    await expect(page.getByTestId("ruler")).toContainText("0:03");
    await expect.poll(() => waveformHasInk(page), { message: "the waveform was drawn" }).toBe(true);
  });

  /**
   * The head the backend restored is the one a preview must render. It
   * never reached the frontend: `useSession` learned the head only from
   * `node-created` events, and none fires for a head that was already
   * there — so "preview" did nothing at all, with no error.
   */
  test("can preview the mix of the state they left", async ({ app }) => {
    const head = nodeId(7);
    await app.boot({
      ...projectWith([toneTrack()], head),
      // `render_preview` answers with the rendered file's path.
      render_preview: ok(fixturePath("tone3s")),
    });
    const page = app.page;
    await expect(page.getByTestId("ruler")).toContainText("0:03");

    await page.getByTestId("render-preview-button").click();

    await expect
      .poll(() => app.requestsFor("render_preview"), { message: "a render of the restored head" })
      .toEqual([{ node: head }]);
  });

  /**
   * The restored head is read once at boot, and an agent edit can land
   * before that read comes back. The edit's head is the newer one, so the
   * read must only fill a head that is still unknown — otherwise the app
   * quietly goes back to the state before the edit and previews that.
   */
  test("keeps an edit's head when it lands before the restored one arrives", async ({ app }) => {
    const restored = nodeId(1);
    const edited = nodeId(2);
    const tracks = [toneTrack()];
    await app.boot({
      ...projectWith(tracks, restored),
      get_session_head: deferred("restored head"),
      render_preview: ok(fixturePath("tone3s")),
    });
    const page = app.page;

    await app.emit("agent://node-created", { node_id: edited });
    await app.release("restored head", restored);
    await expect(page.getByTestId("ruler")).toContainText("0:03");

    await page.getByTestId("render-preview-button").click();

    await expect
      .poll(() => app.requestsFor("render_preview"), { message: "a render of the newer head" })
      .toEqual([{ node: edited }]);
  });
});

/**
 * #332, the same bug by another door. "Open project…", "New project…"
 * and a row in the recents list all go through one handler, which lists
 * the opened project's tracks and never turns them into a timeline.
 */
test.describe("opening a project that already has a track", () => {
  test("from the recents list, shows its timeline", async ({ app }) => {
    const project = "/home/user/Music/interview";
    await app.boot({
      ...readyToLoad(),
      list_recent_projects: ok([
        { path: project, name: "interview", last_opened_at: "2026-09-20T10:00:00Z" },
      ]),
    });
    const page = app.page;
    await expect(page.getByTestId("empty-state")).toBeVisible();

    // What the backend answers once it has that folder open:
    // `open_project` returns `ProjectInfo { path, head }`, and a project
    // that never saved a view reads `ViewState::default()`, which
    // serialises as `{}` because every field is skipped when `None`.
    await app.become({
      open_project: ok({ path: project, head: nodeId(3) }),
      get_view_state: ok({}),
      ...sessionWith([toneTrack()]),
    });
    await page.getByTestId(`recent-open-${project}`).click();

    await expect(page.getByTestId("empty-state")).toHaveCount(0);
    await expect(page.getByTestId("ruler")).toContainText("0:03");
    await expect.poll(() => waveformHasInk(page), { message: "the waveform was drawn" }).toBe(true);
  });
});

/**
 * Found in review of the first #332 fix, which derived the timeline in a
 * helper that three call sites remembered to use. Every test here is a
 * path that did not, or a track list the helper read wrongly.
 */
test.describe("whatever brings a session's audio in, the timeline follows", () => {
  test("recording from a fresh start shows the take", async ({ app }) => {
    const take = fixturePath("tone3s");
    await app.boot({
      ...readyToLoad(),
      start_recording: ok("recording started"),
      // `stop_recording` answers `{ path, sample_rate, channels }`.
      stop_recording: ok({ path: take, sample_rate: 44_100, channels: 1 }),
      // `batch_load` answers `BatchLoadResult`.
      batch_load: ok({ tracks_loaded: 1, last_node_id: nodeId(5) }),
    });
    const page = app.page;
    await expect(page.getByTestId("empty-state")).toBeVisible();

    await page.getByTestId("record-btn").click();
    // Once the take is loaded, the backend lists it as a track.
    await app.become(sessionWith([trackFor(take, fixtureSeconds("tone3s"))]));
    await page.getByTestId("record-btn").click();

    await expect(page.getByTestId("empty-state")).toHaveCount(0);
    await expect(page.getByTestId("ruler")).toContainText("0:03");
    await expect.poll(() => waveformHasInk(page), { message: "the waveform was drawn" }).toBe(true);
  });

  test("a first track with no audio does not hide a second that has some", async ({ app }) => {
    await app.boot(
      projectWith(
        [
          emptyTrack("Host", "0b9f6c2a-1d3e-4f5a-8b7c-9d0e1f2a3b4c"),
          trackFor(fixturePath("tone3s"), fixtureSeconds("tone3s"), {
            name: "Guest",
            id: "5c6d7e8f-9a0b-4c1d-8e2f-3a4b5c6d7e8f",
          }),
        ],
        nodeId(4),
      ),
    );
    const page = app.page;

    await expect(page.getByTestId("empty-state")).toHaveCount(0);
    await expect(page.getByTestId("ruler")).toContainText("0:03");
  });

  test("opening a project with no audio does not keep the last project's file", async ({ app }) => {
    // A file opened in one project must not stay on screen over the next.
    // Opening loads it into that project's session (#321), so it is a
    // track like any other and the timeline follows the tracks.
    const opened = fixturePath("tone3s");
    await app.boot({
      ...readyToLoad(),
      // "Open Audio…" asks for several files, so the picker answers a list.
      "plugin:dialog|open": ok([opened]),
      // `batch_load` answers `BatchLoadResult`.
      batch_load: ok({ tracks_loaded: 1, last_node_id: nodeId(5) }),
    });
    const page = app.page;
    await app.become(sessionWith([toneTrack()]));
    await page.getByTestId("empty-state-open-button").click();
    await expect(page.getByTestId("ruler")).toContainText("0:03");

    const other = "/home/user/Music/untitled";
    await app.become({
      // The folder picker, answered with the chosen folder.
      "plugin:dialog|open": ok(other),
      open_project: ok({ path: other, head: nodeId(9) }),
      ...sessionWith([emptyTrack("Track 1", "7a8b9c0d-1e2f-4a3b-8c4d-5e6f7a8b9c0d")]),
    });
    await page.getByTestId("open-project-button").click();

    await expect(page.getByTestId("empty-state")).toBeVisible();
  });
});

/**
 * Also from review. The first fix gave a returning user a head, and a
 * head is what lets the app save the view — 500 ms after boot, with
 * nothing yet restored, so it wrote zoom 0, no selection and playhead 0
 * over the view they left. On `main` that file survived until the first
 * edit; with the head fix it did not survive the launch.
 */
test.describe("a returning user's view", () => {
  test("is not written before it has been read", async ({ app }) => {
    // The head arrives at boot and would arm a save 500 ms later. If
    // reading the saved view takes longer than that, the save must
    // wait for it rather than write the defaults.
    const head = nodeId(8);
    const saved = { head, zoom_px_per_sec: 200, selection: [0.5, 1.5], playhead_sec: 1 };
    await app.boot({
      ...projectWith([toneTrack()], head),
      get_view_state: deferred("saved view"),
    });

    await app.settle();
    expect(await app.requestsFor("save_view_state"), "a save before the view was read").toEqual([]);

    await app.release("saved view", saved);
    await expect(app.page.getByTestId("timeline-selection-overlay")).toBeVisible();
  });

  test("is restored, not written over with defaults", async ({ app }) => {
    const head = nodeId(8);
    await app.boot({
      ...projectWith([toneTrack()], head),
      get_view_state: ok({ head, zoom_px_per_sec: 200, selection: [0.5, 1.5], playhead_sec: 1 }),
    });
    const page = app.page;

    await expect(page.getByTestId("timeline-selection-overlay")).toBeVisible();
    await expect
      .poll(async () => (await app.requestsFor("save_view_state")).length, {
        message: "the view was saved at least once",
      })
      .toBeGreaterThan(0);
    // Including the playhead. It lives in the mix player, which holds
    // nothing until a preview is rendered and so answers 0; writing that
    // back would move the user to the start of the file.
    for (const saved of await app.requestsFor("save_view_state")) {
      expect(saved, "every save carries the view that was restored").toMatchObject({
        view: { head, zoom_px_per_sec: 200, selection: [0.5, 1.5], playhead_sec: 1 },
      });
    }
  });

  /**
   * From the second review. `view.json` is written 500 ms after the view
   * stops changing, so an edit made in the last half-second before quitting
   * leaves it naming the head *before* that edit. The backend restores the
   * real `HEAD` at launch; if boot then obeyed the saved view's head, it
   * moved `HEAD` back on disk and the edit was gone from the timeline. An
   * edit landing during boot, before the view was read, did the same.
   */
  test("never moves the head the backend restored back to the one it names", async ({ app }) => {
    const before = nodeId(1);
    const latest = nodeId(2);
    await app.boot({
      ...projectWith([toneTrack()], latest),
      get_view_state: ok({ head: before, zoom_px_per_sec: 200 }),
      // What the real command would do if asked: move there and say so.
      set_head_to: ok(before),
      render_preview: ok(fixturePath("tone3s")),
    });
    const page = app.page;
    await expect(page.getByTestId("ruler")).toContainText("0:03");
    await app.settle();

    expect(await app.requestsFor("set_head_to"), "boot moved the head").toEqual([]);
    await page.getByTestId("render-preview-button").click();
    await expect
      .poll(() => app.requestsFor("render_preview"), { message: "a render of the head the backend restored" })
      .toEqual([{ node: latest }]);
  });

  /**
   * The other side of the rule below. Opening a project switches the whole
   * view, so what the last project had on screen must not survive just
   * because it differs from the defaults.
   */
  test("of another project replaces the one on screen when that project is opened", async ({ app }) => {
    const other = "/home/user/Music/interview";
    await app.boot({
      ...projectWith([toneTrack()], nodeId(8)),
      get_view_state: ok({ selection: [0.5, 1.5] }),
    });
    const page = app.page;
    await expect(page.getByTestId("timeline-selection-overlay")).toBeVisible();

    await app.become({
      // The folder picker, answered with the chosen folder.
      "plugin:dialog|open": ok(other),
      open_project: ok({ path: other, head: nodeId(9) }),
      get_view_state: ok({ selection: [2, 2.5] }),
    });
    await page.getByTestId("open-project-button").click();

    await expect
      .poll(async () => (await app.requestsFor("save_view_state")).at(-1), {
        message: "the view saved after opening",
      })
      .toMatchObject({ view: { head: nodeId(9), selection: [2, 2.5] } });
  });

  /**
   * Also from the second review. The saved view is read while the timeline
   * is already usable, so a zoom made in that gap was replaced by the one
   * on disk when the read came back — the user's own action, undone.
   */
  test("does not undo a zoom made before it was read", async ({ app }) => {
    const head = nodeId(8);
    await app.boot({
      ...projectWith([toneTrack()], head),
      get_view_state: deferred("saved view"),
    });
    const page = app.page;
    await expect(page.getByTestId("ruler")).toContainText("0:03");

    // `=` zooms in 40 px/s from auto-fit.
    await page.keyboard.press("=");
    await app.release("saved view", { head, zoom_px_per_sec: 200, selection: [0.5, 1.5] });

    // The selection was never touched, so it is still restored.
    await expect(page.getByTestId("timeline-selection-overlay")).toBeVisible();
    await expect
      .poll(async () => (await app.requestsFor("save_view_state")).at(-1), {
        message: "the view saved after the read",
      })
      .toMatchObject({ view: { zoom_px_per_sec: 40, selection: [0.5, 1.5] } });
  });
});
