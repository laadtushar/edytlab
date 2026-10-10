// More of the assistant against real Claude, on the tool areas claude.mjs
// does not reach: EQ and dynamics, pitch, loudness targets, labels, reverse
// and repeat, FLAC and MP3 export, a plan with several kinds of step, and a
// request for something this build cannot do. Registered only when
// ANTHROPIC_E2E_KEY is set, like claude.mjs, and run the same way.
//
// Where a story can measure the result it measures the sound itself
// (stories/measure.mjs reads the rendered mix or a track's audio back), not
// what the model said. Tolerances are wide enough for the model to pick any
// reasonable settings and narrow enough that doing nothing, or the wrong
// thing, fails.
import { existsSync, mkdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { assert, head, waitForNewHead } from "./helpers.mjs";
import { toolBadges } from "./agent.mjs";
import { ask, askThrough, effectsOf, near, okBadges, ready, reply, setPlanFirst, tracks } from "./claude.mjs";
import { flacInfo, levelSpreadDb, looksLikeMp3, lufs, mono, peakHz, readWav, reversedError, rmsBetween, tiltDb } from "./measure.mjs";

const KEY = process.env.ANTHROPIC_E2E_KEY;

/** One turn: ask, wait, and return only the tool badges this turn added (the
 * chat keeps every badge from every turn). */
async function turn(ctx, text) {
  const seen = (await toolBadges(ctx)).length;
  return (await ask(ctx, text)).slice(seen);
}

const summary = (badges) => badges.map((b) => `${b.text}:${b.status}`).join(", ") || "none";
const ran = (badges, re) => okBadges(badges).some((b) => re.test(b.text));

/** The head's mix, rendered by the app and read back. A chain effect the
 * renderer cannot run fails here, and the error says which effects there were. */
async function headMix(ctx) {
  const node = await ctx.d.invoke("get_session_head");
  try {
    return readWav(await ctx.d.invoke("render_preview", { node }));
  } catch (e) {
    throw new Error(`the head's mix could not be rendered (effects on track 0: ${(await effectsOf(ctx, 0)).join(", ") || "none"}): ${e.message ?? e}`);
  }
}

/** Track `i`'s audio on the session's time axis, read back. */
async function trackWav(ctx, i = 0) {
  return readWav((await tracks(ctx))[i].audio_path);
}

/** Where the last clip on a track ends, in seconds. */
const trackEnd = (t) => Math.max(...t.clips.map((c) => c.start_sec + c.length_sec));

/** How bright and how uneven a take is: its spectral tilt (the top two
 * octaves against the two below, in dB) and the swing of its level. Neither
 * moves when a gain does. */
function voiceStats(wav) {
  const x = mono(wav);
  return { tilt: tiltDb(x, wav.rate), spread: levelSpreadDb(x, wav.rate) };
}

function exportsDir(ctx) {
  const dir = join(ctx.out, "exports");
  mkdirSync(dir, { recursive: true });
  return dir;
}

const stories = [
  {
    id: "7-claude-more-eq-dynamics",
    title: "'Make the vocal brighter and more even': an EQ and a compressor or leveler run, and the mix measures brighter with less level swing",
    async run(ctx) {
      await ready(ctx, ctx.fixtures.voice);
      const before = await head(ctx);
      const was = voiceStats(await headMix(ctx));
      ctx.note(`before: spectral tilt ${was.tilt.toFixed(1)} dB, level swing ${was.spread.toFixed(1)} dB`);
      await ctx.shot("A vocal take with a soft top end and uneven syllables");

      const badges = await turn(ctx, "Track 0 is a vocal. Make it brighter and more even in level.");
      ctx.note(`tools: ${summary(badges)}`);
      await waitForNewHead(ctx, before);
      // The EQ and the dynamics tools rewrite the audio; the effect chain
      // holds only gain, limiter and the filters. Either would count.
      const chain = await effectsOf(ctx, 0);
      ctx.note(`track 0 effect chain: ${chain.join(", ") || "empty"}`);
      assert(ran(badges, /equali[sz]er|\beq\b|shelf/i) || chain.some((k) => /^eq|shelf/i.test(k)), `an EQ ran (${summary(badges)})`);
      assert(ran(badges, /compressor|leveler/i) || chain.some((k) => /compress|level/i.test(k)), `a compressor or leveler ran (${summary(badges)})`);

      const now = voiceStats(await headMix(ctx));
      ctx.note(`after: spectral tilt ${now.tilt.toFixed(1)} dB (${(now.tilt - was.tilt).toFixed(1)} dB brighter), level swing ${now.spread.toFixed(1)} dB (${(was.spread - now.spread).toFixed(1)} dB less)`);
      await ctx.shot(`After: ${(now.tilt - was.tilt).toFixed(1)} dB brighter, level swing ${was.spread.toFixed(1)} to ${now.spread.toFixed(1)} dB`);
      assert(now.tilt - was.tilt >= 0.4, `the top end rose against the middle by at least 0.4 dB (${was.tilt.toFixed(2)} to ${now.tilt.toFixed(2)})`);
      assert(was.spread - now.spread >= 1, `the level swing fell by at least 1 dB (${was.spread.toFixed(2)} to ${now.spread.toFixed(2)})`);
    },
  },
  {
    id: "7-claude-more-pitch-shift",
    title: "Two semitones up, same length: the pitch tool runs, the track keeps its duration and its pitch measures 2 semitones higher",
    async run(ctx) {
      await ready(ctx, ctx.fixtures.tone);
      const before = await head(ctx);
      const was = await trackWav(ctx);
      const wasHz = peakHz(mono(was), was.rate, 300, 700);
      const wasEnd = trackEnd((await tracks(ctx))[0]);
      ctx.note(`before: ${wasHz.toFixed(1)} Hz, ${wasEnd.toFixed(2)} s`);

      const badges = await turn(ctx, "Raise the pitch of track 0 by 2 semitones, and keep it the same length.");
      ctx.note(`tools: ${summary(badges)}`);
      await waitForNewHead(ctx, before);
      assert(ran(badges, /pitch/i), `a pitch tool ran (${summary(badges)})`);

      const now = await trackWav(ctx);
      const hz = peakHz(mono(now), now.rate, 300, 700);
      const want = wasHz * 2 ** (2 / 12);
      const end = trackEnd((await tracks(ctx))[0]);
      ctx.note(`after: ${hz.toFixed(1)} Hz (2 semitones above ${wasHz.toFixed(1)} Hz is ${want.toFixed(1)} Hz), ${end.toFixed(2)} s`);
      await ctx.shot(`Pitch shifted: ${wasHz.toFixed(0)} Hz to ${hz.toFixed(0)} Hz, still ${end.toFixed(1)} s`);
      assert(Math.abs(hz - want) <= 8, `the pitch is 2 semitones up: ${hz.toFixed(1)} Hz, wanted ${want.toFixed(1)}`);
      assert(Math.abs(end - wasEnd) <= 0.15, `the length is unchanged: ${end} s, was ${wasEnd}`);
      assert((await tracks(ctx)).length === 1, "still one track");
    },
  },
  {
    id: "7-claude-more-podcast-loudness",
    title: "'-16 LUFS for a podcast': the rendered mix measures -16 LUFS integrated, within one LU",
    async run(ctx) {
      await ready(ctx, ctx.fixtures.voice);
      const before = await head(ctx);
      const was = lufs(await headMix(ctx));
      ctx.note(`before: ${was.toFixed(2)} LUFS`);
      await ctx.shot(`A voice take at ${was.toFixed(1)} LUFS`);

      const badges = await turn(ctx, "This is for a podcast feed. Set the loudness of track 0 to -16 LUFS integrated.");
      ctx.note(`tools: ${summary(badges)}`);
      await waitForNewHead(ctx, before);
      const mix = await headMix(ctx);
      const now = lufs(mix);
      const gain = (await tracks(ctx))[0].gain_db;
      ctx.note(`after: ${now.toFixed(2)} LUFS (track gain ${gain.toFixed(2)} dB; ${(-16 - was).toFixed(2)} dB was needed)`);
      await ctx.shot(`The mix now measures ${now.toFixed(1)} LUFS`);
      assert(Math.abs(now - -16) <= 1, `the mix is -16 LUFS within 1 LU (${now.toFixed(2)})`);
      assert(gain > 3, `the track was turned up (gain ${gain} dB)`);
      let peak = 0;
      for (const ch of mix.channels) for (const v of ch) peak = Math.max(peak, Math.abs(v));
      assert(peak < 0.999, `the louder mix does not clip (peak ${peak.toFixed(3)})`);
    },
  },
  {
    id: "7-claude-more-labels",
    title: "'Label the drop at 16 s': a marker and a region exist at the times I gave, and the label shows on the timeline",
    async run(ctx) {
      const { d } = ctx;
      await ready(ctx, ctx.fixtures.midnightDrive);
      const before = await head(ctx);
      assert((await d.invoke("list_markers")).length === 0, "no labels to begin with");
      const badges = await turn(ctx, "Mark the drop at 16 seconds with a marker called Drop, and label the last 8 seconds, from 24 s to 32 s, as a region called Outro.");
      ctx.note(`tools: ${summary(badges)}`);
      await waitForNewHead(ctx, before);

      const found = await d.invoke("list_markers");
      ctx.note(`labels in the session: ${found.map((m) => (m.kind === "marker" ? `${m.name} @ ${m.time_sec} s` : `${m.name} ${m.start_sec} to ${m.end_sec} s`)).join("; ")}`);
      const drop = found.find((m) => /drop/i.test(m.name));
      assert(drop && drop.kind === "marker" && near(drop.time_sec, 16, 0.1), `a marker called Drop at 16 s (${JSON.stringify(drop)})`);
      const outro = found.find((m) => /outro/i.test(m.name));
      assert(outro && outro.kind === "region" && near(outro.start_sec, 24, 0.1) && near(outro.end_sec, 32, 0.15), `a region called Outro from 24 s to 32 s (${JSON.stringify(outro)})`);

      const chips = () => d.exec(() => [...document.querySelectorAll("[data-testid='label-chip']")].map((e) => e.getAttribute("data-label-name")));
      await d.until(async () => (await chips()).some((n) => /drop/i.test(n)), { timeout: 15000, label: "the Drop label on the timeline" });
      await ctx.shot("The Drop marker and the Outro region, in the label lane");
    },
  },
  {
    id: "7-claude-more-reverse-repeat",
    title: "Loop a section, then reverse the track: the track is as long as the loop makes it, and the reversed audio is sample for sample the old audio backwards",
    async run(ctx) {
      await ready(ctx, ctx.fixtures.music);
      const was = trackEnd((await tracks(ctx))[0]);
      ctx.note(`before: ${was.toFixed(2)} s`);

      // 1. 1.5 s to 3.5 s is two seconds, with the near-silent second inside it.
      const first = await turn(ctx, "Loop the part of track 0 from 1.5 s to 3.5 s two more times.");
      ctx.note(`loop tools: ${summary(first)}`);
      const looped = trackEnd((await tracks(ctx))[0]);
      await ctx.shot(`Looped: ${was.toFixed(1)} s grew to ${looped.toFixed(1)} s`);
      assert(Math.abs(looped - (was + 4)) <= 0.15, `two more 2 s copies make the track ${was + 4} s (${looped})`);
      if (ran(first, /repeat/i) && !ran(first, /paste/i)) {
        // Repeat selection appends its copies, and each copy has its quiet second half way in.
        const w = await trackWav(ctx);
        const gap = rmsBetween(w, 8.6, 9.4);
        const loud = rmsBetween(w, 8.0, 8.4);
        ctx.note(`the first appended copy: ${loud.toFixed(3)} rms at its start, ${gap.toFixed(4)} where the quiet second falls`);
        assert(gap < 0.01 && loud > 0.05, `the appended copy has the section's shape (loud ${loud}, quiet ${gap})`);
      }

      // 2. Reverse all of it, and compare with what it was.
      const prior = await trackWav(ctx);
      const second = await turn(ctx, "Now reverse the whole of track 0.");
      ctx.note(`reverse tools: ${summary(second)}`);
      assert(ran(second, /reverse/i), `a reverse ran (${summary(second)})`);
      const now = await trackWav(ctx);
      const err = reversedError(prior, now);
      ctx.note(`reversed: ${now.seconds.toFixed(2)} s, greatest difference from the old audio read backwards ${err.toExponential(2)}`);
      await ctx.shot("Reversed: the decays now swell");
      assert(Math.abs(trackEnd((await tracks(ctx))[0]) - looped) <= 0.05, "reversing kept the length");
      assert(err < 0.001, `the audio is the old audio backwards (greatest difference ${err})`);
    },
  },
  {
    id: "7-claude-more-export-formats",
    title: "A FLAC and an MP3 at the names I gave: both files exist with the right extension, a FLAC or MP3 header, and sizes that fit",
    async run(ctx) {
      await ready(ctx, ctx.fixtures.music);
      const dir = exportsDir(ctx);
      const stamp = Date.now();
      const flac = join(dir, `night-bus-master-${stamp}.flac`);
      const mp3 = join(dir, `night-bus-preview-${stamp}.mp3`);
      const badges = await turn(ctx, `Export the current mix twice: as a lossless FLAC to ${flac}, and as a 128 kbps MP3 to ${mp3}.`);
      ctx.note(`tools: ${summary(badges)}`);
      await ctx.shot("Both exports done");

      assert(existsSync(flac), `the FLAC exists (${flac})`);
      assert(existsSync(mp3), `the MP3 exists (${mp3})`);
      const flacBytes = statSync(flac).size;
      const mp3Bytes = statSync(mp3).size;
      const info = flacInfo(flac);
      assert(info, "the .flac file starts with the FLAC marker");
      assert(info.rate === 44100 && info.channels === 2, `the FLAC is 44.1 kHz stereo (${JSON.stringify(info)})`);
      // The encoder may leave the length unrecorded; when it is there, it is the mix's 8 s.
      if (info.samples > 0) assert(Math.abs(info.seconds - 8) <= 0.1, `the FLAC runs 8 s (${info.seconds})`);
      const wavBytes = 8 * 44100 * 4;
      assert(flacBytes > 1000 && flacBytes < wavBytes, `the FLAC is smaller than the same audio as WAV (${flacBytes} bytes, WAV ${wavBytes})`);
      assert(looksLikeMp3(mp3), "the .mp3 file starts like an MP3");
      // 128 kbps for 8 s is 128 000 bytes; the band takes in tags and frame rounding.
      assert(mp3Bytes > 96000 && mp3Bytes < 170000, `the MP3 is about 128 kbps for 8 s (${mp3Bytes} bytes)`);
      ctx.note(`FLAC ${flacBytes} bytes (${info.samples > 0 ? `${info.seconds.toFixed(2)} s` : "length not recorded"}), MP3 ${mp3Bytes} bytes`);
      assert(okBadges(badges).filter((b) => /export/i.test(b.text)).length >= 2, `two exports ran (${summary(badges)})`);
    },
  },
  {
    id: "7-claude-more-plan-cleanup",
    title: "Plan first: a three-part clean-up waits for my approval, then the filter, the loudness change and the fade all happen",
    async run(ctx) {
      const { d } = ctx;
      await ready(ctx, ctx.fixtures.music);
      await setPlanFirst(ctx);
      const before = await head(ctx);
      let approvals = 0;
      const badges = await askThrough(ctx, "Clean up track 0 for a podcast: high-pass it at 80 Hz, normalize its loudness to -16 LUFS, then fade out the last 2 seconds.", "Clean-up", {
        press: async (c, sel) => {
          // The first card is the one that must hold everything back.
          if (approvals++ === 0) assert((await head(c)) === before, "nothing ran before the plan was approved");
          await c.d.click(sel);
        },
      });
      assert(approvals >= 1, "a plan card was shown");
      ctx.note(`approvals: ${approvals}; tools: ${summary(badges)}`);
      await waitForNewHead(ctx, before);

      const chain = await effectsOf(ctx, 0);
      const gain = (await tracks(ctx))[0].gain_db;
      ctx.note(`track 0: gain ${gain.toFixed(2)} dB, effect chain ${chain.join(", ") || "empty"}`);
      await ctx.shot("The plan was approved and run");
      assert(ran(badges, /high.?pass/i) || chain.some((k) => /high.?pass/i.test(k)), `a high-pass was applied (${summary(badges)}; chain ${chain.join(", ")})`);
      assert(ran(badges, /loudness/i), `loudness was normalized (${summary(badges)})`);
      assert(ran(badges, /fade/i), `the fade ran (${summary(badges)})`);
      // The track starts at about -10 LUFS, so reaching -16 turns it down.
      assert(gain < -2, `the loudness change turned the track down (gain ${gain} dB)`);
      assert((await d.count("[data-testid='chat-error']")) === 0, "no error in the chat");
    },
  },
  {
    id: "7-claude-more-stems-unavailable",
    title: "Asking for stems, which this build cannot make: nothing changes, Claude says so, and the next request still works",
    async run(ctx) {
      const { d } = ctx;
      await ready(ctx, ctx.fixtures.music);
      const before = await head(ctx);
      const badges = await turn(ctx, "Split track 0 into separate stems: vocals, drums, bass and everything else.");
      const text = await reply(ctx);
      ctx.note(`tools: ${summary(badges)}`);
      ctx.note(`Claude said: ${text.replace(/\s+/g, " ").slice(0, 260)}`);
      await ctx.shot("A request for stem separation");
      assert((await head(ctx)) === before, "the session did not change");
      assert((await tracks(ctx)).length === 1, "no stem tracks were added");
      assert(!ran(badges, /stem/i), `no stem tool succeeded (${summary(badges)})`);
      assert((await d.count("[data-testid='chat-error']")) === 0, "the chat did not break");
      // Either the tool reported that it cannot, or Claude did. The words are the model's; what is fixed is that one of them says no.
      const reported = badges.some((b) => /stem/i.test(b.text) && b.status !== "ok");
      const said = /unavailable|not (?:currently |yet )?(?:available|implemented|supported|possible)|n't (?:available|implemented|supported|possible)|\bcan(?:no|')t\b|\bunable\b|isn't|aren't|doesn't|not able/i.test(text);
      assert(reported || said, `the tool failed or Claude said it cannot (${summary(badges)}; "${text.slice(0, 160)}")`);

      await turn(ctx, "Then just make track 0 louder by 2 dB.");
      assert(near((await tracks(ctx))[0].gain_db, 2), `the next request worked (${(await tracks(ctx))[0].gain_db} dB)`);
      assert((await head(ctx)) !== before, "and edited");
      await ctx.shot("The next request worked");
    },
  },
];

export default KEY ? stories.map((s) => ({ area: "Assistant (Claude)", ...s })) : [];
