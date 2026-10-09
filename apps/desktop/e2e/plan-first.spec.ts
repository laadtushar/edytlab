/**
 * With Plan first on, the first edit is held and shown (#415).
 *
 * When the model writes no plan, the agent's first step that would
 * change the session used to run unapproved. Now it is held before it
 * runs and shown on the plan card as the concrete call: the tool and the
 * arguments it will run with.
 *
 * What only a real browser can see is the part that is not the card
 * itself. By the time the card appears, the model's sentence has already
 * streamed and its tool badge is already on screen. Declining ends the
 * turn with no `done`, so something has to settle both: without that, a
 * half-streamed bubble keeps its caret and a badge keeps saying it is
 * running, above a card that is gone. jsdom draws neither, so this is
 * checked here and not only in the unit tests.
 *
 * `send_message` is held open for the whole turn, as it is in the real
 * app: the command returns when the turn does. `approve_plan` and
 * `reject_plan` return `CmdResult<()>`, which serialises as `null`.
 */

import { fixturePath } from "./audio-fixtures";
import { deferred, nodeId, ok, projectWith, toneTrack } from "./backend";
import { expect, test, type App } from "./fixtures";

const HEAD = nodeId(2);

const HELD_STEP = [{ step: 1, tool: "reverse", description: "track: 0" }];

/**
 * The turn up to the point the card appears: the user asks, no plan
 * arrives, the model says a sentence and starts a call, and the call is
 * held.
 */
async function holdAnEdit(app: App) {
  await app.boot({
    ...projectWith([toneTrack()], HEAD),
    render_preview: ok(fixturePath("tone3s")),
    // The user has turned Plan first on.
    get_plan_first: ok(true),
    send_message: deferred("send"),
    approve_plan: ok(null),
    reject_plan: ok(null),
  });
  const page = app.page;
  await expect(page.getByTestId("ruler")).toContainText("0:03");

  const box = page.getByTestId("chat-form").locator("textarea");
  await box.fill("Reverse the whole of track 0");
  await box.press("Enter");
  await expect.poll(() => app.requestsFor("send_message")).toHaveLength(1);

  // No plan came back; the first edit will be held instead.
  await app.emit("agent://plan-unavailable", {
    reason: "the model did not return a plan",
    first_edit_held: true,
  });
  const notice = page.getByTestId("chat-notice");
  await expect(notice).toContainText("first edit");
  await expect(notice).toContainText("approv");
  await expect(notice).not.toContainText("continuing without");

  // The model's sentence and its call stream before the card appears.
  await app.emit("agent://text-delta", { text: "Reversing track 0." });
  await app.emit("agent://tool-call", { name: "reverse", id: "t1" });
  await expect(page.getByTestId("tool-badge")).toHaveAttribute("data-status", "running");
  await app.emit("agent://plan", { steps: HELD_STEP });

  const card = page.getByTestId("plan-approval-card");
  await expect(card).toBeVisible();
  await expect(card).toContainText("reverse");
  await expect(card).toContainText("track: 0");
  await expect(card).toContainText("1 step");
  await expect(card).not.toContainText("1 steps");
}

test("a held edit is shown, and Discard runs nothing", async ({ app }) => {
  await holdAnEdit(app);
  const page = app.page;

  await page.getByTestId("plan-discard-button").click();
  await expect.poll(() => app.requestsFor("reject_plan")).toHaveLength(1);
  await expect(page.getByTestId("plan-approval-card")).toHaveCount(0);
  expect(await app.requestsFor("approve_plan")).toEqual([]);

  // What the backend then says: the held call is resolved as not run,
  // and the turn ends as rejected, with no `done`.
  await app.emit("agent://tool-call-end", { id: "t1", ok: false });
  await app.emit("agent://plan-rejected", null);

  // The sentence is a message now, not a bubble still being typed.
  await expect(page.getByTestId("message-bubble").filter({ hasText: "Reversing track 0." })).toBeVisible();
  await expect(page.getByTestId("caret")).toHaveCount(0);
  await expect(page.getByTestId("tool-badge")).not.toHaveAttribute("data-status", "running");

  await app.release("send", null);
});

test("Run on a held edit approves it", async ({ app }) => {
  await holdAnEdit(app);
  const page = app.page;

  await page.getByTestId("plan-run-button").click();
  await expect.poll(() => app.requestsFor("approve_plan")).toEqual([{ steps: null }]);
  await expect(page.getByTestId("plan-approval-card")).toHaveCount(0);

  // The step runs, and the turn finishes as any other.
  await app.emit("agent://tool-call-end", { id: "t1", ok: true });
  await app.emit("agent://done", {});

  await expect(page.getByTestId("tool-badge")).toHaveAttribute("data-status", "ok");
  await expect(page.getByTestId("message-bubble").filter({ hasText: "Reversing track 0." })).toBeVisible();
  await expect(page.getByTestId("caret")).toHaveCount(0);
  expect(await app.requestsFor("reject_plan")).toEqual([]);

  await app.release("send", null);
});
