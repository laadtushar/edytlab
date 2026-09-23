/**
 * The app comes up in the states a user actually starts from.
 *
 * Asserted by what is on screen, found by booting the harness and
 * reading its page snapshot — not by what the components were expected
 * to show. The first draft of this file looked for the timeline's drop
 * hint, which a first launch never draws: it shows the empty-state
 * screen instead, with onboarding over it.
 */

import { fixturePath } from "./audio-fixtures";
import { firstRun, readyToLoad, session, trackFor } from "./backend";
import { expect, test } from "./fixtures";

test.describe("a first launch, with no project and no key", () => {
  test("asks for a key over the empty state", async ({ app }) => {
    await app.boot(firstRun());
    const page = app.page;

    const onboarding = page.getByRole("dialog", { name: "Settings" });
    await expect(onboarding.getByRole("heading", { name: "Welcome to edytlab" })).toBeVisible();
    // Anthropic, because `AppState::new` starts there.
    await expect(onboarding.getByRole("textbox", { name: "Anthropic API key" })).toHaveValue("");
    // Nothing to save yet.
    await expect(onboarding.getByRole("button", { name: "Save & Continue" })).toBeDisabled();

    // The way in, behind it.
    await expect(page.getByRole("heading", { name: "edytlab", level: 1 })).toBeVisible();
    for (const name of ["New project…", "Open project…", "Start from template", "Open Audio…"]) {
      await expect(page.getByRole("main").getByRole("button", { name, exact: true }).last()).toBeVisible();
    }
  });
});

test.describe("once the agent loads a file", () => {
  test("the lane decodes it in a real browser", async ({ app }) => {
    await app.boot(readyToLoad());
    const page = app.page;
    await expect(page.getByRole("heading", { name: "edytlab", level: 1 })).toBeVisible();

    // The agent's `load` tool wrote a session node; the backend now has
    // a track, and says so with the event it emits for every new node.
    await app.become(session([trackFor(fixturePath("tone3s"))]));
    await app.emit("agent://node-created", { node_id: "n1" });

    // Real WaveSurfer, real decode: the ruler only spans three seconds
    // once the lane has decoded the file and reported its duration.
    await expect(page.getByTestId("ruler")).toContainText("0:03");
    await expect(page.getByTestId("timeline-lane-error")).toHaveCount(0);
  });
});
