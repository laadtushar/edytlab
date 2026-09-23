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
  firstRun,
  nodeId,
  ok,
  projectWith,
  readyToLoad,
  sessionWith,
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
    await app.become(sessionWith([trackFor(fixturePath("tone3s"), fixtureSeconds("tone3s"))]));
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
      projectWith([trackFor(fixturePath("tone3s"), fixtureSeconds("tone3s"))], nodeId(1)),
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
      ...projectWith([trackFor(fixturePath("tone3s"), fixtureSeconds("tone3s"))], head),
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
    const tracks = [trackFor(fixturePath("tone3s"), fixtureSeconds("tone3s"))];
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
      ...sessionWith([trackFor(fixturePath("tone3s"), fixtureSeconds("tone3s"))]),
    });
    await page.getByTestId(`recent-open-${project}`).click();

    await expect(page.getByTestId("empty-state")).toHaveCount(0);
    await expect(page.getByTestId("ruler")).toContainText("0:03");
    await expect.poll(() => waveformHasInk(page), { message: "the waveform was drawn" }).toBe(true);
  });
});
