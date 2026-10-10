// Demo recordings: three professional DJ journeys through the real app,
// driven by real Claude. Each is also a story with assertions, so a demo
// can only show what actually worked.
//
//   ANTHROPIC_E2E_KEY=… RECORD=1 RECORD_AUDIO=demo.monitor \
//     DEMO_MODEL=claude-opus-5-5 ./run-suite.sh 8-demo
//
// They are played the way a person would: the pointer travels to every
// control it presses, requests are typed into the chat, and the result of
// the work is played back, so the film has something to hear. Captions
// and pace marks land in results.json; make-demo-videos.mjs burns the
// captions in and plays the waits faster than the listening.
import { existsSync, mkdirSync, readFileSync, copyFileSync, rmSync } from "node:fs";
import { basename, join } from "node:path";
import { assert, dismissKeychainNotice, K, sleep, waitForWaveform } from "./helpers.mjs";
import { askThrough, effectsOf, lowPassed, numbers, okBadges, onboardClaude, reply, tracks, wavSeconds } from "./claude.mjs";
import { pointAt, press, rest } from "./human.mjs";

const KEY = process.env.ANTHROPIC_E2E_KEY;
const MODEL = process.env.DEMO_MODEL ?? "claude-opus-5-5";
const EFFORT = process.env.DEMO_EFFORT; // low | medium | high | xhigh | max
const MUSIC = "/home/dj/Music";

// How fast each kind of moment plays in the finished video.
const LISTEN = 1;
const HANDS = 1.25; // pointing, clicking, reading a reply
const TYPING = 1.75;
const WAITING = 3.5; // Claude working

const box = "[data-testid='chat-form'] textarea";

/** Type into the chat box at a person's pace, then send. */
async function typeAndSend(ctx, text) {
  const { d } = ctx;
  ctx.pace(HANDS);
  await press(ctx, box);
  ctx.pace(TYPING);
  for (const ch of text) {
    await d.type(box, ch);
    await sleep(20 + Math.random() * 45);
  }
  await sleep(700);
  await d.keys(K.enter);
  await rest(ctx);
  ctx.pace(WAITING);
}

/** One step of the journey: a caption, the request, the whole turn.
 *
 * A step that expects an edit and gets a question back ("Shall I go
 * ahead?") is answered the way a person would, once. */
async function step(ctx, caption, text, { edits = true } = {}) {
  ctx.caption(caption);
  const before = await ctx.d.invoke("get_session_head");
  const opts = { send: typeAndSend, press };
  let badges = await askThrough(ctx, text, caption, opts);
  if (edits && (await ctx.d.invoke("get_session_head")) === before) {
    const last = await ctx.d.exec(() => {
      const b = [...document.querySelectorAll("[data-testid='message-bubble'][data-role='assistant']")];
      return b.length ? b[b.length - 1].textContent.trim() : "";
    });
    if (last.endsWith("?")) {
      ctx.note(`${caption}: Claude asked before acting; answered yes`);
      ctx.pace(HANDS);
      await sleep(2500);
      badges = badges.concat(await askThrough(ctx, "Yes, go ahead.", `${caption} (confirmed)`, opts));
    }
  }
  // Time to read the reply and the tools it ran.
  ctx.pace(HANDS);
  await sleep(4000);
  return badges;
}

/** The DJ's folder for this set: just these tracks, where a person keeps
 * music, so the file chooser shows a real place. */
function library(ctx, folder, ...keys) {
  const dir = join(MUSIC, folder);
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  mkdirSync(join(MUSIC, "Mixes"), { recursive: true });
  return {
    dir,
    files: keys.map((k) => {
      const to = join(dir, basename(ctx.fixtures[k]));
      copyFileSync(ctx.fixtures[k], to);
      return to;
    }),
  };
}

async function start(ctx, { dir, files }, title) {
  const { d } = ctx;
  await onboardClaude(ctx, { model: MODEL, effort: EFFORT });
  await d.until(async () => (await d.count("[data-testid='settings']")) === 0, { timeout: 60000, label: "the welcome to close" });
  // Before recording: the demo videos should not open on a warning about
  // the machine that records them.
  await dismissKeychainNotice(ctx);
  assert((await d.invoke("get_active_provider")) === "anthropic", "Anthropic is active");
  await sleep(800);
  ctx.record();
  ctx.pace(HANDS);
  ctx.caption(title);
  await sleep(3000);
  ctx.caption("Open the tracks");
  await pointAt(ctx, "[data-testid='open-audio-button']");
  const answer = files.length === 1 ? { path: files[0], typeDelay: 40 } : { paths: files, dir, typeDelay: 40 };
  await ctx.chooseThrough("[data-testid='open-audio-button']", answer, { shot: false });
  await d.until(async () => (await d.count("[data-testid='timeline-lane']")) >= files.length, { timeout: 30000, label: "the lanes" });
  await waitForWaveform(ctx);
  await rest(ctx);
  await sleep(2500);
}

/** The session's length: where the last clip ends. */
const sessionEnd = async (ctx) => (await tracks(ctx)).reduce((m, t) => Math.max(m, ...t.clips.map((c) => c.start_sec + c.length_sec)), 0);

/** Is there a Play button to press (#425)? Without one, playback is the
 * keyboard's: Space, Home and the arrows. */
const hasPlayButton = async (ctx) => (await ctx.d.count("[data-testid='play-pause-button']")) > 0;

/** Put the playhead at `sec`: a click on the timeline where the app takes
 * one, else Home and the arrow keys (5 s, and 1 s with Shift). */
async function seek(ctx, sec) {
  const { d } = ctx;
  const lane = "[data-testid='timeline-lane-surface']";
  if (await hasPlayButton(ctx)) {
    await press(ctx, lane, { fx: Math.min(0.995, sec / (await sessionEnd(ctx))), fy: 0.5 });
    return;
  }
  // A click near the start takes the keys away from the chat box.
  await press(ctx, lane, { fx: 0.01, fy: 0.5 });
  await d.keys(K.home);
  for (let i = 0; i < Math.floor(sec / 5); i++) {
    await d.keys(K.right);
    await sleep(120);
  }
  for (let i = 0; i < Math.round(sec % 5); i++) {
    await d.keys(K.shift, K.right);
    await sleep(120);
  }
}

async function togglePlay(ctx) {
  if (await hasPlayButton(ctx)) await press(ctx, "[data-testid='play-pause-button']");
  else await ctx.d.keys(" ");
}

/** Render the preview mix, which is what plays. Every edit drops it, and
 * Play renders a new one when there is none (#431). The demo renders with
 * Preview anyway, because it seeks to a point before playing and a seek
 * needs the mix to have loaded first. */
async function freshMix(ctx) {
  const { d } = ctx;
  await press(ctx, "[data-testid='render-preview-button']");
  await d.until(async () => /ready/i.test(await d.text("[data-testid='status-bar-state']")) && (await d.count("[data-testid='status-bar-mix-stale']")) === 0, {
    timeout: 60000,
    label: "the preview mix",
  });
  // With a Play button (#425), it enables once the mix has loaded.
  if (await hasPlayButton(ctx)) {
    await d.until(async () => d.exec(() => !document.querySelector("[data-testid='play-pause-button']").disabled), {
      timeout: 30000,
      label: "Play to enable",
    });
  }
}

/** Listen to `seconds` from `from`, in real time on the film. */
async function listen(ctx, caption, from, seconds) {
  ctx.pace(HANDS);
  ctx.caption(caption);
  await freshMix(ctx);
  await seek(ctx, from);
  await sleep(400);
  ctx.pace(LISTEN);
  await togglePlay(ctx);
  await sleep(seconds * 1000);
  await togglePlay(ctx);
  await sleep(600);
  ctx.pace(HANDS);
  await rest(ctx);
}

function exported(file, { about, tolerance = 1 }) {
  assert(existsSync(file), `the export exists (${file})`);
  const seconds = wavSeconds(readFileSync(file));
  if (about !== undefined) assert(Math.abs(seconds - about) <= tolerance, `the export is about ${about.toFixed(1)} s (${seconds.toFixed(2)})`);
  return seconds;
}

/** Seconds into a WAV file before its first non-silent sample (above
 * -80 dBFS on any channel: padding is written as digital zeros, while a
 * fade-in starting from silence crosses this within milliseconds), or 0
 * for a file this cannot read. A track
 * can come in late two ways: its clip moved along the timeline, or its
 * audio padded with silence in front. Both sound the same, so a check
 * of where a track comes in has to hear the silence, not just read the
 * clip's start. */
function wavOnset(path) {
  if (!/\.wav$/i.test(path) || !existsSync(path)) return 0;
  const b = readFileSync(path);
  const format = b.readUInt16LE(20);
  const channels = b.readUInt16LE(22);
  const rate = b.readUInt32LE(24);
  const bits = b.readUInt16LE(34);
  let at = 12;
  while (at < b.length - 8) {
    const id = b.subarray(at, at + 4).toString();
    const size = b.readUInt32LE(at + 4);
    if (id === "data") {
      const bytes = bits / 8;
      const frame = bytes * channels;
      const end = Math.min(b.length, at + 8 + size);
      const read = (o) =>
        format === 3 && bits === 32
          ? b.readFloatLE(o)
          : bits === 16
            ? b.readInt16LE(o) / 32768
            : bits === 24
              ? b.readIntLE(o, 3) / 8388608
              : bits === 32
                ? b.readInt32LE(o) / 2147483648
                : 1;
      for (let o = at + 8, i = 0; o + frame <= end; o += frame, i++) {
        for (let c = 0; c < channels; c++) if (Math.abs(read(o + c * bytes)) > 1e-4) return i / rate;
      }
      return 0;
    }
    at += 8 + size + (size % 2);
  }
  return 0;
}

const mmss = (s) => {
  const whole = Math.round(s);
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, "0")}`;
};

/** Check a placement the way a DJ would look at the timeline: if Claude
 * is off, say so once, with the numbers, and let it fix it on camera.
 * `read` returns the values to check; `off` returns a correction prompt
 * when they are wrong, or null when they are right. Asserts after one
 * correction, so a demo can never publish a wrong edit. */
async function settle(ctx, caption, read, off, describe) {
  const first = await read();
  const ask = off(first);
  if (!ask) return first;
  ctx.note(`${caption}: correction needed (${describe(first)})`);
  await step(ctx, caption, ask);
  const second = await read();
  assert(!off(second), `${caption} — still off after one correction (${describe(second)})`);
  return second;
}

const demos = [
  {
    id: "8-demo-dj-beatmatched-transition",
    title: "Demo: beatmatch and blend two tracks, then master and export the transition",
    async run(ctx) {
      const lib = library(ctx, "Friday set", "midnightDrive", "neonRush");
      await start(ctx, lib, "A DJ blends Midnight Drive (120 BPM) into Neon Rush (128 BPM)");
      const out = join(MUSIC, "Mixes", "midnight-into-neon.wav");

      await listen(ctx, "Before: both tracks from the top, 120 against 128 BPM", 0, 5);

      await step(ctx, "1 · What am I working with?", "I'm mixing Midnight Drive into Neon Rush. What tempo is each track? Don't change anything yet.", { edits: false });
      const told = numbers(await reply(ctx));
      assert(told.some((n) => Math.abs(n - 120) <= 2) && told.some((n) => Math.abs(n - 128) <= 2), `both tempos found (${told.join(", ")})`);

      const before = (await tracks(ctx))[1].clips[0].length_sec;
      await step(ctx, "2 · Beatmatch", "Time-stretch Neon Rush to Midnight Drive's tempo, keeping its pitch.");
      const after = (await tracks(ctx))[1].clips[0].length_sec;
      assert(Math.abs(after - before * (128 / 120)) < 0.4, `Neon Rush now runs at 120 BPM (${before.toFixed(2)} s to ${after.toFixed(2)} s)`);

      await step(ctx, "3 · Line up the overlap", "Start Neon Rush at 16 seconds, so it plays under the last 8 bars of Midnight Drive.");
      await settle(
        ctx,
        "Nudge it onto the bar",
        async () => (await tracks(ctx))[1].clips[0].start_sec,
        (s) => (Math.abs(s - 16) < 0.06 ? null : `Neon Rush starts at ${s.toFixed(2)} s. Move it so it starts at exactly 16.00 s.`),
        (s) => `Neon Rush at ${s.toFixed(3)} s`,
      );

      const blend = await step(
        ctx,
        "4 · Blend: crossfade, low-pass on the outgoing track",
        "Blend them: fade Midnight Drive out from 16 seconds to its end, fade Neon Rush in over its first 16 seconds, and put a low-pass filter on Midnight Drive from 16 seconds to the end so it loses its top end on the way out.",
      );
      assert(await lowPassed(ctx, 0, blend), `Midnight Drive has a low-pass (effects: ${(await effectsOf(ctx, 0)).join(", ")})`);

      await listen(ctx, "Listen: the transition, from 0:12", 12, 16);

      // Two requests, not one: mastering alone is several edits, and
      // splitting keeps each request well inside the per-request tool
      // budget (#439).
      await step(ctx, "5 · Master for streaming", "Master the mix for streaming: compress it gently, put a limiter on the master at -1 dB, and normalize it to -14 LUFS.");
      await step(ctx, "6 · Export", `Export the mix as a WAV file to ${out}`);
      const seconds = exported(out, { about: 16 + after });
      ctx.note(`exported ${basename(out)}: ${seconds.toFixed(2)} s`);

      await listen(ctx, "The mastered mix, from 0:20", 20, 10);
      ctx.caption(`Done: ${basename(out)}, ${mmss(seconds)}, -14 LUFS`);
      await sleep(3500);
    },
  },
  {
    id: "8-demo-dj-extended-club-intro",
    title: "Demo: build an extended intro for mixing, with a filter that opens into the drop",
    async run(ctx) {
      const lib = library(ctx, "Solar Flare edit", "solarFlare");
      await start(ctx, lib, "A DJ makes a mixable intro for Solar Flare (124 BPM)");
      const out = join(MUSIC, "Mixes", "solar-flare-extended-intro.wav");

      await listen(ctx, "Before: the original intro", 0, 6);

      await step(ctx, "1 · Tempo and bar length", "What's the tempo of Solar Flare, and how long is one bar in seconds? Don't change anything yet.", { edits: false });
      const told = numbers(await reply(ctx));
      assert(told.some((n) => Math.abs(n - 124) <= 2), `124 BPM found (${told.join(", ")})`);

      const lengthBefore = await sessionEnd(ctx);
      await step(ctx, "2 · Extend the intro", "The first 8 bars are drums only. Repeat those 8 bars once more at the start, so the intro is 16 bars long and the rest of the track follows.");
      const lengthAfter = await sessionEnd(ctx);
      const eightBars = 8 * 4 * (60 / 124);
      assert(Math.abs(lengthAfter - lengthBefore - eightBars) < 0.5, `the track is 8 bars longer (${lengthBefore.toFixed(2)} s to ${lengthAfter.toFixed(2)} s)`);

      const build = await step(ctx, "3 · A filter that opens into the drop", "Put a high-pass filter on the 16-bar intro, so the low end is held back and comes in at the drop.");
      const fx = await effectsOf(ctx, 0);
      assert(fx.some((k) => /high.?pass|eq/i.test(k)) || okBadges(build).some((b) => /high.?pass|filter|eq/i.test(b.text)), `a high-pass is on the intro (effects: ${fx.join(", ")}; tools: ${build.map((b) => b.text).join(", ")})`);

      await step(ctx, "4 · Fade in and export", `Fade in the first 4 bars, then export the track as a WAV file to ${out}`);
      const seconds = exported(out, { about: lengthAfter });
      ctx.note(`exported ${basename(out)}: ${seconds.toFixed(2)} s`);

      const drop = 16 * 4 * (60 / 124);
      await listen(ctx, "Listen: the new intro fades in", 0, 7);
      await listen(ctx, `Listen: the low end arrives at the drop (${mmss(drop)})`, Math.floor(drop - 5), 10);
      ctx.caption(`Done: ${basename(out)}, a 16-bar intro`);
      await sleep(3500);
    },
  },
  {
    id: "8-demo-dj-mini-mix",
    title: "Demo: a three-track mini-mix, tempo- and loudness-matched, limited and exported",
    async run(ctx) {
      const lib = library(ctx, "Warm-up mix", "midnightDrive", "solarFlare", "neonRush");
      await start(ctx, lib, "A DJ builds a three-track mini-mix at 124 BPM");
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
      const bar = 4 * (60 / 124);
      const endOf = (t) => t.clips.reduce((a, c) => Math.max(a, c.start_sec + c.length_sec), 0);
      // Where the track is first heard: its clip's start plus any silence
      // in front of the audio (a clip reads its file from the start).
      const startOf = (t) => Math.min(...t.clips.map((c) => c.start_sec + wavOnset(c.source_path)));
      // On the bar grid of the track before it, within a bar of the 4-bar
      // mark. A track that is not a whole number of bars long (Solar Flare
      // is 16½) puts the exact 4-bar mark between downbeats, and a DJ
      // brings the next track in on the downbeat: either is right.
      const lands = (start, want, gridFrom) => {
        if (Math.abs(start - want) < 0.3) return true;
        const bars = (start - gridFrom) / bar;
        return Math.abs(start - want) <= bar + 0.05 && Math.abs(bars - Math.round(bars)) * bar < 0.06;
      };
      const readMix = async () => {
        const all = await tracks(ctx);
        return [named(all, "midnight"), named(all, "solar"), named(all, "neon")];
      };
      // Where each track should come in, from where the one before ends:
      // 4 bars (7.74 s at 124 BPM) before it.
      const targets = (t) => {
        const solar = endOf(t[0]) - 4 * bar;
        const neon = solar + (endOf(t[1]) - startOf(t[1])) - 4 * bar;
        return { solar, neon };
      };
      const t2 = await settle(
        ctx,
        "Nudge the overlaps onto the bar",
        readMix,
        (t) => {
          const want = targets(t);
          const fixes = [];
          if (!lands(startOf(t[1]), want.solar, startOf(t[0]))) fixes.push(`Solar Flare at ${want.solar.toFixed(2)} s`);
          if (!lands(startOf(t[2]), want.neon, startOf(t[1]))) fixes.push(`Neon Rush at ${want.neon.toFixed(2)} s`);
          return fixes.length
            ? `The overlaps are off. Each track should come in 4 bars (${(4 * bar).toFixed(2)} s) before the previous one ends, so start ${fixes.join(" and ")}, and keep the crossfades.`
            : null;
        },
        (t) => t.map((x) => `${x.name} ${startOf(x).toFixed(2)}-${endOf(x).toFixed(2)} s`).join("; "),
      );
      ctx.note(`starts: ${t2.map((t) => `${t.name} ${startOf(t).toFixed(2)}-${endOf(t).toFixed(2)} s`).join("; ")}`);

      const level = await step(ctx, "3 · Match loudness, limit the master", "Match their loudness so no track jumps out: bring each track to the same level, around -14 LUFS, and put a limiter on the master at -1 dB.");
      const node = await ctx.d.invoke("get_node", { id: await ctx.d.invoke("get_session_head") });
      const master = (node.state.master_chain ?? []).map((e) => e.kind);
      assert(master.some((k) => /limit/i.test(k)) || okBadges(level).some((b) => /limit/i.test(b.text)), `a limiter is on the master (master chain: ${master.join(", ")})`);

      const first = startOf(t2[1]);
      const second = startOf(t2[2]);
      await listen(ctx, `Listen: Midnight Drive into Solar Flare (${mmss(first)})`, Math.max(0, Math.floor(first - 3)), 11);
      await listen(ctx, `Listen: Solar Flare into Neon Rush (${mmss(second)})`, Math.max(0, Math.floor(second - 3)), 11);

      await step(ctx, "4 · Export the mix", `Export the mix as a WAV file to ${out}`);
      const seconds = exported(out, { about: endOf(t2[2]) });
      ctx.note(`exported ${basename(out)}: ${seconds.toFixed(2)} s`);

      ctx.caption(`Done: ${basename(out)}, ${mmss(seconds)}`);
      await sleep(3500);
    },
  },
];

export default KEY ? demos.map((s) => ({ area: "Demos", ...s })) : [];
