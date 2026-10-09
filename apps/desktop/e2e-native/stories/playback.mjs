// Playback as a person drives it from the keyboard: play, pause, seek,
// jump to either end, and loop. The playhead's position is read from the
// DOM, so a transport that draws but does not move fails.
import { assert, K, onboard, openAudio, renderPreview, sleep } from "./helpers.mjs";

export default [
  {
    id: "4-playback-keys",
    area: "Timeline",
    title: "Space plays and pauses, the arrows seek, Home and End jump, and L toggles loop",
    async run(ctx) {
      const { d } = ctx;
      await onboard(ctx);
      await openAudio(ctx, ctx.fixtures.music); // 8 s
      await sleep(800);
      const at = () =>
        d.exec(() => {
          const p = document.querySelector("[data-testid='timeline-playhead']");
          return p ? Number(p.getAttribute("data-playhead-sec")) : null;
        });
      // Focus the page, not a control that would swallow Space.
      await d.click("[data-testid='timeline-lane-surface']");
      // Render the mix first, so the seeking keys below act on a mix that
      // has loaded. (Space on a session with no mix renders one and then
      // plays it, #431; that would leave the playhead already moving here.)
      await renderPreview(ctx);
      await ctx.shot("After Preview: the mix is rendered");
      await d.click("[data-testid='timeline-lane-surface']");
      await d.keys(K.home);
      await d.until(async () => (await at()) === 0, { label: "Home to put the playhead at 0" });
      await ctx.shot("Home: the playhead at 0");

      await d.keys(K.right);
      await d.until(async () => Math.abs((await at()) - 5) < 0.3, { label: "Right to seek +5 s" });
      await ctx.shot(`Right arrow: +5 s (${await at()})`);
      await d.keys(K.left);
      await d.until(async () => Math.abs(await at()) < 0.3, { label: "Left to seek back -5 s" });

      await d.keys(K.end);
      await d.until(async () => Math.abs((await at()) - 8) < 0.3, { label: "End to jump to the end" });
      await ctx.shot("End: the playhead at the end");
      await d.keys(K.home);
      await d.until(async () => (await at()) === 0, { label: "Home" });

      await d.keys(" ");
      await d.until(async () => (await at()) > 0.5, { timeout: 20000, label: "the playhead to advance while playing" });
      await ctx.shot(`Playing: the playhead moved to ${await at()}`);
      await d.keys(" ");
      await sleep(600);
      const paused = await at();
      await sleep(1500);
      assert(Math.abs((await at()) - paused) < 0.05, `Space paused: the playhead held at ${paused}`);
      await ctx.shot(`Paused at ${paused}`);

      const loopOn = () => d.attr("[data-testid='loop-btn']", "class").then((c) => /amber/.test(c));
      assert(!(await loopOn()), "loop starts off");
      await d.keys("l");
      await d.until(loopOn, { label: "L to turn loop on" });
      await ctx.shot("L: loop on");
      await d.keys("l");
      await d.until(async () => !(await loopOn()), { label: "L to turn loop off" });
    },
  },
];
