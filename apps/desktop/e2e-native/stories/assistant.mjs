// The assistant's surroundings, as a person uses them: a selection that
// scopes an edit, slash commands, and the capabilities menu that decides
// which tools the model is offered. Scripted through the fake model, so
// what is asserted is what the app sent it.
import { assert, head, K, onboard, openAudio, sleep, waitForNewHead } from "./helpers.mjs";
import { say, toolBadges, waitForReply } from "./agent.mjs";

/** The `role: "user"` messages of the last streaming request. */
async function lastUserText(ctx) {
  const streaming = (await ctx.llmRequests()).filter((r) => r.stream);
  const last = streaming[streaming.length - 1];
  const users = (last?.messages ?? []).filter((m) => m.role === "user");
  const content = users[users.length - 1]?.content;
  return typeof content === "string" ? content : JSON.stringify(content);
}

async function toolNamesOffered(ctx) {
  const streaming = (await ctx.llmRequests()).filter((r) => r.stream);
  const last = streaming[streaming.length - 1];
  return (last?.tools ?? []).map((t) => t.function?.name ?? t.name);
}

export default [
  {
    id: "5-selection-edit",
    area: "Assistant",
    title: "With a region selected, 'fade this out' is sent with the region and edits the head",
    async run(ctx) {
      const { d } = ctx;
      await onboard(ctx);
      await openAudio(ctx, ctx.fixtures.music);
      await sleep(500);
      const r = await d.rect("[data-testid='timeline-lane-surface']");
      await d.drag("[data-testid='timeline-lane-surface']", -r.width / 4, 0, r.width / 4, 0);
      await d.waitFor("[data-testid='status-bar-selection']");
      const sel = await d.text("[data-testid='status-bar-selection']");
      await ctx.shot(`A region selected: ${sel}`);

      await ctx.script({
        stream: [
          { tool_calls: [{ name: "fade", arguments: { track: 0, kind: "out" } }] },
          { text: "Faded the selection out." },
        ],
        oneShot: [{ text: "edit" }],
      });
      const before = await head(ctx);
      await say(ctx, "Fade this out.");
      await waitForReply(ctx, { timeout: 60000 });
      await ctx.shot("The reply, with the fade tool it ran");

      const sent = await lastUserText(ctx);
      assert(/^\[apply to \d+:\d\d(\.\d+)?-\d+:\d\d(\.\d+)?\] /.test(sent), `the message carried the region (${JSON.stringify(sent.slice(0, 60))})`);
      assert(sent.includes("Fade this out."), "and the words as typed");
      const badges = await toolBadges(ctx);
      assert(badges.length === 1 && badges[0].status === "ok", `the fade ran: ${JSON.stringify(badges)}`);
      await waitForNewHead(ctx, before);
    },
  },
  {
    id: "5-slash-commands",
    area: "Assistant",
    title: "Typing / lists commands; Enter puts one in the box, Escape closes the list, and the prompt is sent as shown",
    async run(ctx) {
      const { d } = ctx;
      await onboard(ctx);
      await openAudio(ctx, ctx.fixtures.tone);
      const box = "[data-testid='chat-form'] textarea";
      const value = () => d.exec(() => document.querySelector("[data-testid='chat-form'] textarea").value);
      // The list's heading, not the page text: the composer's own hint
      // ("/ for commands") is always there.
      const listShown = () =>
        d.exec(() => [...document.querySelectorAll("span")].some((e) => e.textContent.trim().toLowerCase() === "commands"));

      await d.type(box, "/");
      await d.until(listShown, { label: "the command list" });
      await ctx.shot("Typing / opens the command list");
      await d.keys(K.escape);
      await d.until(async () => !(await listShown()), { label: "the list to close on Escape" });
      assert((await value()) === "/", "Escape closed the list and kept the text");
      await ctx.shot("Escape closed it");

      await d.type(box, `${K.ctrl}a`);
      await d.type(box, "/fade");
      await d.until(listShown, { label: "a filtered list" });
      await ctx.shot("/fade narrows the list");
      await d.keys(K.enter);
      await d.until(async () => (await value()).length > 5 && !(await value()).startsWith("/"), { label: "the prompt to fill the box" });
      const prompt = await value();
      assert(!(await listShown()), "the list closed");
      await ctx.shot(`Enter filled the box: ${prompt.slice(0, 60)}`);

      await ctx.script({ stream: [{ text: "Noted." }], oneShot: [{ text: "edit" }] });
      await d.keys(K.enter);
      await waitForReply(ctx, { timeout: 60000 });
      const sent = await lastUserText(ctx);
      assert(sent.includes(prompt), `the prompt reached the model as shown (${JSON.stringify(sent.slice(0, 80))})`);
    },
  },
  {
    id: "5-capabilities",
    area: "Assistant",
    title: "Switching a tool off in the capabilities menu stops the model being offered it",
    async run(ctx) {
      const { d } = ctx;
      await onboard(ctx);
      await openAudio(ctx, ctx.fixtures.tone);
      const row = "[data-testid='capability-row-set_track_gain']";

      await d.click("[data-testid='capabilities-toggle']");
      await d.waitFor("[data-testid='capabilities-menu']");
      await d.waitFor(row);
      await ctx.shot("The capabilities menu");
      await d.click("[aria-label='Enable set_track_gain']");
      await d.until(async () => (await d.exec(() => document.querySelector("[aria-label='Enable set_track_gain']").checked)) === false, {
        label: "set_track_gain to read as off",
      });
      await ctx.shot("set_track_gain switched off");
      await d.click("[data-testid='capabilities-toggle']");

      await ctx.script({ stream: [{ text: "Hi." }], oneShot: [{ text: "edit" }] });
      await say(ctx, "Hello.");
      await waitForReply(ctx, { timeout: 60000 });
      const off = await toolNamesOffered(ctx);
      assert(off.length > 10, `the model was offered tools (${off.length})`);
      assert(!off.includes("set_track_gain"), "set_track_gain was withheld");
      assert(off.includes("fade"), "the rest were still offered");

      await d.click("[data-testid='capabilities-toggle']");
      await d.waitFor(row);
      await d.click("[aria-label='Enable set_track_gain']");
      await d.click("[data-testid='capabilities-toggle']");
      await ctx.script({ stream: [{ text: "Hi again." }], oneShot: [{ text: "edit" }] });
      await say(ctx, "Hello again.");
      await d.until(async () => (await toolNamesOffered(ctx)).includes("set_track_gain"), { label: "the tool to be offered again", timeout: 60000 });
      await ctx.shot("Switched back on: offered again");
    },
  },
];
