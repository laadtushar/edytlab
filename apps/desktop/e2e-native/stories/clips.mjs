// Clips as a person arranges them: drag a chip to move it, nudge it with
// the arrow keys, delete it, and take it back with undo. Each outcome is
// read from the backend (`list_tracks`) as well as the screen.
import { assert, head, K, onboard, openAudio, sleep, waitForNewHead } from "./helpers.mjs";

const clipsOf = async (ctx) => (await ctx.d.invoke("list_tracks"))[0].clips ?? [];

export default [
  {
    id: "4-clips-move-remove",
    area: "Timeline",
    title: "Drag a clip, nudge it with the arrows, delete it, and undo brings it back",
    async run(ctx) {
      const { d } = ctx;
      await onboard(ctx);
      await openAudio(ctx, ctx.fixtures.music);
      await d.waitFor("[data-testid='clip-chip-0']");
      const first = await clipsOf(ctx);
      ctx.note(`clips at load: ${JSON.stringify(first)}`);
      assert(first.length === 1, `one clip at load (${first.length})`);
      const start0 = first[0].start_sec;
      await ctx.shot("One clip on the track");

      let before = await head(ctx);
      await d.drag("[data-testid='clip-chip-0']", 0, 0, 120, 0);
      await waitForNewHead(ctx, before);
      const moved = (await clipsOf(ctx))[0].start_sec;
      assert(moved > start0 + 0.3, `dragging right moved the clip later (${start0} to ${moved})`);
      await ctx.shot(`Dragged: the clip now starts at ${moved.toFixed(2)} s`);

      before = await head(ctx);
      await d.click("[data-testid='clip-chip-0']");
      await d.keys(K.left);
      await waitForNewHead(ctx, before);
      const nudged = (await clipsOf(ctx))[0].start_sec;
      assert(nudged < moved - 0.01, `the left arrow nudged it earlier (${moved} to ${nudged})`);
      await ctx.shot(`Left arrow: ${nudged.toFixed(2)} s`);

      before = await head(ctx);
      await d.keys(K.del);
      await waitForNewHead(ctx, before);
      await d.until(async () => (await clipsOf(ctx)).length === 0, { label: "the backend to drop the clip" });
      await d.until(async () => (await d.count("[data-testid='clip-chip-0']")) === 0, { label: "the chip to go" });
      await ctx.shot("Delete: the clip is gone");

      await d.click("[data-testid='timeline-root']").catch(() => {});
      await d.keys(K.ctrl, "z");
      await d.until(async () => (await clipsOf(ctx)).length === 1, { label: "undo to bring the clip back" });
      await d.waitFor("[data-testid='clip-chip-0']");
      await ctx.shot("Ctrl+Z: the clip is back");
    },
  },
];
