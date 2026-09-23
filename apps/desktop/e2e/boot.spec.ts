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
import { firstRun, nodeId, readyToLoad, sessionWith, trackFor } from "./backend";
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
