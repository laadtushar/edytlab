// Plan first: the assistant shows its steps, and a person approves, edits
// or discards them. Scripted through the fake model, which answers every
// non-streaming call in chat-completions shape only, as Ollama and Groq
// do; run in the default (fake) mode.
import { assert, K, head, onboard, openAudio, sleep } from "./helpers.mjs";

const PLAN = [
  { step: 1, tool: "set_track_gain", description: "Raise track 0 by 6 dB" },
  { step: 2, tool: "render_preview", description: "Render a preview" },
];
const planReply = `<plan>${JSON.stringify(PLAN)}</plan>`;

async function say(ctx, text) {
  await ctx.d.type("[data-testid='chat-form'] textarea", text);
  await sleep(300);
  await ctx.d.keys(K.enter);
}

async function turnPlanFirstOn(ctx) {
  await ctx.d.click("[data-testid='plan-first-toggle']");
  await ctx.d.until(async () => (await ctx.d.attr("[data-testid='plan-first-toggle']", "aria-pressed")) === "true", {
    label: "Plan first to be on",
  });
}

async function assistantText(ctx) {
  return ctx.d.exec(() =>
    [...document.querySelectorAll("[data-testid='message-bubble'][data-role='assistant']")].map((e) => e.textContent).join("\n"),
  );
}

export default [
  {
    id: "5-plan-approve-edited",
    area: "Assistant",
    title: "With Plan first on, I see the steps, edit one, run them, and the edit reaches the model",
    async run(ctx) {
      const { d } = ctx;
      await onboard(ctx);
      await openAudio(ctx, ctx.fixtures.music);
      await turnPlanFirstOn(ctx);
      await ctx.script({
        stream: [
          { tool_calls: [{ name: "set_track_gain", arguments: { track: 0, db: 6 } }] },
          { text: "Raised track 0 by 6 dB." },
        ],
        oneShot: [{ text: "edit" }, { text: planReply }],
      });
      const before = await head(ctx);
      await say(ctx, "Make track 0 louder by 6 dB.");

      await d.waitFor("[data-testid='plan-approval-card']", { timeout: 30000 }).catch(async (e) => {
        await ctx.shot("BUG: no plan card appeared");
        throw new Error(`no plan card appeared; the chat says: ${JSON.stringify((await assistantText(ctx)).slice(0, 200))} (${e.message})`);
      });
      assert((await head(ctx)) === before, "nothing ran before approval");
      assert((await d.count("[data-testid='plan-edit-button']")) === 2, "each step can be edited");
      await ctx.shot("The plan, waiting for approval");

      await d.click("[data-testid='plan-edit-button']");
      await d.type("[data-testid='plan-step-editor']", `${K.ctrl}a`);
      await d.type("[data-testid='plan-step-editor']", "Raise track 0 by exactly 6 dB");
      await d.click("[data-testid='plan-step-save']");
      await ctx.shot("Step 1 edited");
      await d.click("[data-testid='plan-run-button']");

      await d.until(async () => (await head(ctx)) !== before, { timeout: 60000, label: "the head to move after Run" });
      const tracks = await d.invoke("list_tracks");
      assert(Math.abs(tracks[0].gain_db - 6) < 0.01, `track 0 is +6 dB (got ${tracks[0].gain_db})`);
      await d.until(async () => (await d.count("[data-testid='plan-approval-card']")) === 0, { label: "the plan card to go away" });
      await ctx.shot("Approved and run");

      const sent = JSON.stringify(await ctx.llmRequests());
      assert(sent.includes("Raise track 0 by exactly 6 dB"), "the edited step reached the model");
    },
  },
  {
    id: "5-plan-discard",
    area: "Assistant",
    title: "Discarding a plan runs nothing and leaves the chat usable",
    async run(ctx) {
      const { d } = ctx;
      await onboard(ctx);
      await openAudio(ctx, ctx.fixtures.music);
      await turnPlanFirstOn(ctx);
      await ctx.script({
        stream: [{ text: "Fine, I am here." }],
        oneShot: [{ text: "edit" }, { text: planReply }],
      });
      const before = await head(ctx);
      await say(ctx, "Make track 0 louder by 6 dB.");
      await d.waitFor("[data-testid='plan-approval-card']", { timeout: 30000 }).catch(async () => {
        await ctx.shot("BUG: no plan card appeared");
        throw new Error(`no plan card appeared; the chat says: ${JSON.stringify((await assistantText(ctx)).slice(0, 200))}`);
      });
      await ctx.shot("The plan, before discarding");
      await d.click("[data-testid='plan-discard-button']");
      await d.until(async () => (await d.count("[data-testid='plan-approval-card']")) === 0, { label: "the card to go away" });
      await sleep(1500);
      assert((await head(ctx)) === before, "discarding ran nothing");
      assert(Math.abs((await d.invoke("list_tracks"))[0].gain_db) < 0.01, "the gain is untouched");
      assert((await d.count("[data-testid='chat-error']")) === 0, "no error is shown");
      await ctx.shot("Discarded: no edit, no error");
    },
  },
  {
    id: "5-plan-request",
    area: "Assistant",
    title: "The plan request carries the instruction to plan and the editor's system prompt",
    async run(ctx) {
      const { d } = ctx;
      await onboard(ctx);
      await openAudio(ctx, ctx.fixtures.tone);
      await turnPlanFirstOn(ctx);
      await ctx.script({ stream: [{ text: "ok" }], oneShot: [{ text: "edit" }, { text: planReply }] });
      await say(ctx, "Fade out the last second.");
      await d.waitFor("[data-testid='plan-approval-card']", { timeout: 30000 }).catch(() => {});
      const requests = (await ctx.llmRequests()).filter((r) => !r.stream);
      ctx.note(`non-streaming requests: ${requests.length}`);
      requests.forEach((r, i) => ctx.note(`request ${i + 1} keys: ${Object.keys(r).join(", ")}; roles: ${(r.messages ?? []).map((m) => m.role).join(",")}`));
      const planCall = requests[requests.length - 1];
      assert(planCall, "a plan request was made");
      const everything = JSON.stringify(planCall.messages ?? []);
      assert(everything.includes("<plan>"), "the plan instruction reached an OpenAI-compatible server (it is sent as a top-level `system` field they ignore)");
    },
  },
];
