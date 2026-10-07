// The assistant, end to end: a person types, a model decides, real tools
// edit the session, and the timeline, graph and files follow. Run with
// LLM=real against a local model on :11434 (see HANDOFF.md).
import { assert, K, head, onboard, openAudio, renderPreview, sleep, waitForNewHead } from "./helpers.mjs";

/** Type into the chat box and send it, as a person does. */
async function say(ctx, text) {
  const { d } = ctx;
  await d.type("[data-testid='chat-form'] textarea", text);
  await sleep(300);
  await d.keys(K.enter);
}

/** Wait for the assistant to finish: no thinking indicator, a reply shown. */
async function waitForReply(ctx, { timeout = 480000 } = {}) {
  const { d } = ctx;
  await d.until(async () => {
    const busy = await d.count("[data-testid='thinking-indicator']");
    const replies = await d.count("[data-testid='message-bubble'][data-role='assistant']");
    const error = await d.count("[data-testid='chat-error']");
    if (error) throw new Error(`the chat showed an error: ${await d.text("[data-testid='chat-error']")}`);
    return !busy && replies >= 1;
  }, { timeout, label: "the assistant to finish" });
  await sleep(600);
}

async function toolBadges(ctx) {
  return ctx.d.exec(() =>
    [...document.querySelectorAll("[data-testid='tool-badge']")].map((e) => ({
      status: e.getAttribute("data-status"),
      text: e.textContent.trim().slice(0, 80),
    })),
  );
}

export default [
  {
    id: "5-agent-gain",
    area: "Assistant",
    title: "I ask for a louder track in plain words; a tool runs, the head moves, and undo takes it back",
    async run(ctx) {
      const { d } = ctx;
      await onboard(ctx);
      await openAudio(ctx, ctx.fixtures.music);
      const before = await head(ctx);
      await ctx.shot("The stereo take loaded, before asking for anything");

      await say(ctx, "Make track 0 louder by 6 dB.");
      await sleep(1500);
      await ctx.shot("Sent: the assistant is thinking");
      await waitForReply(ctx);
      await ctx.shot("The assistant's reply, with the tool it ran");

      const badges = await toolBadges(ctx);
      assert(badges.length >= 1, `a tool badge appeared (${JSON.stringify(badges)})`);
      assert(badges.every((b) => b.status === "ok"), `every tool call succeeded: ${JSON.stringify(badges)}`);
      const after = await waitForNewHead(ctx, before);
      const tracks = await d.invoke("list_tracks");
      assert(Math.abs(tracks[0].gain_db - 6) < 0.01, `track 0 gain is +6 dB (got ${tracks[0].gain_db})`);
      await ctx.shot(`The head moved ${before} to ${after}; the track's gain reads +6 dB`);

      await d.click("[data-testid='timeline-root']").catch(() => {});
      await d.keys(K.ctrl, "z");
      await d.until(async () => (await head(ctx)) === before, { label: "undo to the previous head" });
      const undone = await d.invoke("list_tracks");
      assert(Math.abs(undone[0].gain_db) < 0.01, `undo restored 0 dB (got ${undone[0].gain_db})`);
      await ctx.shot("Ctrl+Z: the head and the gain are back");
    },
  },
  {
    id: "5-agent-question",
    area: "Assistant",
    title: "A question gets an answer in words and changes nothing",
    async run(ctx) {
      const { d } = ctx;
      await onboard(ctx);
      await openAudio(ctx, ctx.fixtures.tone);
      const before = await head(ctx);
      await say(ctx, "In one sentence, what can you do for me?");
      await waitForReply(ctx);
      await ctx.shot("A plain-text answer");
      const text = await d.text("[data-testid='message-bubble'][data-role='assistant']");
      assert(text.trim().length > 10, `a real answer (${JSON.stringify(text.slice(0, 80))})`);
      assert((await head(ctx)) === before, "no edit was made");
    },
  },
];
