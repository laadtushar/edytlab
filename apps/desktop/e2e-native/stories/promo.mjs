// The v0.7.0 promo's footage: eight shots of the real app, filmed live.
//
// The app is the released v0.7.0 build (point APP at its binary), the
// assistant is real Claude through the app's own Anthropic provider, and
// nothing on screen is scripted: no replies, no tool calls, no stepped clock.
// The only things set up beforehand are the project's contents (audio written
// from arithmetic, a history made with the app's own commands) and the window
// (a 1600x1000 page at the display's scale of 2), and none of that is on film.
// Each story is also a test: it asserts what the footage claims, so a shot
// whose assertion fails is not one to use.
//
//   PROMO=1 RECORD=1 RECORD_AUDIO=demo.monitor SCREEN=3300x2200x24 GDK_SCALE=2 \
//     ANTHROPIC_E2E_KEY=… DEMO_MODEL=claude-sonnet-5-5 DEMO_EFFORT=low \
//     ./run-suite.sh 9-promo-s1
//
// Without ANTHROPIC_E2E_KEY the stories run against the scripted model
// (fake-llm.mjs, free), to rehearse the screen work; that is a rehearsal, not
// footage. `marketing/promo-v0.7.0/record/make-footage.mjs` turns what these
// leave in $OUT (videos/, results.json, promo/) into the footage folder.
import { copyFileSync, existsSync, mkdirSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { spawn } from "node:child_process";
import { join } from "node:path";
import { x } from "../native.mjs";
import { assert, dismissKeychainNotice, head, K, onboard, sleep, waitForNewHead, waitForWaveform } from "./helpers.mjs";
import { onboardClaude, askThrough, lastReply, okBadges, tracks } from "./claude.mjs";
import { toolBadges } from "./agent.mjs";
import { origin, pointAt, press, rest } from "./human.mjs";
import { writeStems } from "./promo-audio.mjs";
import { wav } from "../fixtures.mjs";

const KEY = process.env.ANTHROPIC_E2E_KEY;
const LIVE = Boolean(KEY);
const MODEL = process.env.DEMO_MODEL ?? "claude-sonnet-5-5";
const EFFORT = process.env.DEMO_EFFORT ?? "low";
const PAGE = { w: 1600, h: 1000 }; // CSS px: the page the footage shows
const TITLE_BAR = 25; // CSS px GTK draws above the page, inside the window
const box = "[data-testid='chat-form'] textarea";
const rand = (a, b) => a + Math.random() * (b - a);

/** What a story leaves for the footage maker: $OUT/promo/<id>.notes.json. */
function saveNotes(ctx, id, data) {
  mkdirSync(join(ctx.out, "promo"), { recursive: true });
  writeFileSync(join(ctx.out, "promo", `${id}.notes.json`), JSON.stringify({ live: LIVE, model: LIVE ? MODEL : "scripted rehearsal", effort: LIVE ? EFFORT : null, ...data }, null, 2));
}

/** A moment of the film, by name: lands in results.json's captions, with its time. */
const mark = (ctx, label) => ctx.caption(label);

/** Put the window at the top left, sized so the page is exactly PAGE at the
 * display's scale, and say where on the screen the page is. */
async function stageWindow(ctx) {
  const { d } = ctx;
  const scale = await d.exec(() => window.devicePixelRatio || 1);
  const win = x("search", "--onlyvisible", "--name", "edytlab").split("\n").filter(Boolean)[0];
  assert(win, "the app's window was found");
  x("windowmove", win, "0", "0");
  let h = PAGE.h + TITLE_BAR;
  for (let i = 0; i < 4; i++) {
    x("windowsize", win, String(Math.round(PAGE.w * scale)), String(Math.round(h * scale)));
    await sleep(1200);
    const now = await d.exec(() => ({ w: innerWidth, h: innerHeight }));
    if (now.w === PAGE.w && now.h === PAGE.h) break;
    assert(i < 3, `the page is ${PAGE.w}x${PAGE.h} (it is ${now.w}x${now.h})`);
    h += PAGE.h - now.h;
  }
  const o = await origin(ctx);
  await rest(ctx);
  return { scale, region: { x: o.x, y: o.y, w: PAGE.w * scale, h: PAGE.h * scale } };
}

/** First launch: the provider, as a person sets it (the key is never on film or in a screenshot). */
async function connect(ctx) {
  const { d } = ctx;
  if (LIVE) {
    await onboardClaude(ctx, { model: MODEL, effort: EFFORT });
    await d.until(async () => (await d.count("[data-testid='settings']")) === 0, { timeout: 60000, label: "the welcome to close" });
    await dismissKeychainNotice(ctx);
    assert((await d.invoke("get_active_provider")) === "anthropic", "Anthropic is the active provider");
  } else {
    await onboard(ctx);
  }
}

/** Where a shot's audio lives. */
// A short path, because the History graph prints the path of a loaded file on its node.
const audioDir = (ctx, name) => join("/tmp/promo-audio", name);

/** Load `files` ({ "Name.wav": parts }) as tracks through the real file chooser, as Open Audio does:
 * the files are alone in a folder of their own, which is opened and everything in it selected, so
 * they load in name order. */
async function load(ctx, files, dir = "project") {
  const folder = audioDir(ctx, dir);
  rmSync(folder, { recursive: true, force: true });
  const paths = writeStems(folder, files);
  const lanes = (await ctx.d.count("[data-testid='timeline-lane']")) + paths.length;
  await ctx.chooseThrough("[data-testid='open-audio-button']", { paths, dir: folder, typeDelay: 15 }, { shot: false });
  await ctx.d.until(async () => (await ctx.d.count("[data-testid='timeline-lane']")) >= Math.min(lanes, 6), { timeout: 120000, label: "the lanes" });
  await waitForWaveform(ctx);
}

/** Type into the chat box the way a person does: a real click in the box, then real key events
 * (xdotool, a word at a time at a human pace), then Enter. */
async function typeAndSend(ctx, text) {
  await press(ctx, box);
  await sleep(600);
  for (const word of text.match(/\S+\s*/g)) {
    x("type", "--clearmodifiers", "--delay", String(Math.round(rand(38, 80))), "--", word);
    await sleep(rand(30, 150));
  }
  await sleep(800);
  x("key", "Return");
  await rest(ctx);
}

/** One request to the assistant: typed, sent, and the whole turn waited for.
 * Returns what the chat shows afterwards. */
async function ask(ctx, text, label, script) {
  if (!LIVE && script) await ctx.script(script);
  const started = Date.now();
  const badges = await askThrough(ctx, text, label, { send: typeAndSend, press });
  return { badges, reply: await lastReply(ctx), seconds: (Date.now() - started) / 1000 };
}

/** The mix's own Play button: press it and wait until it plays. */
async function play(ctx) {
  const { d } = ctx;
  await press(ctx, "[data-testid='play-pause-button']");
  await d.until(async () => (await d.attr("[data-testid='play-pause-button']", "aria-label")) === "Pause", { timeout: 60000, label: "the mix to play" });
}
const pause = (ctx) => press(ctx, "[data-testid='play-pause-button']");

/** A flash of the page and a 1 kHz tone at the same instant, for lining the recorded sound up with the recorded
 * picture: PulseAudio's capture and x11grab's frames do not share a clock, and how far apart they run changes from
 * one recording to the next. The footage maker finds the flash and the tone in the recording, measures the gap and
 * cuts the clip from the mark "start", after both. */
async function beacon(ctx) {
  const tone = join(ctx.out, "promo-audio", "tone.wav");
  if (!existsSync(tone)) {
    mkdirSync(join(ctx.out, "promo-audio"), { recursive: true });
    writeFileSync(tone, wav({ seconds: 0.3, sample: (t) => Math.sin(2 * Math.PI * 1000 * t) * 0.6 }));
  }
  const player = spawn("paplay", ["--device=demo", tone], { stdio: "ignore" });
  // A white block over the top left third of the page: a cheap repaint (inverting the whole page stalls the capture).
  await ctx.d.exec(() => {
    const b = document.createElement("div");
    b.id = "promo-beacon";
    b.style.cssText = "position:fixed;left:0;top:0;width:800px;height:500px;background:#fff;z-index:2147483647";
    document.body.appendChild(b);
  });
  mark(ctx, "beacon");
  await sleep(450);
  await ctx.d.exec(() => document.getElementById("promo-beacon")?.remove());
  if (player.exitCode === null) await new Promise((r) => player.on("exit", r));
  await sleep(900);
  mark(ctx, "start");
}

/** Start filming. ffmpeg loses the first second or two while it opens the display and the sound, so nothing happens
 * for a while; the clip is cut from the mark "start" (after the beacon, for a shot with sound). */
async function begin(ctx, region, { sound = false } = {}) {
  ctx.record({ region, crf: 18, preset: "ultrafast" });
  await sleep(2600);
  if (sound) await beacon(ctx);
  else mark(ctx, "start");
  await sleep(1500);
}

/** The five-stem Backing and the Vocal: the project most shots open. */
const BACKING = ["drums", "bass", "pad", "keys", "lead"];
const TWO = { "Backing.wav": BACKING, "Vocal.wav": "vocal" };
const THREE = { "Backing.wav": BACKING, "Drums.wav": "drums", "Vocal.wav": "vocal" };
const SIX = { "Drums.wav": "drums", "Bass.wav": "bass", "Pad.wav": "pad", "Keys.wav": "keys", "Lead.wav": "lead", "Vocal.wav": "vocal" };

const lane = (what) => `[data-testid='timeline-lane-${what}']`;
const laneReadout = (ctx, what, i) => ctx.d.exec((w, n) => document.querySelectorAll(`[data-testid='timeline-lane-${w}-readout']`)[n]?.textContent ?? "", what, i);

/** Edit a lane the way a person does with the keyboard: focus its slider, tap a key. Returns the new head. */
async function nudge(ctx, laneIndex, what, key) {
  const before = await head(ctx);
  await ctx.d.exec((sel, i) => document.querySelectorAll(sel)[i].focus(), lane(what), laneIndex);
  await ctx.d.keys(key);
  return waitForNewHead(ctx, before);
}

/** Mute lane `i`: one click on its Mute button. */
async function muteLane(ctx, i) {
  const before = await head(ctx);
  await ctx.d.exec((n) => document.querySelectorAll("[data-testid='timeline-lane-mute']")[n].click(), i);
  return waitForNewHead(ctx, before);
}

/** Ctrl+Z `times` times, each until the head has moved. */
async function undo(ctx, times) {
  for (let n = 0; n < times; n++) {
    const before = await head(ctx);
    await ctx.d.click("[data-testid='timeline-root']").catch(() => {});
    await ctx.d.keys(K.ctrl, "z");
    await ctx.d.until(async () => (await head(ctx)) !== before, { timeout: 10000, label: "undo to move the head" });
  }
}

/** A history with branches, made by editing lanes: three loads, two abandoned tries and a line of twelve
 * edits (20 nodes). Nothing here is on film; the Graph tab shows what was made. Lanes: 0 Backing, 1 Drums, 2 Vocal. */
async function makeHistory(ctx) {
  const { right, left } = K;
  const base = await head(ctx);
  // The first abandoned try: three edits, then undone.
  await nudge(ctx, 2, "gain", right);
  await nudge(ctx, 2, "gain", right);
  await nudge(ctx, 1, "pan", right);
  await undo(ctx, 3);
  assert((await head(ctx)) === base, "back at the loaded session");
  // The second: two edits.
  await nudge(ctx, 0, "gain", left);
  await nudge(ctx, 0, "gain", left);
  await undo(ctx, 2);
  assert((await head(ctx)) === base, "back at the loaded session again");
  // The line that stays.
  for (const [l, w, k] of [[0, "gain", right], [0, "gain", right], [2, "pan", right], [2, "gain", left], [1, "gain", right], [1, "pan", left], [2, "gain", left], [0, "pan", right], [1, "gain", right], [2, "pan", right], [0, "gain", right]]) {
    await nudge(ctx, l, w, k);
  }
  await muteLane(ctx, 1);
}

const showTab = async (ctx, id) => {
  await ctx.d.click(`[data-testid='tab-${id}']`);
};
const bubbles = (ctx) => ctx.d.count("[data-testid='graph-bubble']");

/** Scroll an element to `top` or `left` over `ms`, with the page's own scrolling, eased. */
function glideScroll(ctx, selector, to, ms, axis = "top") {
  return ctx.d.exec(
    (sel, target, dur, ax) =>
      new Promise((resolve) => {
        const el = document.querySelector(sel);
        const from = ax === "top" ? el.scrollTop : el.scrollLeft;
        const t0 = performance.now();
        const step = (now) => {
          const p = Math.min(1, (now - t0) / dur);
          const e = p < 0.5 ? 4 * p * p * p : 1 - (-2 * p + 2) ** 3 / 2;
          const v = from + (target - from) * e;
          if (ax === "top") el.scrollTop = v;
          else el.scrollLeft = v;
          p < 1 ? requestAnimationFrame(step) : resolve(v);
        };
        requestAnimationFrame(step);
      }),
    selector,
    to,
    ms,
    axis,
  );
}

/** The 200-track project's tracks, by the three loads that make it: the strings (12) come after 147 others, past
 * where the session block in the system prompt stops listing tracks. */
const SECTIONS_BEFORE = [["Flute", 8], ["Oboe", 6], ["Clarinet", 8], ["Bassoon", 6], ["Horn", 10], ["Trumpet", 8], ["Trombone", 8], ["Tuba", 4], ["Timpani", 4], ["Percussion", 16], ["Harp", 6], ["Piano", 6], ["Celesta", 2], ["Choir", 8], ["Synth Pad", 12], ["Synth Lead", 8], ["FX", 27]];
const SECTIONS_STRINGS = [["Violin I", 3], ["Violin II", 3], ["Viola", 2], ["Cello", 2], ["Contrabass", 2]];
const SECTIONS_AFTER = [["Drum Kit", 12], ["Vox", 10], ["Foley", 6], ["Sub", 4], ["Ambience", 9]];
const STRING_NAMES = SECTIONS_STRINGS.flatMap(([n, c]) => Array.from({ length: c }, (_, i) => `${n} ${"abc"[i]}`));

/** One folder of short mono files named after the tracks they make, each a copy of one of a few stems. */
function writeSection(ctx, dir, sections) {
  const folder = audioDir(ctx, dir);
  rmSync(folder, { recursive: true, force: true });
  const stemDir = audioDir(ctx, "stems");
  const stems = ["drums", "bass", "pad", "keys", "lead", "vocal"];
  if (!existsSync(stemDir)) writeStems(stemDir, Object.fromEntries(stems.map((n) => [`${n}.wav`, n])), 8, 22050);
  mkdirSync(folder, { recursive: true });
  let n = 0;
  const names = [];
  for (const [section, count] of sections) {
    for (let i = 0; i < count; i++) {
      const letters = STRING_NAMES.some((s) => s.startsWith(`${section} `)) || count <= 3;
      const name = letters ? `${section} ${"abcdefgh"[i]}` : `${section} ${i + 1}`;
      copyFileSync(join(stemDir, `${stems[n++ % stems.length]}.wav`), join(folder, `${name}.wav`));
      names.push(name);
    }
  }
  return { folder, names, paths: names.map((nm) => join(folder, `${nm}.wav`)) };
}

const stories = [
  {
    id: "9-promo-av-calibration",
    title: "Promo: how far the recorded sound runs ahead of the recorded picture (a flash and a tone sent together, four times)",
    async run(ctx) {
      const { d } = ctx;
      const { region } = await stageWindow(ctx);
      await onboard(ctx);
      const tone = join(ctx.out, "promo-audio", "tone.wav");
      mkdirSync(join(ctx.out, "promo-audio"), { recursive: true });
      writeFileSync(tone, wav({ seconds: 0.35, sample: (t) => Math.sin(2 * Math.PI * 1000 * t) * 0.6 }));
      await begin(ctx, region);
      const sent = [];
      for (let n = 1; n <= 4; n++) {
        // The page goes white and the tone starts, in the same breath.
        const player = spawn("paplay", ["--device=demo", tone], { stdio: "ignore" });
        await d.exec(() => { document.documentElement.style.filter = "invert(1)"; });
        sent.push(Date.now());
        mark(ctx, `flash ${n}`);
        await sleep(700);
        await d.exec(() => { document.documentElement.style.filter = ""; });
        if (player.exitCode === null) await new Promise((r) => player.on("exit", r));
        await sleep(1300);
      }
      saveNotes(ctx, "calibration", { sentAt: sent });
    },
  },
  {
    id: "9-promo-s1-describe-it-happens",
    title: "Promo S1: a plain-English request that real Claude turns into a fade and a gain change, then played",
    async run(ctx) {
      const { d } = ctx;
      const { region } = await stageWindow(ctx);
      await connect(ctx);
      await load(ctx, TWO);
      const request = "Fade the backing in over the first 4 seconds and bring the vocal down 3 dB";
      const before = await head(ctx);
      const startTracks = await tracks(ctx);
      assert(startTracks.length === 2 && startTracks[0].name === "Backing" && startTracks[1].name === "Vocal", `Backing and Vocal (${startTracks.map((t) => t.name).join(", ")})`);

      await begin(ctx, region, { sound: true });
      mark(ctx, "request");
      const turn = await ask(ctx, request, "S1", {
        stream: [
          { tool_calls: [{ name: "fade", arguments: { track: 0, kind: "in", range: { start_sec: 0, end_sec: 4 } } }, { name: "set_track_gain", arguments: { track: 1, db: -3 } }] },
          { text: "Done. The backing now fades in over its first 4 seconds, and the vocal is 3 dB quieter." },
        ],
        oneShot: [{ text: "edit" }],
      });
      mark(ctx, "done");
      await sleep(3000);

      const after = await tracks(ctx);
      const ok = okBadges(turn.badges);
      assert(ok.some((b) => /fade/i.test(b.text)), `a fade ran (${JSON.stringify(turn.badges)})`);
      assert(Math.abs(after[1].gain_db + 3) < 0.11, `the vocal is at -3 dB (${after[1].gain_db})`);
      assert(after[0].clips[0].source_path !== startTracks[0].clips[0].source_path, "the backing's audio is the faded copy");
      assert((await head(ctx)) !== before, "the head moved");
      const readouts = [await laneReadout(ctx, "gain", 0), await laneReadout(ctx, "gain", 1)];

      mark(ctx, "play");
      await play(ctx);
      mark(ctx, "playing");
      await sleep(7000);
      await pause(ctx);
      mark(ctx, "paused");
      await sleep(1500);
      saveNotes(ctx, "s1", { request, badges: turn.badges, reply: turn.reply, turnSeconds: turn.seconds, gains: after.map((t) => t.gain_db), readouts });
    },
  },
  {
    id: "9-promo-s2-faders-stay-put",
    title: "Promo S2: lane faders moved by key taps stay where they are put",
    async run(ctx) {
      const { d } = ctx;
      const { region } = await stageWindow(ctx);
      await onboard(ctx);
      await load(ctx, { "Bass.wav": "bass", "Drums.wav": "drums", "Keys.wav": "keys" });
      const gain = `${lane("gain")}`;
      // What the first lane's readout says, every time it changes, with when.
      await d.exec(() => {
        const el = document.querySelector("[data-testid='timeline-lane-gain-readout']");
        window.__readouts = [{ t: performance.now(), text: el.textContent }];
        new MutationObserver(() => window.__readouts.push({ t: performance.now(), text: el.textContent })).observe(el, { subtree: true, childList: true, characterData: true });
      });
      const start = await tracks(ctx);
      assert(start[0].gain_db === 0, "the first lane starts at 0 dB");

      await begin(ctx, region);
      // Take hold of the slider: a real click on its thumb, which leaves the value alone.
      const r = await d.rect(gain);
      mark(ctx, "slider");
      await pointAt(ctx, gain, { fx: (8 + (r.width - 16) * (60 / 84)) / r.width, fy: 0.5 });
      x("click", "1");
      await sleep(300);
      assert(await d.exec(() => document.activeElement?.getAttribute("data-testid") === "timeline-lane-gain"), "the slider has the keys");
      mark(ctx, "slider focused");
      await sleep(900);
      const taps = [];
      for (let n = 1; n <= 4; n++) {
        x("keydown", "Right");
        await sleep(rand(110, 170));
        x("keyup", "Right");
        taps.push(await d.exec(() => performance.now()));
        mark(ctx, `tap ${n}`);
        await sleep(rand(330, 430));
      }
      await sleep(2200);
      const history = await d.exec(() => window.__readouts);
      const values = history.map((h) => Number(h.text.replace(/[^\d.+-]/g, "")));
      assert(values.every((v, i) => i === 0 || v >= values[i - 1]), `the readout never went back: ${history.map((h) => h.text).join(", ")}`);
      assert((await laneReadout(ctx, "gain", 0)) === "+2.0 dB", `the readout holds +2.0 dB (${await laneReadout(ctx, "gain", 0)})`);
      assert(Math.abs((await tracks(ctx))[0].gain_db - 2) < 0.01, "the session holds +2.0 dB");
      mark(ctx, "held");
      await sleep(1200);
      saveNotes(ctx, "s2", { readouts: history, tapsAt: taps, gain: (await tracks(ctx))[0].gain_db });
    },
  },
  {
    id: "9-promo-s3-history-graph",
    title: "Promo S3: a version named, then the history compacted, both through the assistant, with the graph keeping up",
    async run(ctx) {
      const { d } = ctx;
      const { region } = await stageWindow(ctx);
      await connect(ctx);
      await load(ctx, THREE);
      await makeHistory(ctx);
      await showTab(ctx, "graph");
      await d.until(async () => (await bubbles(ctx)) >= 20, { timeout: 30000, label: "the graph to show its nodes" });
      const nodes0 = await bubbles(ctx);
      const headBefore = await head(ctx);

      await begin(ctx, region);
      mark(ctx, "request 1");
      const first = await ask(ctx, "Name this version chorus", "S3 name", {
        stream: [{ tool_calls: [{ name: "name_node", arguments: { node_id: await d.invoke("get_session_head"), label: "chorus" } }] }, { text: "Done. This version is now called chorus." }],
        oneShot: [{ text: "edit" }],
      });
      mark(ctx, "named");
      await sleep(3500);
      const labelled = await d.exec(() => [...document.querySelectorAll("[data-testid='graph-bubble']")].some((b) => /chorus/i.test(b.textContent)));
      assert(LIVE ? labelled : true, "a bubble in the graph reads chorus");
      assert((await head(ctx)) === headBefore, "naming moved no head");
      const nodes1 = await bubbles(ctx);

      mark(ctx, "request 2");
      const second = await ask(ctx, "Compact the history, keep the last 8 edits", "S3 compact", {
        stream: [{ tool_calls: [{ name: "compact_session", arguments: { keep_last: 8 } }] }, { text: "That would remove 12 nodes and keep the last 8. Shall I go ahead?" }],
        oneShot: [{ text: "edit" }],
      });
      mark(ctx, "asked");
      await sleep(3000);
      let nodes2 = await bubbles(ctx);
      // The tool only reports what it would remove until it is told to apply it, and a model that has been told so
      // asks first (a person would say yes). One answer, once.
      let third = null;
      if (nodes2 === nodes1) {
        mark(ctx, "request 3");
        third = await ask(ctx, "Yes, go ahead.", "S3 confirm", {
          stream: [{ tool_calls: [{ name: "compact_session", arguments: { keep_last: 8, apply: true } }] }, { text: "Done. The history keeps the last 8 edits." }],
          oneShot: [{ text: "edit" }],
        });
        mark(ctx, "compacted");
        await sleep(3500);
        nodes2 = await bubbles(ctx);
      }
      assert(nodes2 < nodes1, `nodes left the graph (${nodes1} to ${nodes2}; ${JSON.stringify((third ?? second).badges)})`);
      saveNotes(ctx, "s3", { nodesBefore: nodes0, nodesAfterName: nodes1, nodesAfterCompact: nodes2, first, second, third, labelled });
    },
  },
  {
    id: "9-promo-s4-200-tracks",
    title: "Promo S4: 200 tracks and 'make all the strings quieter', real Claude paging with list_session",
    async run(ctx) {
      const { d } = ctx;
      const { region } = await stageWindow(ctx);
      await connect(ctx);
      const parts = [writeSection(ctx, "s4-a", SECTIONS_BEFORE), writeSection(ctx, "s4-b", SECTIONS_STRINGS), writeSection(ctx, "s4-c", SECTIONS_AFTER)];
      let want = 0;
      for (const part of parts) {
        want += part.paths.length;
        await ctx.chooseThrough("[data-testid='open-audio-button']", { paths: part.paths, dir: part.folder, typeDelay: 10 }, { shot: false });
        await d.until(async () => (await d.count("[data-testid='timeline-lane']")) >= want, { timeout: 300000, label: `${want} lanes` });
      }
      const all = await tracks(ctx);
      assert(all.length === 200, `200 tracks (${all.length})`);
      const strings = all.map((t, i) => [t.name, i]).filter(([n]) => STRING_NAMES.includes(n)).map(([, i]) => i);
      assert(strings.length === 12 && strings[0] === 147 && strings[11] === 158, `the strings are tracks 148 to 159 (${strings.join(",")})`);
      await waitForWaveform(ctx);
      await sleep(1500);
      const request = "make all the strings quieter";

      await begin(ctx, region);
      // A look down the lanes, so the size of it shows, and back.
      mark(ctx, "look down");
      const body = "[data-testid='timeline-body']";
      await glideScroll(ctx, body, 9000, 1800);
      await sleep(150);
      await glideScroll(ctx, body, 0, 1300);
      await sleep(500);
      mark(ctx, "request");
      const turn = await ask(ctx, request, "S4", {
        stream: [
          { tool_calls: [{ name: "list_session", arguments: { what: "tracks", offset: 100, limit: 100 } }] },
          { tool_calls: strings.map((i) => ({ name: "set_track_gain", arguments: { track: i, db: -4 } })) },
          { text: "Done. All 12 string tracks are now 4 dB quieter." },
        ],
        oneShot: [{ text: "edit" }],
      });
      mark(ctx, "done");
      await sleep(2500);
      const after = await tracks(ctx);
      const lowered = strings.filter((i) => after[i].gain_db < -0.5);
      const others = after.filter((t, i) => !strings.includes(i) && Math.abs(t.gain_db) > 0.01).map((t) => `${t.name} ${t.gain_db}`);
      assert(lowered.length === 12, `all twelve strings are quieter (${lowered.length}; ${strings.map((i) => after[i].gain_db).join(", ")})`);
      assert(others.length === 0, `nothing else was touched (${others.join("; ")})`);
      assert(turn.badges.some((b) => /session|list/i.test(b.text)) || !LIVE, `the assistant paged with list_session (${JSON.stringify(turn.badges.slice(0, 4))})`);
      // Down to the strings.
      const top = await d.exec((i) => {
        const l = document.querySelectorAll("[data-testid='timeline-lane']")[i];
        const sc = document.querySelector("[data-testid='timeline-body']");
        return l.getBoundingClientRect().top - sc.getBoundingClientRect().top + sc.scrollTop;
      }, strings[0]);
      mark(ctx, "scroll to the strings");
      await glideScroll(ctx, body, top - 120, 2600);
      mark(ctx, "strings in view");
      await sleep(2800);
      const readouts = await d.exec(() => [...document.querySelectorAll("[data-testid='timeline-lane-gain-readout']")].map((e) => e.textContent).filter((t) => /-\d/.test(t)));
      saveNotes(ctx, "s4", { request, ...turn, strings, gains: strings.map((i) => after[i].gain_db), minus4InDom: readouts.length });
    },
  },
  {
    id: "9-promo-s5-runtime-bundled",
    title: "Promo S5: Settings shows the ONNX Runtime the installer carries",
    async run(ctx) {
      const { d } = ctx;
      const { region } = await stageWindow(ctx);
      await onboard(ctx);
      await load(ctx, TWO);
      await begin(ctx, region);
      mark(ctx, "gear");
      await press(ctx, "[data-testid='open-settings-button']");
      await d.waitFor("[data-testid='settings-card']");
      mark(ctx, "settings open");
      await sleep(1400);
      const line = "[data-testid='settings-ml-runtime']";
      const text = await d.text(line);
      assert(/ONNX Runtime: [\d.]+ \(bundled\)/.test(text), `the line says the runtime is bundled (${text})`);
      const detail = await d.text("[data-testid='settings-ml-runtime-detail']").catch(() => "");
      const over = await d.exec(() => {
        const b = document.querySelector("[data-testid='settings-body']");
        return b.scrollHeight - b.clientHeight;
      });
      if (over > 2) await glideScroll(ctx, "[data-testid='settings-body']", over, 1500);
      mark(ctx, "runtime line in view");
      await pointAt(ctx, line, { fx: 0.9 });
      await sleep(3600);
      saveNotes(ctx, "s5", { text, detail, scrolledPx: over });
    },
  },
  {
    id: "9-promo-s6-skills-note",
    title: "Promo S6: the Skills tab says bundled skills are updated unless edited",
    async run(ctx) {
      const { d } = ctx;
      const { region } = await stageWindow(ctx);
      await onboard(ctx);
      await load(ctx, TWO);
      await begin(ctx, region);
      await press(ctx, "[data-testid='open-settings-button']");
      await d.waitFor("[data-testid='settings-card']");
      await sleep(1200);
      mark(ctx, "skills tab");
      await press(ctx, "[data-testid='settings-tab-skills']");
      await d.waitFor("[data-testid='skills-bundled-note']");
      const note = await d.text("[data-testid='skills-bundled-note']");
      assert(/updated when a release changes them, unless you have edited them/i.test(note), `the note (${note})`);
      mark(ctx, "bundled note shown");
      const rows = await d.exec(() => [...document.querySelectorAll("[data-testid^='skills-row-']")].map((e) => e.getAttribute("data-testid").replace("skills-row-", "")));
      assert(rows.length === 8 && rows.includes("loudness-master"), `eight bundled skills (${rows.join(", ")})`);
      await sleep(900);
      await pointAt(ctx, "[data-testid='skills-bundled-note']");
      await sleep(2600);
      mark(ctx, "a skill opened");
      await press(ctx, "[data-testid='skills-row-loudness-master']");
      await d.until(async () => (await d.exec(() => document.querySelector("[data-testid='skills-name']")?.value)) === "loudness-master", { label: "the skill to open" });
      await sleep(2400);
      saveNotes(ctx, "s6", { note, skills: rows });
    },
  },
  {
    id: "9-promo-s7-mix-stays-playable",
    title: "Promo S7: the mix still plays after a tab switch with its cache file removed",
    async run(ctx) {
      const { d } = ctx;
      const { region } = await stageWindow(ctx);
      await onboard(ctx);
      await load(ctx, TWO);
      for (const [l, w, k] of [[0, "gain", K.right], [1, "gain", K.left], [0, "pan", K.right]]) await nudge(ctx, l, w, k);
      // The project's preview cache: the mix the window plays is a file in it.
      const previews = join(ctx.home, ".local/share/app.edytlab.desktop/default-project/.audiograph/previews");
      const mixes = () => (existsSync(previews) ? readdirSync(previews).filter((f) => f.endsWith(".wav")) : []);
      await begin(ctx, region, { sound: true });
      mark(ctx, "play");
      await play(ctx);
      mark(ctx, "playing");
      const names = await d.until(async () => (mixes().length ? mixes() : false), { label: "the mix's file" });
      await sleep(3300);
      mark(ctx, "graph tab");
      await showTab(ctx, "graph");
      await d.until(async () => (await bubbles(ctx)) >= 5, { timeout: 20000, label: "the graph" });
      assert(names.length === 1, `one mix is in the preview cache (${names.join(", ")})`);
      for (const n of names) rmSync(join(previews, n));
      assert(mixes().length === 0, "the mix's file is gone");
      await sleep(1800);
      mark(ctx, "timeline tab");
      await showTab(ctx, "timeline");
      await d.until(async () => mixes().length > 0, { timeout: 30000, label: "the mix to be rendered again" });
      await d.until(async () => (await d.count("[data-testid='timeline-mix-error']")) === 0 && !(await d.attr("[data-testid='play-pause-button']", "disabled")), { timeout: 30000, label: "the mix to load" });
      await sleep(900);
      mark(ctx, "play again");
      await play(ctx);
      mark(ctx, "playing again");
      await sleep(4200);
      await pause(ctx);
      mark(ctx, "paused");
      assert((await d.count("[data-testid='timeline-mix-error']")) === 0, "no 'could not load the mix'");
      await sleep(1000);
      saveNotes(ctx, "s7", { removed: names, renderedAgain: mixes() });
    },
  },
  {
    id: "9-promo-s8a-broll-timeline",
    title: "Promo S8a: b-roll, six tracks and five labels, zoomed and panned",
    async run(ctx) {
      const { d } = ctx;
      const { region } = await stageWindow(ctx);
      await onboard(ctx);
      await load(ctx, SIX);
      // Five labels, the way a person adds them: double-click the label lane, type a name, Enter.
      const track = "[data-testid='label-lane-track']";
      await d.exec((sel) => document.querySelector(sel).scrollIntoView({ block: "center" }), track);
      await sleep(400);
      const r = await d.rect(track);
      const names = ["intro", "verse", "chorus", "bridge", "outro"];
      for (const [i, name] of names.entries()) {
        await d.dblclick(track, -r.width / 2 + (r.width * (i * 0.2 + 0.02)), 0);
        await d.type("[data-testid='label-draft-input']", `${name}${K.enter}`);
        await d.until(async () => (await d.count("[data-testid='label-chip']")) === i + 1, { timeout: 10000, label: `label ${name}` });
      }
      // Back to the top of the lanes, with the pointer off the label lane (its hint shows on hover).
      await d.exec(() => { document.querySelector("[data-testid='timeline-body']").scrollTop = 0; });
      await rest(ctx);
      await sleep(900);
      await begin(ctx, region);
      mark(ctx, "zoom");
      await press(ctx, "[data-testid='zoom-in-btn']");
      await sleep(700);
      await press(ctx, "[data-testid='zoom-in-btn']");
      await sleep(900);
      mark(ctx, "pan");
      const hs = "[data-testid='timeline-hscroll']";
      const max = await d.exec((sel) => { const e = document.querySelector(sel); return e.scrollWidth - e.clientWidth; }, hs);
      assert(max > 400, `there is room to pan (${max} px)`);
      await rest(ctx);
      await glideScroll(ctx, hs, max, 7200, "left");
      mark(ctx, "lanes");
      await sleep(500);
      await glideScroll(ctx, "[data-testid='timeline-body']", 4000, 3600);
      await sleep(900);
      saveNotes(ctx, "s8a", { panPx: max, labels: names });
    },
  },
  {
    id: "9-promo-s8b-broll-graph",
    title: "Promo S8b: b-roll, the History graph with its branches, zoomed and dragged",
    async run(ctx) {
      const { d } = ctx;
      const { region } = await stageWindow(ctx);
      await onboard(ctx);
      await load(ctx, THREE);
      await makeHistory(ctx);
      await showTab(ctx, "graph");
      await d.until(async () => (await bubbles(ctx)) >= 20, { timeout: 30000, label: "the graph" });
      await begin(ctx, region);
      mark(ctx, "wheel");
      const root = "[data-testid='graph-view-root']";
      await pointAt(ctx, root, { fx: 0.5, fy: 0.5 });
      for (let i = 0; i < 3; i++) {
        x("click", "4");
        await sleep(180);
      }
      await sleep(900);
      mark(ctx, "drag");
      x("mousedown", "1");
      const o = await origin(ctx);
      const [px, py] = [Math.round(o.x + 600 * o.s), Math.round(o.y + 500 * o.s)];
      for (let i = 1; i <= 60; i++) {
        x("mousemove", String(px - Math.round((i / 60) * 260 * o.s)), String(py + Math.round((i / 60) * 220 * o.s)));
        await sleep(110);
      }
      x("mouseup", "1");
      await sleep(2200);
      saveNotes(ctx, "s8b", { nodes: await bubbles(ctx) });
    },
  },
];

export default process.env.PROMO ? stories.map((s) => ({ area: "Promo", ...s })) : [];
