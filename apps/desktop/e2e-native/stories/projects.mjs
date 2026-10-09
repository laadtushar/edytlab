import { mkdtempSync, existsSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { assert, onboard, openAudio, sleep, waitForWaveform, head } from "./helpers.mjs";

export default [
  {
    id: "2-open-audio",
    area: "Loading audio",
    title: "Open Audio loads a file as a track, draws it, and starts the project's history",
    async run(ctx) {
      const { d } = ctx;
      await onboard(ctx);
      await ctx.shot("The empty editor, ready for a file");
      await openAudio(ctx, ctx.fixtures.music);
      await sleep(800);
      const h = await head(ctx);
      assert(/^[0-9a-f]{7}$/.test(h), `a head: ${h}`);
      assert(/music-8s-stereo/i.test(await d.text("[data-testid='status-bar-file']")), "file named in the status bar");
      await ctx.shot("The stereo take on the timeline; the status bar names it and shows the first node");
    },
  },
  {
    id: "2-open-several",
    area: "Loading audio",
    title: "Choosing several files loads one track each",
    async run(ctx) {
      const { d } = ctx;
      await onboard(ctx);
      await openAudio(ctx, ctx.fixtures.music, ctx.fixtures.take2, ctx.fixtures.tone);
      await sleep(800);
      assert((await d.count("[data-testid='timeline-lane']")) === 3, "three lanes");
      await ctx.shot("Three files, three tracks");
    },
  },
  {
    id: "2-not-audio",
    area: "Loading audio",
    title: "A file that is not audio is refused, by name, and nothing is loaded",
    async run(ctx) {
      const { d } = ctx;
      await onboard(ctx);
      await ctx.chooseThrough("[data-testid='open-audio-button']", { path: ctx.fixtures.notAudio });
      await d.waitFor("[data-testid='render-error']", { timeout: 20000 });
      const msg = await d.text("[data-testid='render-error']");
      await ctx.shot("The error names the file that could not be loaded");
      assert(/notes\.wav/.test(msg), `error names the file: ${msg}`);
      assert((await d.count("[data-testid='timeline-lane']")) === 0, "no lane");
    },
  },
  {
    id: "2-cancel-dialog",
    area: "Loading audio",
    title: "Cancelling the file dialog changes nothing",
    async run(ctx) {
      const { d } = ctx;
      await onboard(ctx);
      await ctx.chooseThrough("[data-testid='open-audio-button']", { cancel: true });
      await sleep(1500);
      assert((await d.count("[data-testid='render-error']")) === 0, "no error");
      assert((await d.count("[data-testid='empty-state']")) === 1, "still empty");
      await ctx.shot("Cancelled: still the empty editor, no error");
    },
  },
  {
    id: "3-new-project",
    area: "Projects",
    title: "New project in an empty folder, load audio, then reopen it from the recents list",
    async run(ctx) {
      let { d } = ctx;
      await onboard(ctx);
      const dir = mkdtempSync("/tmp/edytlab-project-");
      await ctx.chooseThrough("[data-testid='new-project-button']", { path: dir });
      await d.until(async () => (await d.invoke("list_recent_projects")).some((p) => (p.path ?? p) === dir), { label: "project in recents" });
      await ctx.shot("A new, empty project");
      await openAudio(ctx, ctx.fixtures.tone);
      await sleep(1000);
      const before = await head(ctx);
      assert(existsSync(join(dir, ".audiograph")), "the project's store is in the chosen folder");
      await ctx.shot("Audio loaded into the new project");
      // Quit and start again, with the same files and keychain.
      d = await ctx.boot(ctx.home, { keepKeyring: true });
      await d.waitFor("[data-testid='status-bar']");
      await sleep(2000);
      await ctx.shot("After a restart");
      if ((await d.count("[data-testid='recent-projects']")) && (await d.count("[data-testid='timeline-lane']")) === 0) {
        await d.click(`[data-testid='recent-open-${dir}']`);
      }
      await d.until(async () => (await d.count("[data-testid='timeline-lane']")) === 1, { timeout: 20000, label: "the project's track" });
      await waitForWaveform(ctx);
      assert((await head(ctx)) === before, "same head as before the restart");
      await ctx.shot("The project is back exactly as it was left");
    },
  },
  {
    id: "3-template",
    area: "Projects",
    title: "Start from a template creates its tracks",
    async run(ctx) {
      const { d } = ctx;
      await onboard(ctx);
      await d.click("[data-testid='empty-state-templates-button']");
      await sleep(800);
      await ctx.shot("The template picker");
      const picked = await d.exec(() => {
        const b = [...document.querySelectorAll("button")].find((e) => /podcast/i.test(e.textContent));
        if (!b) return null;
        b.setAttribute("data-e2e-template", "1");
        return b.textContent.trim();
      });
      assert(picked, "a Podcast template button");
      await d.click("[data-e2e-template='1']");
      await sleep(1500);
      const tracks = await d.invoke("list_tracks");
      await ctx.shot(`After applying the template (${tracks.length} tracks: ${tracks.map((t) => t.name).join(", ")})`);
      assert(tracks.length > 0, "template created tracks");
    },
  },
];
