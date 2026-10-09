// Undo and redo, driven the way a person does it: through the keys, after
// edits made on the lanes. Each step is checked on screen and against the
// session itself (`list_tracks`, the status-bar head).
import { K } from "../webdriver.mjs";
import { assert, onboard, openAudio, sleep, head } from "./helpers.mjs";

const tracks = (ctx) => ctx.d.invoke("list_tracks");
const lanes = (ctx) => ctx.d.count("[data-testid='timeline-lane']");

/** Wait for the timeline to draw `n` lanes: the session answers first. */
const lanesAre = (ctx, n) =>
  ctx.d.until(async () => (await lanes(ctx)) === n, { label: `${n} lane(s) drawn` });

async function twoTracks(ctx) {
  await onboard(ctx);
  await openAudio(ctx, ctx.fixtures.tone, ctx.fixtures.take2);
  assert((await tracks(ctx)).length === 2, "two tracks in the session");
}

export default [
  {
    id: "6-undo-mute-solo",
    area: "Undo and redo",
    title: "Undo takes back a solo and then a mute, one step each",
    async run(ctx) {
      const { d } = ctx;
      await twoTracks(ctx);
      await ctx.shot("Two tracks loaded");
      const before = await head(ctx);

      await d.click("[data-testid='timeline-lane-mute']");
      await d.until(async () => (await tracks(ctx))[0].muted === true, { label: "track 0 muted in the session" });
      assert((await d.attr("[data-testid='timeline-lane-mute']", "aria-pressed")) === "true", "mute button shows pressed");
      assert((await head(ctx)) !== before, "muting moved the head (it is a session edit)");
      await ctx.shot("Track 1 muted");

      await d.click("[data-testid='timeline-lane-solo']");
      await d.until(async () => (await tracks(ctx))[0].soloed === true, { label: "track 0 soloed in the session" });
      await ctx.shot("Track 1 muted and soloed");

      // Undo takes back the solo, then the mute: one node each.
      await d.keys(K.ctrl, "z");
      await d.until(async () => (await tracks(ctx))[0].soloed === false, { label: "undo of the solo" });
      assert((await tracks(ctx))[0].muted === true, "undoing the solo leaves the mute");
      await d.keys(K.ctrl, "z");
      await d.until(async () => (await tracks(ctx))[0].muted === false, { label: "undo of the mute" });
      assert((await head(ctx)) === before, "two undos return to the head before both edits");
      assert((await d.attr("[data-testid='timeline-lane-mute']", "aria-pressed")) === "false", "mute button shows not pressed again");
      await ctx.shot("Both undone");
    },
  },
  {
    id: "6-undo-after-remove",
    area: "Undo and redo",
    title: "Undo brings back a track that was just removed (#398)",
    async run(ctx) {
      const { d } = ctx;
      await twoTracks(ctx);
      await d.click("[data-testid='track-menu-btn-1']");
      await d.click("[data-testid='track-duplicate-1']");
      await d.until(async () => (await tracks(ctx)).length === 3, { label: "a third track" });
      await d.click("[data-testid='track-menu-btn-2']");
      await d.click("[data-testid='track-remove-2']");
      await d.until(async () => (await tracks(ctx)).length === 2, { label: "back to two tracks" });
      await lanesAre(ctx, 2);
      await ctx.shot("The copy removed");

      await d.keys(K.ctrl, "z");
      // Today the head after the removal is the very node that existed
      // before the duplicate (same state, same id), so undo goes to *its*
      // parent and the copy cannot come back.
      await d.until(async () => (await tracks(ctx)).length === 3, { label: "undo restores the removed track (#398)" });
      await lanesAre(ctx, 3);
      await ctx.shot("Undo: the copy is back");
      await d.keys(K.ctrl, K.shift, "z");
      await d.until(async () => (await tracks(ctx)).length === 2, { label: "redo removes it again" });
      await ctx.shot("Redo: removed again");
    },
  },
];
