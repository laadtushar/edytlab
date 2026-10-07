// The timeline and its track controls, as a person uses them. Every
// outcome is checked twice: on screen, and against what the backend
// reports (`list_tracks`), so a control that draws but does nothing fails.
import { assert, head, K, onboard, openAudio, sleep, waitForNewHead, waitForWaveform } from "./helpers.mjs";

const lane = (n = 0) => `[data-testid='timeline-lane']:nth-of-type(${n + 1})`;

async function tracks(ctx) {
  return ctx.d.invoke("list_tracks");
}

export default [
  {
    id: "4-track-controls",
    area: "Timeline",
    title: "Mute, solo, gain and pan each change the track, move the head, and the backend agrees",
    async run(ctx) {
      const { d } = ctx;
      await onboard(ctx);
      await openAudio(ctx, ctx.fixtures.music);
      await ctx.shot("A track with its controls");

      let before = await head(ctx);
      await d.click("[data-testid='timeline-lane-mute']");
      await waitForNewHead(ctx, before);
      assert((await tracks(ctx))[0].muted === true, "the backend says muted");
      assert(/muted/i.test(await d.text("[data-testid='timeline-lane-mute']")), "the button says muted");
      await ctx.shot("Muted: the button says so and the head moved");
      await d.click("[data-testid='timeline-lane-mute']");

      before = await head(ctx);
      await d.click("[data-testid='timeline-lane-solo']");
      await waitForNewHead(ctx, before);
      assert((await tracks(ctx))[0].soloed === true, "the backend says soloed");
      await ctx.shot("Soloed");
      await d.click("[data-testid='timeline-lane-solo']");

      before = await head(ctx);
      for (let i = 0; i < 4; i++) await d.type("[data-testid='timeline-lane-gain']", K.right);
      await waitForNewHead(ctx, before);
      // Each nudge is its own edit, committed one after another: wait for
      // the last, not the first.
      await d.until(async () => Math.abs((await tracks(ctx))[0].gain_db - 2) < 0.01, {
        label: "gain to reach +2 dB after four 0.5 dB steps",
      });
      await d.until(async () => /\+2\.0/.test(await d.text("[data-testid='timeline-lane-gain-readout']")), {
        label: "the readout to say +2.0",
      });
      await ctx.shot("Gain nudged up four steps: +2.0 dB");

      before = await head(ctx);
      for (let i = 0; i < 5; i++) await d.type("[data-testid='timeline-lane-pan']", K.left);
      await waitForNewHead(ctx, before);
      const pan = (await tracks(ctx))[0].pan;
      assert(pan < -0.01, `panned left (got ${pan})`);
      await ctx.shot(`Panned left (${pan.toFixed(2)})`);

      before = await head(ctx);
      await d.dblclick("[data-testid='timeline-lane-pan']");
      await waitForNewHead(ctx, before);
      assert(Math.abs((await tracks(ctx))[0].pan) < 0.01, "double-click re-centred the pan");
      await ctx.shot("Double-click on the pan slider: centred again");
    },
  },
  {
    id: "4-track-menu",
    area: "Timeline",
    title: "Rename, duplicate and remove a track from its menu",
    async run(ctx) {
      const { d } = ctx;
      await onboard(ctx);
      await openAudio(ctx, ctx.fixtures.music);

      await d.click("[data-testid='track-menu-btn-0']");
      await sleep(300);
      await ctx.shot("The track's menu");
      await d.click("[data-testid='track-rename-0']");
      await d.type("[data-testid='track-rename-input-0']", `${K.ctrl}a`);
      await d.type("[data-testid='track-rename-input-0']", "Lead guitar");
      await d.keys(K.enter);
      await d.until(async () => (await tracks(ctx))[0].name === "Lead guitar", { label: "the rename" });
      await d.until(async () => /lead guitar/i.test(await d.text("[data-testid='timeline-lane-name']")), {
        label: "the lane to show the new name",
      });
      await ctx.shot("Renamed to Lead guitar");
      // A rename must not cost the track its waveform.
      await waitForWaveform(ctx).catch(async () => {
        await ctx.shot("BUG: the waveform did not come back after the rename");
        throw new Error("the waveform did not come back after renaming the track");
      });

      await d.click("[data-testid='track-menu-btn-0']");
      await d.click("[data-testid='track-duplicate-0']");
      await d.until(async () => (await d.count("[data-testid='timeline-lane']")) === 2, { label: "two lanes" });
      assert((await tracks(ctx)).length === 2, "the backend has two tracks");
      await ctx.shot("Duplicated: two tracks");

      await d.click("[data-testid='track-menu-btn-1']");
      await d.click("[data-testid='track-remove-1']");
      await d.until(async () => (await d.count("[data-testid='timeline-lane']")) === 1, { label: "back to one lane" });
      assert((await tracks(ctx)).length === 1, "the backend has one track again");
      await ctx.shot("The copy removed");
    },
  },
  {
    id: "4-zoom-select",
    area: "Timeline",
    title: "Zoom in, drag a selection, snap it to zero crossings, and clear it",
    async run(ctx) {
      const { d } = ctx;
      await onboard(ctx);
      await openAudio(ctx, ctx.fixtures.music);
      await sleep(500);

      const ticks = () => d.exec(() => document.querySelector("[data-testid='ruler']").textContent.replace(/\s+/g, " ").trim());
      const fit = await ticks();
      await d.click("[data-testid='zoom-in-btn']");
      await d.click("[data-testid='zoom-in-btn']");
      await sleep(800);
      const zoomed = await ticks();
      assert(zoomed !== fit, `the ruler changed on zoom (${fit} vs ${zoomed})`);
      await ctx.shot("Zoomed in twice: a finer ruler and a scrollbar");

      await d.click("[data-testid='fit-to-window-btn']");
      await sleep(600);
      assert((await ticks()) === fit, "fit-to-window restored the ruler");
      await ctx.shot("Fit to window");

      const r = await d.rect("[data-testid='timeline-lane-surface']");
      await d.drag("[data-testid='timeline-lane-surface']", -r.width / 4, 0, r.width / 4, 0);
      await d.waitFor("[data-testid='timeline-selection-overlay']");
      await d.waitFor("[data-testid='status-bar-selection']");
      const sel = await d.text("[data-testid='status-bar-selection']");
      assert(/sel/i.test(sel), `the status bar shows the selection (${sel})`);
      await ctx.shot(`A selection dragged across the middle: ${sel}`);

      await d.click("[data-testid='snap-zero-btn']");
      assert((await d.attr("[data-testid='snap-zero-btn']", "aria-pressed")) === "true", "snap is on");
      await ctx.shot("Snap to zero crossings on");

      await d.keys(K.escape);
      await d.until(async () => (await d.count("[data-testid='timeline-selection-overlay']")) === 0, { label: "the selection to clear" });
      await ctx.shot("Escape cleared the selection");
    },
  },
];
