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

/** Open Audio through the real file chooser, selecting `paths`. */
export async function openAudio(ctx, ...paths) {
  const answer = paths.length === 1 ? { path: paths[0] } : { paths };
  await ctx.chooseThrough("[data-testid='open-audio-button']", answer);
  await ctx.d.until(async () => (await ctx.d.count("[data-testid='timeline-lane']")) >= paths.length, {
    timeout: 30000,
    label: `${paths.length} lane(s)`,
  });
  await waitForWaveform(ctx);
}

/** Wait until a lane's waveform has ink on it. WaveSurfer draws into
 * shadow roots, so canvases are found through them. */
export async function waitForWaveform(ctx) {
  await ctx.d.until(() => ctx.d.exec(waveformHasInk), { timeout: 45000, label: "waveform ink" });
}

/** Runs in the page. */
export function waveformHasInk() {
  const lane = document.querySelector("[data-testid='timeline-lane']");
  if (!lane) return false;
  const canvases = [];
  const walk = (root) => {
    root.querySelectorAll("canvas").forEach((c) => canvases.push(c));
    root.querySelectorAll("*").forEach((e) => e.shadowRoot && walk(e.shadowRoot));
  };
  walk(lane);
  for (const c of canvases) {
    if (!c.width || !c.height) continue;
    const px = c.getContext("2d").getImageData(0, 0, c.width, c.height).data;
    for (let i = 3; i < px.length; i += 4 * 61) if (px[i] > 0) return true;
  }
  return false;
}

/** The head shown in the status bar, lower-cased: WebDriver returns the
 * rendered text, and the bar uppercases it with CSS. */
export async function head(ctx) {
  return (await ctx.d.text("[data-testid='status-bar-head']")).replace(/^head\s*/i, "").trim().toLowerCase();
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
