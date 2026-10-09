// Demo recordings: three professional DJ journeys through the real app,
// driven by real Claude. Each is also a story with assertions, so a demo
// can only show what actually worked.
//
//   ANTHROPIC_E2E_KEY=… RECORD=1 DEMO_MODEL=claude-opus-5-5 ./run-suite.sh 8-demo
//
// Typing is paced like a person's and each step is captioned; the
// captions land in results.json and are burned in by make-demo-videos.sh.
import { copyFileSync, existsSync, mkdirSync, readFileSync } from "node:fs";
import { basename, join } from "node:path";
import { assert, K, openAudio, renderPreview, sleep } from "./helpers.mjs";
import { askThrough, effectsOf, lowPassed, numbers, okBadges, onboardClaude, reply, tracks, wavSeconds } from "./claude.mjs";

const KEY = process.env.ANTHROPIC_E2E_KEY;
const MODEL = process.env.DEMO_MODEL ?? "claude-opus-5-5";
const MUSIC = "/home/dj/Music";

/** Type into the chat box at a person's pace, then send. */
async function typeAndSend(ctx, text) {
  const { d } = ctx;
  const box = "[data-testid='chat-form'] textarea";
  await d.click(box);
  for (let i = 0; i < text.length; i += 2) {
    await d.type(box, text.slice(i, i + 2));
    await sleep(28);
  }
  await sleep(700);
  await d.keys(K.enter);
}

/** One step of the journey: a caption, the request, the whole turn.
 *
 * A step that expects an edit and gets a question back ("Shall I go
 * ahead?") is answered the way a person would, once. */
async function step(ctx, caption, text, { edits = true } = {}) {
  ctx.caption(caption);
  const before = await ctx.d.invoke("get_session_head");
  let badges = await askThrough(ctx, text, caption, { send: typeAndSend });
  if (edits && (await ctx.d.invoke("get_session_head")) === before) {
    const last = await ctx.d.exec(() => {
      const b = [...document.querySelectorAll("[data-testid='message-bubble'][data-role='assistant']")];
      return b.length ? b[b.length - 1].textContent.trim() : "";
    });
    if (last.endsWith("?")) {
      ctx.note(`${caption}: Claude asked before acting; answered yes`);
      await sleep(1500);
      badges = badges.concat(await askThrough(ctx, "Yes, go ahead.", `${caption} (confirmed)`, { send: typeAndSend }));
    }
  }
  await sleep(2500);
  return badges;
}

/** The DJ's library: the demo tracks, copied where a person keeps music. */
function library(ctx, ...keys) {
  mkdirSync(join(MUSIC, "Tracks"), { recursive: true });
  mkdirSync(join(MUSIC, "Mixes"), { recursive: true });
  return keys.map((k) => {
    const to = join(MUSIC, "Tracks", basename(ctx.fixtures[k]));
    copyFileSync(ctx.fixtures[k], to);
    return to;
  });
}

async function start(ctx, files, title) {
  const { d } = ctx;
  await onboardClaude(ctx, { model: MODEL });
  await d.until(async () => (await d.count("[data-testid='settings']")) === 0, { timeout: 60000, label: "the welcome to close" });
  assert((await d.invoke("get_active_provider")) === "anthropic", "Anthropic is active");
  await sleep(800);
  ctx.record();
  ctx.caption(title);
  await sleep(2500);
  ctx.caption("Open the tracks");
  await openAudio(ctx, ...files);
  await sleep(2000);
}

/** Play a few seconds from `at`, so the result is seen moving. */
async function playFrom(ctx, caption, seconds = 6) {
  const { d } = ctx;
  ctx.caption(caption);
  await renderPreview(ctx);
  await d.click("[data-testid='timeline-lane-surface']").catch(() => {});
  await d.keys(K.home);
  await d.keys(" ");
  await sleep(seconds * 1000);
  await d.keys(" ");
  await sleep(1500);
}

function exported(file, { about, tolerance = 1 }) {
  assert(existsSync(file), `the export exists (${file})`);
  const seconds = wavSeconds(readFileSync(file));
  if (about !== undefined) assert(Math.abs(seconds - about) <= tolerance, `the export is about ${about.toFixed(1)} s (${seconds.toFixed(2)})`);
  return seconds;
}

const demos = [
  {
    id: "8-demo-dj-beatmatched-transition",
    title: "Demo: beatmatch and blend two tracks, then master and export the transition",
    async run(ctx) {
      const files = library(ctx, "midnightDrive", "neonRush");
      await start(ctx, files, "A DJ blends Midnight Drive (120 BPM) into Neon Rush (128 BPM)");
      const out = join(MUSIC, "Mixes", "midnight-into-neon.wav");

      await step(ctx, "1 · What am I working with?", "I'm mixing Midnight Drive into Neon Rush. What tempo is each track? Don't change anything yet.", { edits: false });
      const told = numbers(await reply(ctx));
      assert(told.some((n) => Math.abs(n - 120) <= 2) && told.some((n) => Math.abs(n - 128) <= 2), `both tempos found (${told.join(", ")})`);

      const before = (await tracks(ctx))[1].clips[0].length_sec;
      await step(ctx, "2 · Beatmatch", "Time-stretch Neon Rush to Midnight Drive's tempo, keeping its pitch.");
      const after = (await tracks(ctx))[1].clips[0].length_sec;
      assert(Math.abs(after - before * (128 / 120)) < 0.4, `Neon Rush now runs at 120 BPM (${before.toFixed(2)} s to ${after.toFixed(2)} s)`);

      await step(ctx, "3 · Line up the overlap", "Start Neon Rush at 16 seconds, so it plays under the last 8 bars of Midnight Drive.");
      const startSec = (await tracks(ctx))[1].clips[0].start_sec;
      assert(Math.abs(startSec - 16) < 0.06, `Neon Rush starts at 16 s (${startSec})`);

      const blend = await step(
        ctx,
        "4 · Blend: crossfade, low-pass on the outgoing track",
        "Blend them: fade Midnight Drive out from 16 seconds to its end, fade Neon Rush in over its first 16 seconds, and put a low-pass filter on Midnight Drive from 16 seconds to the end so it loses its top end on the way out.",
      );
      assert(await lowPassed(ctx, 0, blend), `Midnight Drive has a low-pass (effects: ${(await effectsOf(ctx, 0)).join(", ")})`);

      await step(
        ctx,
        "5 · Master and export",
        `Master the mix for streaming: compress it gently, put a limiter on the master at -1 dB, normalize it to -14 LUFS, then export it as a WAV file to ${out}`,
      );
      const seconds = exported(out, { about: 16 + after });
      ctx.note(`exported ${basename(out)}: ${seconds.toFixed(2)} s`);

      await playFrom(ctx, "Play it back");
      ctx.caption(`Done: ${basename(out)}, ${seconds.toFixed(1)} s, -14 LUFS`);
      await sleep(3000);
    },
  },
  {
    id: "8-demo-dj-extended-club-intro",
    title: "Demo: build an extended intro for mixing, with a filter that opens into the drop",
    async run(ctx) {
      const files = library(ctx, "solarFlare");
      await start(ctx, files, "A DJ makes a mixable intro for Solar Flare (124 BPM)");
      const out = join(MUSIC, "Mixes", "solar-flare-extended-intro.wav");

      await step(ctx, "1 · Tempo and bar length", "What's the tempo of Solar Flare, and how long is one bar in seconds? Don't change anything yet.", { edits: false });
      const told = numbers(await reply(ctx));
      assert(told.some((n) => Math.abs(n - 124) <= 2), `124 BPM found (${told.join(", ")})`);

      const lengthBefore = (await tracks(ctx))[0].clips.reduce((a, c) => Math.max(a, c.start_sec + c.length_sec), 0);
      await step(ctx, "2 · Extend the intro", "The first 8 bars are drums only. Repeat those 8 bars once more at the start, so the intro is 16 bars long and the rest of the track follows.");
      const lengthAfter = (await tracks(ctx))[0].clips.reduce((a, c) => Math.max(a, c.start_sec + c.length_sec), 0);
      const eightBars = 8 * 4 * (60 / 124);
      assert(Math.abs(lengthAfter - lengthBefore - eightBars) < 0.5, `the track is 8 bars longer (${lengthBefore.toFixed(2)} s to ${lengthAfter.toFixed(2)} s)`);

      const build = await step(ctx, "3 · A filter that opens into the drop", "Put a high-pass filter on the 16-bar intro, so the low end is held back and comes in at the drop.");
      const fx = await effectsOf(ctx, 0);
      assert(fx.some((k) => /high.?pass|eq/i.test(k)) || okBadges(build).some((b) => /high.?pass|filter|eq/i.test(b.text)), `a high-pass is on the intro (effects: ${fx.join(", ")}; tools: ${build.map((b) => b.text).join(", ")})`);

      await step(ctx, "4 · Fade in and export", `Fade in the first 4 bars, then export the track as a WAV file to ${out}`);
      const seconds = exported(out, { about: lengthAfter });
      ctx.note(`exported ${basename(out)}: ${seconds.toFixed(2)} s`);

      await playFrom(ctx, "Play the new intro");
      ctx.caption(`Done: ${basename(out)}, a 16-bar intro`);
      await sleep(3000);
    },
  },
  {
    id: "8-demo-dj-mini-mix",
    title: "Demo: a three-track mini-mix, tempo- and loudness-matched, limited and exported",
    async run(ctx) {
      const files = library(ctx, "midnightDrive", "solarFlare", "neonRush");
      await start(ctx, files, "A DJ builds a three-track mini-mix at 124 BPM");
      const out = join(MUSIC, "Mixes", "mini-mix-124bpm.wav");

      await step(ctx, "1 · Match every track to 124 BPM", "Time-stretch every track to 124 BPM, keeping their pitch. Solar Flare is already at 124.");
      // Picked together, the files load in name order, so tracks are
      // found by name, not by the order they were listed in.
      const named = (all, prefix) => {
        const t = all.find((x) => x.name.startsWith(prefix));
        assert(t, `a track named ${prefix}… (${all.map((x) => x.name).join(", ")})`);
        return t;
      };
      const t1 = await tracks(ctx);
      const lenOf = (t) => t.clips.reduce((a, c) => a + c.length_sec, 0);
      assert(Math.abs(lenOf(named(t1, "midnight")) - 32 * (120 / 124)) < 0.4, `Midnight Drive is at 124 BPM (${lenOf(named(t1, "midnight")).toFixed(2)} s)`);
      assert(Math.abs(lenOf(named(t1, "neon")) - 32 * (128 / 124)) < 0.4, `Neon Rush is at 124 BPM (${lenOf(named(t1, "neon")).toFixed(2)} s)`);
      assert(Math.abs(lenOf(named(t1, "solar")) - 32) < 0.1, "Solar Flare is left alone");

      await step(
        ctx,
        "2 · Sequence with 4-bar overlaps",
        "Sequence them: Midnight Drive first, then Solar Flare starting 4 bars before Midnight Drive ends, then Neon Rush starting 4 bars before Solar Flare ends. Crossfade each overlap.",
      );
      const all2 = await tracks(ctx);
      const t2 = [named(all2, "midnight"), named(all2, "solar"), named(all2, "neon")];
      const bar = 4 * (60 / 124);
      const endOf = (t) => t.clips.reduce((a, c) => Math.max(a, c.start_sec + c.length_sec), 0);
      const startOf = (t) => Math.min(...t.clips.map((c) => c.start_sec));
      ctx.note(`starts: ${t2.map((t) => `${t.name} ${startOf(t).toFixed(2)}-${endOf(t).toFixed(2)} s`).join("; ")}`);
      assert(Math.abs(startOf(t2[1]) - (endOf(t2[0]) - 4 * bar)) < 0.3, `Solar Flare comes in 4 bars before Midnight Drive ends (${startOf(t2[1]).toFixed(2)} s)`);
      assert(Math.abs(startOf(t2[2]) - (endOf(t2[1]) - 4 * bar)) < 0.3, `Neon Rush comes in 4 bars before Solar Flare ends (${startOf(t2[2]).toFixed(2)} s)`);

      const level = await step(ctx, "3 · Match loudness, limit the master", "Match their loudness so no track jumps out: bring each track to the same level, around -14 LUFS, and put a limiter on the master at -1 dB.");
      const node = await ctx.d.invoke("get_node", { id: await ctx.d.invoke("get_session_head") });
      const master = (node.state.master_chain ?? []).map((e) => e.kind);
      assert(master.some((k) => /limit/i.test(k)) || okBadges(level).some((b) => /limit/i.test(b.text)), `a limiter is on the master (master chain: ${master.join(", ")})`);

      await step(ctx, "4 · Export the mix", `Export the mix as a WAV file to ${out}`);
      const seconds = exported(out, { about: endOf(t2[2]) });
      ctx.note(`exported ${basename(out)}: ${seconds.toFixed(2)} s`);

      await playFrom(ctx, "Play the mix", 7);
      ctx.caption(`Done: ${basename(out)}, ${seconds.toFixed(0)} s`);
      await sleep(3000);
    },
  },
];

export default KEY ? demos.map((s) => ({ area: "Demos", ...s })) : [];
