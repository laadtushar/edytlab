// Track controls: what a person does to a mix once a few files are loaded.
// Each step is checked twice: on screen, and against the session itself
// (`list_tracks`), so a control that only looks like it worked fails.
import { K } from "../webdriver.mjs";
import { assert, onboard, openAudio, sleep, head } from "./helpers.mjs";

const tracks = (ctx) => ctx.d.invoke("list_tracks");
const lanes = (ctx) => ctx.d.count("[data-testid='timeline-lane']");

/** Press `key` `n` times on whatever has focus. */
async function press(ctx, key, n) {
  // Paced like a person typing: faster than ~60 ms between presses the
  // slider drops steps (a refresh resets it mid-run, see the issue).
  for (let i = 0; i < n; i++) {
    await ctx.d.keys(key);
    await sleep(250);
  }
}

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
    id: "4-mute-solo",
    area: "Track controls",
    title: "Mute and solo a track from its lane, and undo it",
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
    id: "4-gain-pan",
    area: "Track controls",
    title: "Gain and pan sliders write the session, one undoable step each",
    async run(ctx) {
      const { d } = ctx;
      await twoTracks(ctx);
      // A click on a range input sets it where the pointer lands, so read
      // the session's value afterwards instead of assuming it starts at 0.
      await d.click("[data-testid='timeline-lane-gain']");
      await d.until(async () => (await tracks(ctx))[0].gain_db !== 0, { label: "the click set a gain" });
      await sleep(600); // the click's own refresh lands before the first key press, as it does for a person
      const g0 = (await tracks(ctx))[0].gain_db;
      await press(ctx, K.right, 2); // +0.5 dB a key press
      await d.until(async () => Math.abs((await tracks(ctx))[0].gain_db - (g0 + 1)) < 0.01, { label: `gain ${g0} + 1 dB in the session` });
      const want = (g0 + 1).toFixed(1);
      const readout = await d.text("[data-testid='timeline-lane-gain-readout']");
      assert(readout.replace(/\s+/g, "").toLowerCase().includes(`${want}db`), `gain readout says ${want} dB: ${readout}`);
      await ctx.shot(`Gain at ${want} dB`);

      // A click on the centre of the pan slider leaves it at exactly 0, so
      // move it with the keys (0.02 a press) from there.
      await d.click("[data-testid='timeline-lane-pan']");
      await sleep(600);
      await press(ctx, K.left, 5);
      await d.until(async () => Math.abs((await tracks(ctx))[0].pan + 0.1) < 0.011, { label: "pan -0.1 in the session" });
      await ctx.shot("Pan moved left by the keys");

      // Double-click returns pan to the centre, exactly.
      await d.dblclick("[data-testid='timeline-lane-pan']");
      await d.until(async () => (await tracks(ctx))[0].pan === 0, { label: "pan back to exactly 0" });
      await ctx.shot("Pan double-clicked back to centre");
      assert((await tracks(ctx))[1].gain_db === 0, "the other track is untouched");
    },
  },
  {
    id: "5-track-menu",
    area: "Track controls",
    title: "Rename, duplicate and remove a track from its menu, then undo and redo",
    async run(ctx) {
      const { d } = ctx;
      await twoTracks(ctx);

      await d.click("[data-testid='track-menu-btn-1']");
      await d.click("[data-testid='track-rename-1']");
      await ctx.shot("Renaming the second track");
      // The box starts with the current name; select it so typing replaces it.
      await d.type("[data-testid='track-rename-input-1']", K.ctrl + "a" + "\uE000");
      await d.type("[data-testid='track-rename-input-1']", "Lead vocal");
      await d.keys(K.enter);
      await d.until(async () => (await tracks(ctx))[1].name === "Lead vocal", { label: "rename in the session" });
      const names = async () => (await d.exec(() => [...document.querySelectorAll("[data-testid='timeline-lane-name']")].map((e) => e.textContent)));
      assert((await names())[1] === "Lead vocal", `lane shows the new name: ${await names()}`);
      await ctx.shot("Renamed");

      await d.click("[data-testid='track-menu-btn-1']");
      await d.click("[data-testid='track-duplicate-1']");
      await d.until(async () => (await tracks(ctx)).length === 3, { label: "a third track" });
      await lanesAre(ctx, 3);
      await ctx.shot("Duplicated: three tracks");

      await d.click("[data-testid='track-menu-btn-2']");
      await d.click("[data-testid='track-remove-2']");
      await d.until(async () => (await tracks(ctx)).length === 2, { label: "back to two tracks" });
      await lanesAre(ctx, 2);
      await ctx.shot("The copy removed");
    },
  },
  {
    id: "5-undo-after-remove",
    area: "Track controls",
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
