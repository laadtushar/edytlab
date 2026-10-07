// What every story does first, done the way a person does it.
import { K } from "../webdriver.mjs";

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
export { K };

/** First launch: pick local Ollama (the fake model). Keyless, so the
 * blocking welcome closes as soon as it is chosen. */
export async function onboard(ctx) {
  const { d } = ctx;
  await d.waitFor("[data-testid='settings']");
  await sleep(600);
  await d.click("[data-testid='settings-provider-ollama']");
  await d.until(async () => (await d.count("[data-testid='settings']")) === 0, { label: "welcome to close" });
}

/** Open Audio, answering the OS file dialog with `paths`. */
export async function openAudio(ctx, ...paths) {
  await ctx.answerDialogs(paths);
  await ctx.d.click("[data-testid='open-audio-button']");
  await ctx.d.until(async () => (await ctx.d.count("[data-testid='timeline-lane']")) >= paths.length, {
    timeout: 30000,
    label: `${paths.length} lane(s)`,
  });
  await waitForWaveform(ctx);
}

/** Wait until a lane's waveform canvas has ink on it. */
export async function waitForWaveform(ctx) {
  await ctx.d.until(
    () =>
      ctx.d.exec(() => {
        const c = document.querySelector("[data-testid='timeline-lane'] canvas");
        if (!c || !c.width) return false;
        const g = c.getContext("2d");
        const px = g.getImageData(0, 0, c.width, c.height).data;
        for (let i = 3; i < px.length; i += 4 * 97) if (px[i] > 0) return true;
        return false;
      }),
    { timeout: 20000, label: "waveform ink" },
  );
}

export async function head(ctx) {
  return (await ctx.d.text("[data-testid='status-bar-head']")).replace(/^head\s*/i, "").trim();
}

export async function waitForNewHead(ctx, before) {
  return ctx.d.until(async () => {
    const h = await head(ctx);
    return h && h !== before && !/no head/i.test(h) ? h : false;
  }, { timeout: 20000, label: `head to move from ${before}` });
}

/** Render the mix, which playback needs. */
export async function renderPreview(ctx) {
  await ctx.d.click("[data-testid='render-preview-button']");
  await ctx.d.until(async () => /ready/i.test(await ctx.d.text("[data-testid='status-bar-state']")) && (await ctx.d.count("[data-testid='status-bar-mix-stale']")) === 0, {
    timeout: 30000,
    label: "preview rendered",
  });
}

export function assert(cond, msg) {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
}
