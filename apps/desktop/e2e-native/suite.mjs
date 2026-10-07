// Runs every user story against the real desktop binary and writes
// screenshots, results.json and an HTML report to $OUT.
//
//   OUT=/tmp/edytlab-native node suite.mjs [story-id-substring ...]
//
// Each story group starts the app fresh (a new HOME, so a first launch),
// except where a story is about what survives a restart.
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
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

function freshEnv(home, keyring) {
  const env = { ...process.env, OUT };
  if (home) env.E2E_HOME = home;
  if (keyring) env.E2E_KEYRING = keyring;
  const [usedHome, usedKeyring] = execFileSync(new URL("./run-env.sh", import.meta.url).pathname, { env })
    .toString()
    .trim()
    .split(/\s+/);
  return { home: usedHome, keyring: usedKeyring };
}

async function boot(home, keyring) {
  const used = freshEnv(home, keyring);
  const d = await Driver.start({ application: APP });
  await d.waitFor("[data-testid='status-bar']", { timeout: 30000 });
  return { d, ...used };
}

async function script(turns) {
  await fetch(`${LLM}/__script`, { method: "POST", body: JSON.stringify(turns) });
}
async function llmRequests() {
  return (await fetch(`${LLM}/__requests`)).json();
}

const results = [];
for (const story of stories) {
  if (only.length && !only.some((o) => story.id.includes(o))) continue;
  const result = { id: story.id, area: story.area, title: story.title, steps: [], status: "pass" };
  const started = Date.now();
  let session;
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
      /** Quit and start again. `keepKeyring` keeps the keychain, as a
       * real restart does; HOME is kept unless another is given. */
      boot: async (home, { keepKeyring = true } = {}) => {
        await session.d.quit();
        session = await boot(home ?? session.home, keepKeyring ? session.keyring : undefined);
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
    await session?.d.quit();
  }
  result.ms = Date.now() - started;
  results.push(result);
  console.log(`${result.status === "pass" ? "PASS" : "FAIL"} ${story.id} (${result.ms} ms)${result.error ? `\n  ${result.error.split("\n")[0]}` : ""}`);
}

writeFileSync(join(OUT, "results.json"), JSON.stringify(results, null, 2));
const passed = results.filter((r) => r.status === "pass").length;
console.log(`\n${passed}/${results.length} stories passed`);
