// The assistant against real Claude, through the app's own Anthropic
// provider. Registered only when ANTHROPIC_E2E_KEY is set; the app is
// pointed at claude-proxy.mjs (CLAUDE_PROXY), which counts spend and stops
// at a budget. Assertions are on outcomes the backend reports, never on
// the model's wording, because the wording is not deterministic.
import { existsSync, mkdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { assert, head, K, openAudio, sleep, waitForNewHead } from "./helpers.mjs";
import { say, toolBadges, waitForReply } from "./agent.mjs";

const KEY = process.env.ANTHROPIC_E2E_KEY;
const PROXY = process.env.CLAUDE_PROXY ?? "http://127.0.0.1:8788";
// The model and reasoning effort the stories run with. Unset, the app's own
// default applies, which is what a new user gets. Set, a run can compare
// models (CLAUDE_MODEL=claude-haiku-5-5 ./run-suite.sh 7-claude).
const MODEL = process.env.CLAUDE_MODEL || undefined;
const EFFORT = process.env.CLAUDE_EFFORT || undefined; // low | medium | high | xhigh | max
const TURN = 240000;

/** First launch: choose Anthropic and enter the key, as a person does.
 * The key field is a password field, and no screenshot is taken while
 * the dialog is open. */
export async function onboardClaude(ctx, { key = KEY, model, effort } = {}) {
  const { d } = ctx;
  await d.waitFor("[data-testid='settings']");
  await sleep(600);
  await d.click("[data-testid='settings-provider-anthropic']");
  await d.type("[data-testid='settings-key-input']", key);
  await d.type("[data-testid='settings-base-url-input']", PROXY);
  if (model) {
    await d.type("[data-testid='settings-model-input']", `${K.ctrl}a`);
    await d.type("[data-testid='settings-model-input']", model);
  }
  // Reasoning effort (low … max), where the build has the setting. An
  // older build without it fails loudly rather than recording at the
  // model's default effort while claiming another.
  if (effort) {
    const sel = "[data-testid='settings-effort-select']";
    assert((await d.count(sel)) > 0, `this build has no reasoning-effort setting (wanted ${effort})`);
    const ok = await d.exec(
      (s, v) => {
        const el = document.querySelector(s);
        if (![...el.options].some((o) => o.value === v)) return false;
        const set = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set;
        set.call(el, v);
        el.dispatchEvent(new Event("change", { bubbles: true }));
        return el.value === v;
      },
      sel,
      effort,
    );
    assert(ok, `reasoning effort ${effort} is not an option`);
  }
  await d.click("[data-testid='settings-save-button']");
}

async function ready(ctx, ...files) {
  const { d } = ctx;
  await onboardClaude(ctx, { model: MODEL, effort: EFFORT });
  ctx.note(`model: ${MODEL ?? "the app's default"}, effort: ${EFFORT ?? "the app's default"}`);
  await d.until(async () => (await d.count("[data-testid='settings']")) === 0, { timeout: 60000, label: "the welcome to close" });
  assert((await d.invoke("get_active_provider")) === "anthropic", "Anthropic is the active provider");
  if (files.length) await openAudio(ctx, ...files);
  ctx.record();
}

async function ask(ctx, text) {
  await say(ctx, text);
  await waitForReply(ctx, { timeout: TURN });
  return toolBadges(ctx);
}

export const tracks = (ctx) => ctx.d.invoke("list_tracks");

/** The effect kinds on `track` at the head, read from the session itself. */
export async function effectsOf(ctx, track) {
  const id = await ctx.d.invoke("get_session_head");
  const node = await ctx.d.invoke("get_node", { id });
  return (node.state.tracks[track]?.effects ?? []).map((e) => e.kind);
}

/** A low-pass was applied to `track`: as an effect in its chain, or as a
 * destructive filter tool (both are a valid answer to "add a low-pass"). */
export async function lowPassed(ctx, track, badges) {
  const fx = await effectsOf(ctx, track);
  return fx.some((k) => /low.?pass/i.test(k)) || okBadges(badges).some((b) => /low.?pass/i.test(b.text));
}
export const near = (a, b, tol = 0.11) => Math.abs(a - b) <= tol;
export const okBadges = (badges) => badges.filter((b) => b.status === "ok");
export const failed = (badges) => badges.filter((b) => b.status !== "ok");
export const reply = (ctx) =>
  ctx.d.exec(() =>
    [...document.querySelectorAll("[data-testid='message-bubble'][data-role='assistant']")].map((e) => e.textContent).join("\n"),
  );

/** Send, then wait for the whole turn: the box is disabled until the
 * turn ends, plan approval included. A plan is approved as it appears. */
export async function askThrough(ctx, text, label, { send = say, press = (c, sel) => c.d.click(sel) } = {}) {
  const { d } = ctx;
  await send(ctx, text);
  await d.until(async () => d.exec(() => document.querySelector("[data-testid='chat-form'] textarea").disabled), {
    timeout: 20000,
    label: `${label}: the turn to start`,
  }).catch(() => {});
  const end = Date.now() + TURN * 2;
  while (Date.now() < end) {
    if (await d.count("[data-testid='plan-approval-card']")) {
      const steps = await d.exec(() => document.querySelector("[data-testid='plan-approval-card'] ol")?.innerText ?? "");
      ctx.note(`${label}: Claude proposed a plan: ${steps.replace(/\s+/g, " ").slice(0, 400)}`);
      await ctx.shot(`${label}: Claude's plan`);
      await press(ctx, "[data-testid='plan-run-button']");
    }
    const busy = await d.exec(() => document.querySelector("[data-testid='chat-form'] textarea").disabled);
    if (!busy && !(await d.count("[data-testid='plan-approval-card']"))) break;
    await sleep(1000);
  }
  await sleep(1500);
  if (await d.count("[data-testid='chat-error']")) {
    throw new Error(`${label}: the chat showed an error: ${await d.text("[data-testid='chat-error']")}`);
  }
  return toolBadges(ctx);
}

/** Every number in `text`. */
export const numbers = (text) => [...text.matchAll(/-?\d+(?:\.\d+)?/g)].map((m) => Number(m[0]));

/** A WAV file's length in seconds, from its header. */
export function wavSeconds(bytes) {
  const channels = bytes.readUInt16LE(22);
  const rate = bytes.readUInt32LE(24);
  const bits = bytes.readUInt16LE(34);
  let at = 12;
  while (at < bytes.length - 8) {
    const id = bytes.subarray(at, at + 4).toString();
    const size = bytes.readUInt32LE(at + 4);
    if (id === "data") return size / (rate * channels * (bits / 8));
    at += 8 + size + (size % 2);
  }
  return 0;
}

async function setPlanFirst(ctx) {
  await ctx.d.click("[data-testid='plan-first-toggle']");
  await ctx.d.until(async () => (await ctx.d.attr("[data-testid='plan-first-toggle']", "aria-pressed")) === "true", { label: "Plan first on" });
}

const stories = [
  {
    id: "7-claude-connection",
    title: "With an Anthropic key, the connection test passes and Claude's models are listed",
    async run(ctx) {
      const { d } = ctx;
      await ready(ctx);
      await d.click("[data-testid='open-settings-button']");
      await d.waitFor("[data-testid='settings']");
      // The panel does not show a stored key back, so the test needs it
      // typed again (the field is a password field).
      await d.type("[data-testid='settings-key-input']", KEY);
      await d.click("[data-testid='settings-test-button']");
      const outcome = await d.until(async () => {
        for (const id of ["settings-test-ok", "settings-test-no-tools", "settings-test-error"]) {
          if (await d.count(`[data-testid='${id}']`)) return id;
        }
        return false;
      }, { timeout: 60000, label: "the connection test" });
      assert(outcome === "settings-test-ok", `the test said ${outcome}: ${await d.text(`[data-testid='${outcome}']`)}`);
      const models = await d.exec(() => [...document.querySelectorAll("[data-testid='settings-model-datalist'] option")].map((o) => o.value));
      ctx.note(`models offered: ${models.slice(0, 8).join(", ")}${models.length > 8 ? ", …" : ""}`);
      assert(models.some((m) => /claude/i.test(m)), "Claude models are listed");
      await ctx.shot("Settings with Anthropic: the connection test passed (key field masked)");
    },
  },
  {
    id: "7-claude-wrong-key",
    title: "A wrong key is saved without a check, and the first message says the key is the problem and offers Settings",
    async run(ctx) {
      const { d } = ctx;
      await onboardClaude(ctx, { key: "sk-ant-api03-this-key-is-not-real-000000000000000000000000" });
      await d.until(async () => (await d.count("[data-testid='settings']")) === 0, { timeout: 60000, label: "the welcome to close" });
      ctx.note("Save accepted the key without checking it");
      await openAudio(ctx, ctx.fixtures.tone);
      await say(ctx, "Make track 0 louder by 3 dB.");
      await d.waitFor("[data-testid='chat-error']", { timeout: 60000 });
      const text = await d.text("[data-testid='chat-error']");
      ctx.note(`chat error: ${text.slice(0, 220)}`);
      await ctx.shot("The first message with a wrong key");
      assert(/401|auth|api.?key/i.test(text), `the error points at the key (${text.slice(0, 160)})`);
      assert((await d.count("[data-testid='thinking-indicator']")) === 0, "not stuck thinking");
      assert((await d.count("[data-testid='chat-error-open-settings']")) === 1, `an Open Settings button is offered (error: ${text.slice(0, 160)})`);
    },
  },
  {
    id: "7-claude-louder-undo-redo",
    title: "I ask Claude for 6 dB more; the track gets it, undo takes it back and redo restores it",
    async run(ctx) {
      const { d } = ctx;
      await ready(ctx, ctx.fixtures.music);
      const before = await head(ctx);
      const badges = await ask(ctx, "Make track 0 louder by 6 dB.");
      await ctx.shot("Claude made the track 6 dB louder");
      assert(okBadges(badges).length >= 1 && failed(badges).length === 0, `the tools succeeded: ${JSON.stringify(badges)}`);
      const after = await waitForNewHead(ctx, before);
      assert(near((await tracks(ctx))[0].gain_db, 6), `gain is +6 dB (${(await tracks(ctx))[0].gain_db})`);
      await d.click("[data-testid='timeline-root']").catch(() => {});
      await d.keys(K.ctrl, "z");
      await d.until(async () => (await head(ctx)) === before, { label: "undo to the head before" });
      assert(near((await tracks(ctx))[0].gain_db, 0), "undo restored 0 dB");
      await ctx.shot("Ctrl+Z: back to 0 dB");
      await d.keys(K.ctrl, K.shift, "z");
      await d.until(async () => (await head(ctx)) === after, { label: "redo to the edited head" });
      assert(near((await tracks(ctx))[0].gain_db, 6), "redo restored +6 dB");
      await ctx.shot("Ctrl+Shift+Z: +6 dB again");
    },
  },
  {
    id: "7-claude-question",
    title: "A question gets an answer and changes nothing",
    async run(ctx) {
      await ready(ctx, ctx.fixtures.tone);
      const before = await head(ctx);
      const badges = await ask(ctx, "In one sentence, what kinds of edits can you make for me? Don't change anything.");
      const text = await reply(ctx);
      await ctx.shot("A plain answer");
      assert(text.trim().length > 20, "a real answer");
      assert((await head(ctx)) === before, `nothing was edited (badges: ${JSON.stringify(badges)})`);
    },
  },
  {
    id: "7-claude-fade-out",
    title: "Fade out the last two seconds",
    async run(ctx) {
      await ready(ctx, ctx.fixtures.music);
      const before = await head(ctx);
      const badges = await ask(ctx, "Fade out the last 2 seconds of track 0.");
      await ctx.shot("Faded out");
      assert(okBadges(badges).some((b) => /fade/i.test(b.text)), `a fade ran: ${JSON.stringify(badges)}`);
      await waitForNewHead(ctx, before);
    },
  },
  {
    id: "7-claude-selection-silence",
    title: "With a region selected, 'silence this' silences that region",
    async run(ctx) {
      const { d } = ctx;
      await ready(ctx, ctx.fixtures.music);
      const r = await d.rect("[data-testid='timeline-lane-surface']");
      await d.drag("[data-testid='timeline-lane-surface']", -r.width / 4, 0, r.width / 8, 0);
      await d.waitFor("[data-testid='status-bar-selection']");
      const sel = await d.text("[data-testid='status-bar-selection']");
      await ctx.shot(`A region selected: ${sel}`);
      const before = await head(ctx);
      const badges = await ask(ctx, "Silence this.");
      await ctx.shot("The selected region silenced");
      assert(okBadges(badges).some((b) => /silence|gain|volume|cut/i.test(b.text)), `a silencing tool ran: ${JSON.stringify(badges)}`);
      await waitForNewHead(ctx, before);
    },
  },
  {
    id: "7-claude-multi-step",
    title: "One request, two edits: normalize, then fade in",
    async run(ctx) {
      await ready(ctx, ctx.fixtures.music);
      const before = await head(ctx);
      const badges = await ask(ctx, "Normalize track 0 to -1 dBFS peak, then fade in its first second.");
      await ctx.shot("Normalized, then faded in");
      const ok = okBadges(badges);
      assert(ok.some((b) => /normal/i.test(b.text)), `normalize ran: ${JSON.stringify(badges)}`);
      assert(ok.some((b) => /fade/i.test(b.text)), `the fade ran: ${JSON.stringify(badges)}`);
      await waitForNewHead(ctx, before);
    },
  },
  {
    id: "7-claude-mute-rename",
    title: "Mute a track and rename it in one sentence",
    async run(ctx) {
      const { d } = ctx;
      await ready(ctx, ctx.fixtures.music);
      await ask(ctx, "Mute track 0 and rename it to Drums.");
      await d.until(async () => {
        const t = (await tracks(ctx))[0];
        return t.muted === true && t.name === "Drums";
      }, { label: "track 0 muted and named Drums", timeout: 20000 });
      await ctx.shot("Muted and renamed to Drums");
    },
  },
  {
    id: "7-claude-two-tracks",
    title: "Two tracks: pan one hard left and turn the other down",
    async run(ctx) {
      const { d } = ctx;
      await ready(ctx, ctx.fixtures.tone, ctx.fixtures.music);
      await ask(ctx, "Pan track 1 fully left, and lower track 0 by 3 dB.");
      await d.until(async () => {
        const t = await tracks(ctx);
        return t[1].pan <= -0.95 && near(t[0].gain_db, -3);
      }, { label: "track 1 panned left and track 0 at -3 dB", timeout: 20000 });
      await ctx.shot("Two tracks: one panned left, one 3 dB down");
    },
  },
  {
    id: "7-claude-follow-up",
    title: "A follow-up refers back: 'do that again'",
    async run(ctx) {
      await ready(ctx, ctx.fixtures.music);
      await ask(ctx, "Make track 0 louder by 3 dB.");
      assert(near((await tracks(ctx))[0].gain_db, 3), `first turn: +3 dB (${(await tracks(ctx))[0].gain_db})`);
      await ask(ctx, "Do that again.");
      await ctx.shot("After 'do that again': +6 dB");
      assert(near((await tracks(ctx))[0].gain_db, 6), `second turn: +6 dB in total (${(await tracks(ctx))[0].gain_db})`);
    },
  },
  {
    id: "7-claude-plan-run",
    title: "Plan first: Claude shows its steps, I run them, and they happen",
    async run(ctx) {
      const { d } = ctx;
      await ready(ctx, ctx.fixtures.music);
      await setPlanFirst(ctx);
      const before = await head(ctx);
      await say(ctx, "Make track 0 louder by 3 dB, then fade out its last second.");
      await d.waitFor("[data-testid='plan-approval-card']", { timeout: TURN }).catch(async () => {
        await ctx.shot("BUG: no plan card");
        throw new Error(`no plan card; the chat says: ${(await reply(ctx)).slice(0, 200)}`);
      });
      const steps = await d.exec(() => document.querySelector("[data-testid='plan-approval-card'] ol")?.innerText ?? "");
      ctx.note(`the plan: ${steps.replace(/\s+/g, " ").slice(0, 300)}`);
      assert((await head(ctx)) === before, "nothing ran before approval");
      await ctx.shot("Claude's plan, waiting for approval");
      await d.click("[data-testid='plan-run-button']");
      await waitForReply(ctx, { timeout: TURN });
      await ctx.shot("Approved and run");
      assert(near((await tracks(ctx))[0].gain_db, 3), `+3 dB after the plan (${(await tracks(ctx))[0].gain_db})`);
      assert(okBadges(await toolBadges(ctx)).some((b) => /fade/i.test(b.text)), "the fade ran");
    },
  },
  {
    id: "7-claude-plan-discard",
    title: "Plan first: discarding Claude's plan runs nothing",
    async run(ctx) {
      const { d } = ctx;
      await ready(ctx, ctx.fixtures.music);
      await setPlanFirst(ctx);
      const before = await head(ctx);
      await say(ctx, "Reverse the whole of track 0.");
      await d.waitFor("[data-testid='plan-approval-card']", { timeout: TURN });
      await ctx.shot("The plan, before discarding");
      await d.click("[data-testid='plan-discard-button']");
      await d.until(async () => (await d.count("[data-testid='plan-approval-card']")) === 0, { label: "the card to go" });
      await sleep(3000);
      assert((await head(ctx)) === before, "discarding ran nothing");
      assert((await d.count("[data-testid='chat-error']")) === 0, "no error");
      await ctx.shot("Discarded: nothing changed");
    },
  },
  {
    id: "7-claude-analyze",
    title: "Asking how loud a track is measures it and answers with a number, without editing",
    async run(ctx) {
      await ready(ctx, ctx.fixtures.music);
      const before = await head(ctx);
      const badges = await ask(ctx, "How loud is track 0? Tell me its integrated loudness in LUFS. Don't change anything.");
      const text = await reply(ctx);
      await ctx.shot("The measured loudness");
      ctx.note(`tools: ${badges.map((b) => `${b.text}:${b.status}`).join(", ")}`);
      assert(/-?\d+(\.\d+)?\s*LUFS/i.test(text), `the answer has a LUFS figure (${text.slice(0, 160)})`);
      assert((await head(ctx)) === before, "nothing was edited");
    },
  },
  {
    id: "7-claude-export",
    title: "Asking for a WAV export writes a real WAV file where I asked",
    async run(ctx) {
      await ready(ctx, ctx.fixtures.music);
      const dir = join(ctx.out, "exports");
      mkdirSync(dir, { recursive: true });
      const file = join(dir, `claude-export-${Date.now()}.wav`);
      await ask(ctx, `Export the current mix as a WAV file to ${file}`);
      await ctx.shot("Exported");
      assert(existsSync(file), `the file exists (${file})`);
      const bytes = readFileSync(file);
      assert(bytes.subarray(0, 4).toString() === "RIFF" && bytes.subarray(8, 12).toString() === "WAVE", "it is a WAV file");
      ctx.note(`exported ${bytes.length} bytes`);
    },
  },
  {
    id: "7-claude-impossible-then-ok",
    title: "A request that cannot work fails cleanly, and the next one still works",
    async run(ctx) {
      const { d } = ctx;
      await ready(ctx, ctx.fixtures.music);
      const before = await head(ctx);
      const badges = await ask(ctx, "Load the file /definitely/not/here/song.wav as a new track.");
      await ctx.shot("A file that does not exist");
      ctx.note(`tools: ${badges.map((b) => `${b.text}:${b.status}`).join(", ")}`);
      assert((await tracks(ctx)).length === 1, "no track was added");
      assert((await d.count("[data-testid='chat-error']")) === 0, "the chat did not break");
      await ask(ctx, "Make track 0 louder by 2 dB.");
      assert(near((await tracks(ctx))[0].gain_db, 2), `the next request worked (${(await tracks(ctx))[0].gain_db})`);
      assert((await head(ctx)) !== before, "and edited");
      await ctx.shot("The next request worked");
    },
  },
  {
    id: "7-claude-tool-switched-off",
    title: "With fade switched off in the capabilities menu, Claude cannot fade",
    async run(ctx) {
      const { d } = ctx;
      await ready(ctx, ctx.fixtures.music);
      await d.click("[data-testid='capabilities-toggle']");
      await d.waitFor("[aria-label='Enable fade']");
      await d.click("[aria-label='Enable fade']");
      await d.until(async () => (await d.exec(() => document.querySelector("[aria-label='Enable fade']").checked)) === false, { label: "fade off" });
      await ctx.shot("fade switched off");
      await d.click("[data-testid='capabilities-toggle']");
      const badges = await ask(ctx, "Fade out the last 2 seconds of track 0.");
      await ctx.shot("Claude's answer without the fade tool");
      ctx.note(`tools: ${badges.map((b) => `${b.text}:${b.status}`).join(", ") || "none"}`);
      assert(!okBadges(badges).some((b) => /^fade/i.test(b.text)), "no fade ran");
    },
  },
  {
    id: "7-claude-dj-transition",
    title: "A DJ transition, over five turns: tempos, time-stretch, overlap, crossfade with a filter sweep, then a loudness-normalized export",
    async run(ctx) {
      const { d } = ctx;
      await ready(ctx, ctx.fixtures.djA, ctx.fixtures.djB);
      await ctx.shot("Two dance loops: track 0 at 120 BPM, track 1 at 128 BPM");
      const used = [];
      const track = async (i) => (await tracks(ctx))[i];

      // 1. What am I working with?
      const before = await head(ctx);
      used.push(...(await askThrough(ctx, "I'm building a DJ transition from track 0 into track 1. What tempo is each track? Don't change anything yet.", "Tempos")));
      const told = numbers(await reply(ctx));
      ctx.note(`numbers in the answer: ${told.join(", ")}`);
      await ctx.shot("Claude measured both tempos");
      assert(told.some((n) => Math.abs(n - 120) <= 2), "it found track 0's 120 BPM");
      assert(told.some((n) => Math.abs(n - 128) <= 2), "it found track 1's 128 BPM");
      assert((await head(ctx)) === before, "measuring changed nothing");

      // 2. Beatmatch.
      const lengthBefore = (await track(1)).clips[0].length_sec;
      used.push(...(await askThrough(ctx, "Time-stretch track 1 so its tempo matches track 0 exactly, keeping its pitch.", "Beatmatch")));
      const stretched = (await track(1)).clips[0].length_sec;
      ctx.note(`track 1 length: ${lengthBefore.toFixed(2)} s to ${stretched.toFixed(2)} s (128 to 120 BPM means x${(128 / 120).toFixed(3)})`);
      await ctx.shot(`Track 1 stretched to 120 BPM: ${stretched.toFixed(2)} s long`);
      assert(Math.abs(stretched - lengthBefore * (128 / 120)) < 0.35, `track 1 is ${(128 / 120).toFixed(3)}x longer (${stretched})`);

      // 3. Overlap.
      used.push(...(await askThrough(ctx, "Now move track 1 so it starts at 8 seconds, overlapping the second half of track 0.", "Overlap")));
      const start = (await track(1)).clips[0].start_sec;
      ctx.note(`track 1 starts at ${start.toFixed(3)} s`);
      await ctx.shot("Track 1 starts at 8 s, under the second half of track 0");
      assert(Math.abs(start - 8) < 0.06, `track 1 starts at 8 s (${start})`);

      // 4. The transition itself.
      used.push(
        ...(await askThrough(
          ctx,
          "Make the transition: fade track 0 out from 8 seconds to its end, fade track 1 in over its first 8 seconds, and put a low-pass filter on track 0 from 8 seconds to its end so the old track loses its top end as it leaves.",
          "Transition",
        )),
      );
      await ctx.shot("The crossfade and the filter sweep");
      const names = used.filter((b) => b.status === "ok").map((b) => b.text.toLowerCase());
      assert(names.filter((n) => n.includes("fade")).length >= 2, `two fades ran (${names.join(", ")})`);
      const fx = await effectsOf(ctx, 0);
      ctx.note(`track 0 effects: ${fx.join(", ") || "none"}`);
      assert(await lowPassed(ctx, 0, used), `track 0 has a low-pass (effects: ${fx.join(", ")}; tools: ${names.join(", ")})`);

      // 5. Master and export.
      const dir = join(ctx.out, "exports");
      mkdirSync(dir, { recursive: true });
      const file = join(dir, `dj-transition-${Date.now()}.wav`);
      used.push(...(await askThrough(ctx, `Normalize the whole mix to -14 LUFS integrated, then export it as a WAV file to ${file}`, "Master")));
      await ctx.shot("Normalized to -14 LUFS and exported");
      assert(existsSync(file), `the export exists (${file})`);
      const bytes = readFileSync(file);
      const seconds = wavSeconds(bytes);
      const expected = 8 + stretched;
      ctx.note(`exported ${(bytes.length / 1e6).toFixed(1)} MB, ${seconds.toFixed(2)} s (expected about ${expected.toFixed(2)} s)`);
      assert(Math.abs(seconds - expected) < 0.6, `the export runs from 0 to the end of track 1 (${seconds} s, expected ${expected})`);

      // The whole session is history: one undo steps back from the export.
      ctx.note(`every tool Claude ran: ${used.map((b) => `${b.text} (${b.status})`).join(", ")}`);
      const failedTools = used.filter((b) => b.status !== "ok");
      if (failedTools.length) ctx.note(`tool calls that failed and were retried: ${failedTools.map((b) => b.text).join(", ")}`);
      await d.click("[data-testid='graph-tab']").catch(() => {});
      await sleep(1500);
      await ctx.shot("The session's history after the whole transition");
    },
  },
];

export default KEY ? stories.map((s) => ({ area: "Assistant (Claude)", ...s })) : [];
