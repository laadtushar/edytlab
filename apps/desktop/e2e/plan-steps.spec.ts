/**
 * The plan card says what each step will do, before Run (#481, #482).
 *
 * A real-Claude run asked for "a marker called Drop at 16 seconds and a
 * region called Outro from 24 s to 32 s". The app showed a plan to
 * approve, and each step read `add_marker —` or a bare `—`: the user
 * pressed Run without seeing the time, the name or the range. Another
 * model's plan for "split into stems" offered a tool whose inference is
 * not shipped, as if it would work.
 *
 * What only a browser can see here is the drawn card: the words a user
 * reads, the Run button they would press, and that a step which can only
 * fail looks different. jsdom checks the same text (`planCard.test.tsx`)
 * without layout or a real click.
 *
 * `agent://plan` is raised with the steps the backend sends: the model's
 * own, minus the steps `fetch_plan` drops, plus `"unavailable": true` on a
 * step whose tool does nothing in this build.
 */

import { fixturePath } from "./audio-fixtures";
import { deferred, nodeId, ok, projectWith, toneTrack } from "./backend";
import { expect, test, type App } from "./fixtures";

const HEAD = nodeId(2);

/** The turn up to the point the card appears: the user asks, a plan comes. */
async function planArrives(app: App, steps: unknown[]) {
  await app.boot({
    ...projectWith([toneTrack()], HEAD),
    render_preview: ok(fixturePath("tone3s")),
    get_plan_first: ok(true),
    send_message: deferred("send"),
    approve_plan: ok(null),
    reject_plan: ok(null),
  });
  const page = app.page;
  await expect(page.getByTestId("ruler")).toContainText("0:03");

  const box = page.getByTestId("chat-form").locator("textarea");
  await box.fill("Mark the drop at 16 seconds and label the last 8 seconds");
  await box.press("Enter");
  await expect.poll(() => app.requestsFor("send_message")).toHaveLength(1);

  await app.emit("agent://plan", { steps });
  const card = page.getByTestId("plan-approval-card");
  await expect(card).toBeVisible();
  return card;
}

test("a two-step plan shows the marker's time and name, and the region's range, before Run", async ({
  app,
}) => {
  const card = await planArrives(app, [
    { step: 1, tool: "label", description: "Add marker 'Drop' at 16.0 s" },
    {
      step: 2,
      tool: "label",
      description: "Add region 'Outro' from 24.0 s to 32.0 s",
    },
  ]);

  await expect(card).toContainText("2 steps");
  const steps = card.getByTestId("plan-step");
  await expect(steps).toHaveCount(2);
  await expect(steps.nth(0)).toContainText("Add marker");
  await expect(steps.nth(0)).toContainText("'Drop' at 16.0 s");
  await expect(steps.nth(1)).toContainText("Add region");
  await expect(steps.nth(1)).toContainText("'Outro' from 24.0 s to 32.0 s");
  // The tool's registered name is not what the user reads.
  await expect(card).not.toContainText("label");

  await card.getByTestId("plan-run-button").click();
  await expect.poll(() => app.requestsFor("approve_plan")).toEqual([{ steps: null }]);
  await expect(card).toHaveCount(0);

  await app.release("send", null);
});

test("a step that came with no description shows its tool and arguments, never a bare dash", async ({
  app,
}) => {
  const card = await planArrives(app, [
    { step: 1, tool: "label", args: { time: 16, name: "Drop" } },
    { step: 2, tool: "reverse", description: "" },
    { step: 3, description: "Listen back to the result" },
  ]);

  const text = card.getByTestId("plan-step-text");
  await expect(text).toHaveCount(3);
  await expect(text.nth(0)).toHaveText('Add marker — time: 16, name: "Drop"');
  // Nothing to follow the name, so no dash after it.
  await expect(text.nth(1)).toHaveText("Reverse");
  await expect(text.nth(2)).toHaveText("Listen back to the result");

  // The user can still read what they approve.
  await card.getByTestId("plan-run-button").click();
  await expect.poll(() => app.requestsFor("approve_plan")).toEqual([{ steps: null }]);

  await app.release("send", null);
});

test("a step whose tool does nothing in this build is marked, and a plan of only those cannot be run", async ({
  app,
}) => {
  const card = await planArrives(app, [
    {
      step: 1,
      tool: "separate_stems",
      description: "Split track 0 into stems",
      unavailable: true,
    },
  ]);

  const step = card.getByTestId("plan-step");
  await expect(step).toHaveAttribute("data-unavailable", "true");
  await expect(step).toContainText("Separate stems");
  await expect(step).toContainText("Split track 0 into stems");
  await expect(step.getByTestId("plan-step-unavailable")).toHaveText(
    "not available in this build",
  );
  // Nothing to edit in a step that can only fail.
  await expect(step.getByTestId("plan-edit-button")).toHaveCount(0);

  await expect(card.getByTestId("plan-run-button")).toBeDisabled();
  await expect(card.getByTestId("plan-nothing-runs")).toBeVisible();

  // Discard is how the user leaves it.
  await card.getByTestId("plan-discard-button").click();
  await expect.poll(() => app.requestsFor("reject_plan")).toHaveLength(1);
  expect(await app.requestsFor("approve_plan")).toEqual([]);

  await app.release("send", null);
});

test("Run stays available when only some steps cannot run", async ({ app }) => {
  const card = await planArrives(app, [
    {
      step: 1,
      tool: "separate_stems",
      description: "Split track 0",
      unavailable: true,
    },
    { step: 2, tool: "analyze_track", description: "Check track 0" },
  ]);

  const steps = card.getByTestId("plan-step");
  await expect(steps.nth(0)).toHaveAttribute("data-unavailable", "true");
  await expect(steps.nth(1)).not.toHaveAttribute("data-unavailable", "true");
  await expect(card.getByTestId("plan-run-button")).toBeEnabled();
  await expect(card.getByTestId("plan-nothing-runs")).toHaveCount(0);

  await card.getByTestId("plan-discard-button").click();
  await expect.poll(() => app.requestsFor("reject_plan")).toHaveLength(1);
  await app.release("send", null);
});
