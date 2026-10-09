// Runs every user story against the real desktop binary and writes
// screenshots, results.json and an HTML report to $OUT.
//
//   OUT=/tmp/edytlab-native node suite.mjs [story-id-substring ...]
//
// Each story group starts the app fresh (a new HOME, so a first launch),
// except where a story is about what survives a restart.
import { execFileSync, spawn } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { basename, join } from "node:path";
import { Driver } from "./webdriver.mjs";
import { chooseThrough, screenshotRoot } from "./native.mjs";
import { writeFixtures } from "./fixtures.mjs";
import { stories } from "./stories/index.mjs";

const APP = process.env.APP ?? "/home/user/edytlab/target/debug/edytlab-desktop";
const OUT = process.env.OUT ?? "/tmp/edytlab-native";
const LLM = "http://127.0.0.1:11434";
const only = process.argv.slice(2);
mkdirSync(join(OUT, "shots"), { recursive: true });
const fixtures = writeFixtures(join(OUT, "fixtures"));

function freshEnv(home, keepKeyring) {
  const env = { ...process.env, OUT };
  if (home) env.E2E_HOME = home;
  if (keepKeyring) env.KEEP_KEYRING = "1";
  const usedHome = execFileSync(new URL("./run-env.sh", import.meta.url).pathname, { env }).toString().trim();
  return { home: usedHome };
}

async function boot(home, keepKeyring) {
  const used = freshEnv(home, keepKeyring);
  const d = await Driver.start({ application: APP });
  await d.waitFor("[data-testid='status-bar']", { timeout: 30000 });
  return { d, ...used };
}

// LLM=real runs against a real model on :11434, which cannot be scripted:
// scripting is a no-op there and the request log is empty.
const REAL_LLM = process.env.LLM === "real";
async function script(turns) {
  if (REAL_LLM) return;
  await fetch(`${LLM}/__script`, { method: "POST", body: JSON.stringify(turns) });
}
async function llmRequests() {
  if (REAL_LLM) return [];
  return (await fetch(`${LLM}/__requests`)).json();
}

// RECORD=1: a story may call ctx.record() to film the screen from that
// point on (after onboarding, so no key field is ever on film). ffmpeg
// grabs the virtual display; the file lands in $OUT/videos.
function screenSize() {
  try {
    const out = execFileSync("xdpyinfo", ["-display", ":99"]).toString();
    return /dimensions:\s+(\d+x\d+)/.exec(out)?.[1] ?? "1440x900";
  } catch {
    return "1440x900";
  }
}

const results = [];
for (const story of stories) {
  if (only.length && !only.some((o) => story.id.includes(o))) continue;
  const result = { id: story.id, area: story.area, title: story.title, steps: [], status: "pass" };
  const started = Date.now();
  let session;
  let recorder = null;
  let recordedFrom = 0;
  try {
    session = await boot(story.home);
    const ctx = {
      d: session.d,
      home: session.home,
      fixtures,
      out: OUT,
      /** Click `selector`, then answer the native chooser it opens. */
      chooseThrough: (selector, answer, opts) =>
        chooseThrough(ctx.d, selector, answer, {
          shotDialog: opts?.shot === false ? undefined : () => ctx.shotNative(opts?.caption ?? "The native file chooser"),
        }),
      script,
      llmRequests,
      /** Film the screen from now until the story ends (RECORD=1 only).
       *
       * RECORD_AUDIO names a PulseAudio source (the monitor of the sink
       * the app plays into, such as `demo.monitor`) to record with it.
       * Both inputs then keep their wall-clock timestamps (`-copyts`):
       * ffmpeg otherwise starts each input at zero on its own, which put
       * the sound a second and a half ahead of the picture.
       * make-demo-videos.mjs lines them up. */
      record() {
        if (!process.env.RECORD || recorder) return;
        mkdirSync(join(OUT, "videos"), { recursive: true });
        const audio = process.env.RECORD_AUDIO;
        const file = `videos/${story.id}.${audio ? "mkv" : "mp4"}`;
        const args = ["-y", "-loglevel", "error"];
        if (audio) args.push("-copyts");
        args.push("-thread_queue_size", "1024", "-f", "x11grab", "-video_size", screenSize(), "-framerate", "15", "-i", ":99");
        if (audio) args.push("-thread_queue_size", "1024", "-f", "pulse", "-fragment_size", "3840", "-i", audio);
        args.push("-c:v", "libx264", "-preset", "veryfast", "-crf", audio ? "20" : "28", "-pix_fmt", "yuv420p");
        if (audio) args.push("-c:a", "pcm_s16le");
        args.push(join(OUT, file));
        recorder = spawn("ffmpeg", args, { stdio: ["pipe", "ignore", "ignore"] });
        result.video = file;
        result.captions = [];
        result.paces = [];
        recordedFrom = Date.now();
      },
      /** A step caption for the recording, at the current moment. `at` is
       * the wall clock, which a recording with audio is aligned on. */
      caption(text) {
        if (recorder) result.captions.push({ t: (Date.now() - recordedFrom) / 1000, at: Date.now(), text });
      },
      /** From now on, play the recording back at `speed`: 1 while there is
       * something to hear, faster while waiting on the model. */
      pace(speed) {
        if (recorder) result.paces.push({ t: (Date.now() - recordedFrom) / 1000, at: Date.now(), speed });
      },
      /** Quit and start again. `keepKeyring` keeps the keychain, as a
       * real restart does; HOME is kept unless another is given. */
      boot: async (home, { keepKeyring = true } = {}) => {
        await session.d.quit();
        session = await boot(home ?? session.home, keepKeyring);
        ctx.d = session.d;
        return session.d;
      },
      async shot(caption) {
        const n = String(result.steps.length + 1).padStart(2, "0");
        const file = `shots/${story.id}-${n}.png`;
        await ctx.d.screenshot(join(OUT, file));
        result.steps.push({ caption, file });
      },
      /** A screenshot of the whole screen, for native windows. */
      async shotNative(caption) {
        const n = String(result.steps.length + 1).padStart(2, "0");
        const file = `shots/${story.id}-${n}.png`;
        screenshotRoot(join(OUT, file));
        result.steps.push({ caption, file });
      },
      note(text) {
        result.steps.push({ caption: text });
      },
    };
    await story.run(ctx);
  } catch (e) {
    result.status = "fail";
    result.error = String(e.stack ?? e).slice(0, 1500);
    try {
      const file = `shots/${story.id}-fail.png`;
      await session?.d.screenshot(join(OUT, file));
      result.steps.push({ caption: "at failure", file });
    } catch {}
  } finally {
    if (recorder) {
      // A moment of the end state, then let ffmpeg finish the file.
      await new Promise((r) => setTimeout(r, 1500));
      const done = new Promise((r) => recorder.on("exit", r));
      recorder.stdin.write("q");
      recorder.stdin.end();
      await Promise.race([done, new Promise((r) => setTimeout(r, 10000))]);
    }
    await session?.d.quit();
  }
  result.ms = Date.now() - started;
  // A passing recording is kept aside with its captions: a later failing
  // run of the same story rewrites videos/<id>.mp4, and a demo that
  // worked must not be lost to one that did not.
  if (result.status === "pass" && result.video && existsSync(join(OUT, result.video))) {
    mkdirSync(join(OUT, "videos", "passed"), { recursive: true });
    copyFileSync(join(OUT, result.video), join(OUT, "videos", "passed", basename(result.video)));
    writeFileSync(join(OUT, "videos", "passed", `${story.id}.json`), JSON.stringify(result, null, 2));
  }
  results.push(result);
  console.log(`${result.status === "pass" ? "PASS" : "FAIL"} ${story.id} (${result.ms} ms)${result.error ? `\n  ${result.error.split("\n")[0]}` : ""}`);
}

// Accumulate across runs, keyed by story id: a run of a few stories updates
// those and keeps the rest, so the report covers everything run so far.
let previous = [];
try {
  previous = JSON.parse(readFileSync(join(OUT, "results.json"), "utf8"));
} catch {}
const ran = new Set(results.map((r) => r.id));
const merged = [...previous.filter((r) => !ran.has(r.id)), ...results];
writeFileSync(join(OUT, "results.json"), JSON.stringify(merged, null, 2));
const passed = results.filter((r) => r.status === "pass").length;
console.log(`\n${passed}/${results.length} stories passed this run (${merged.length} recorded)`);
