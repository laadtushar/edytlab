/**
 * The app screenshots the LinkedIn visuals are built from.
 *
 * The real frontend, from the production bundle, in Chromium, with the
 * Rust side faked at the IPC boundary by `apps/desktop/e2e/backend.ts`,
 * exactly as the CI specs do it. Everything drawn is the app's own
 * markup and stylesheet; what is staged is the data: a two-track
 * session, a chat, a history graph. So the screenshots show the real
 * interface with sample content, and the README and the slides say so.
 *
 * The audio is `loops.ts`: two synthetic 16-bar loops, written from
 * arithmetic into the harness's fixture folder.
 *
 * Run through `render.mjs --shots`, or see README.md. Writes PNGs into
 * `shots/`; the templates reference them from there.
 */

import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { FIXTURE_DIR } from "../../../apps/desktop/e2e/audio-fixtures";
import {
  DURABLE_SAVE,
  deferred,
  nodeId,
  ok,
  placedTrack,
  projectWith,
  readyToLoad,
  trackFor,
} from "../../../apps/desktop/e2e/backend";
import { expect, test, type App } from "../../../apps/desktop/e2e/fixtures";
import { writeLoops } from "./loops";

const SHOTS = process.env.SHOTS_OUT ?? fileURLToPath(new URL("./shots/", import.meta.url));
mkdirSync(SHOTS, { recursive: true });

// 1280 wide, as Tauri opens the window; 840 tall (it opens 800), so the
// whole conversation fits in the chat without scrolling.
test.use({ viewport: { width: 1280, height: 840 } });

const ID_A = "11111111-1111-4111-8111-111111111111";
const ID_B = "22222222-2222-4222-8222-222222222222";

/** The two-track session: B placed 8 bars (16 s) before A ends, crossfaded. */
function session() {
  const loops = writeLoops(join(FIXTURE_DIR, "linkedin"), { a: 0, b: 16 });
  const a = trackFor(loops.a.lane, loops.a.lengthSec, { name: "Track A", id: ID_A });
  a.clips[0].volume_envelope = [
    { time_sec: 0, gain_db: 0 },
    { time_sec: 16, gain_db: 0 },
    { time_sec: 32, gain_db: -48 },
  ];
  const b = placedTrack({
    source: loops.b.source,
    startSec: 16,
    lengthSec: loops.b.lengthSec,
    lanePath: loops.b.lane,
    name: "Track B",
    id: ID_B,
  });
  b.clips[0].volume_envelope = [
    { time_sec: 0, gain_db: -48 },
    { time_sec: 16, gain_db: 0 },
    { time_sec: 32, gain_db: 0 },
  ];
  return { loops, tracks: [a, b] };
}

async function load(app: App, extra: Record<string, ReturnType<typeof ok>> = {}) {
  const { loops, tracks } = session();
  await app.boot({
    ...projectWith(tracks, nodeId(7)),
    render_preview: ok(loops.a.lane),
    ...extra,
  });
  const page = app.page;
  await expect(page.getByTestId("ruler")).toContainText("0:40");
  // The shipped faces, loaded: a screenshot taken before them is in a
  // fallback font.
  await page.evaluate(() => document.fonts.load('400 14px "Geist Variable"'));
  await page.evaluate(() => document.fonts.load('400 14px "Geist Mono Variable"'));
  await page.getByTestId("fit-to-window-btn").click();
  await page.waitForTimeout(600);
}

async function send(app: App, text: string) {
  const box = app.page.getByTestId("chat-form").locator("textarea");
  await box.fill(text);
  await box.press("Enter");
  await expect.poll(() => app.requestsFor("send_message")).toHaveLength(1);
}

const REQUEST = "Match B's tempo to A, start it 8 bars before A ends, and crossfade them.";

const REPLY = [
  "Done. Track B was at 126 BPM, so I time-stretched it to 120, started it at 16.0 s (eight bars before A ends) and crossfaded the overlap.",
  "",
  "Want a low-pass on A's tail before I export?",
].join("\n");

/** The request, the tool calls it ran and the reply, as the chat shows them once the turn is done. */
async function converse(app: App) {
  const page = app.page;
  await send(app, REQUEST);
  const calls = ["analyze_track", "time_stretch", "move_clip", "set_clip_envelope"];
  for (const [i, name] of calls.entries()) {
    await app.emit("agent://tool-call", { name, id: `call-${i}` });
    await app.emit("agent://tool-call-end", { id: `call-${i}`, ok: true });
  }
  await app.emit("agent://text-delta", { text: REPLY });
  await app.emit("agent://done", {});
  await expect(page.getByTestId("caret")).toHaveCount(0);
  await page.waitForTimeout(700);
}

test.describe("at twice the pixels", () => {
  test.use({ deviceScaleFactor: 2 });

  test("the timeline, crossfaded", async ({ app }) => {
    await load(app);
    const page = app.page;
    // The lanes, without the empty space under them.
    const root = await page.getByTestId("timeline-root").boundingBox();
    const last = await page.getByTestId("timeline-lane").last().boundingBox();
    if (!root || !last) throw new Error("no timeline box");
    await page.screenshot({
      path: join(SHOTS, "timeline.png"),
      clip: { x: root.x, y: root.y, width: root.width, height: last.y + last.height - root.y + 1 },
    });
  });
});

test.describe("at three times the pixels", () => {
  // Text-heavy crops that a slide shows two or three times larger than
  // they are on screen, so they are taken at the density they are shown at.
  test.use({ deviceScaleFactor: 3 });

  test("the history graph, with a fork", async ({ app }) => {
    const n = nodeId;
    const ago = (min: number) => new Date(Date.now() - min * 60_000).toISOString();
    await load(app, {
      get_graph: ok({
        nodes: [
          { id: n(1), parent: null, label: "Load Track A", tool: "load", created_at: ago(14) },
          { id: n(2), parent: n(1), label: "Load Track B", tool: "load", created_at: ago(13) },
          { id: n(3), parent: n(2), label: "Match tempo", tool: "time_stretch", created_at: ago(9) },
          { id: n(4), parent: n(3), label: "Crossfade", tool: "set_clip_envelope", created_at: ago(7) },
          { id: n(5), parent: n(3), label: "Hard cut", tool: "split_clip", created_at: ago(6) },
          { id: n(7), parent: n(4), label: "Low-pass on A", tool: "low_pass_filter", created_at: ago(3) },
        ],
        head: n(7),
      }),
    });
    const page = app.page;
    await page.getByTestId("tab-graph").click();
    const bubbles = page.getByTestId("graph-bubble");
    await expect(bubbles).toHaveCount(6);
    await page.waitForTimeout(900);
    // The fork and what hangs off it (the last four nodes) with a margin:
    // not the empty canvas around them, and large enough to read.
    const boxes = await bubbles.evaluateAll((els) =>
      els
        .map((e) => {
          const r = e.getBoundingClientRect();
          return { text: e.textContent ?? "", l: r.left, t: r.top, r: r.right, b: r.bottom };
        })
        .filter((b) => /Match tempo|Crossfade|Hard cut|Low-pass on A/.test(b.text)),
    );
    expect(boxes).toHaveLength(4);
    const pad = 28;
    const l = Math.min(...boxes.map((b) => b.l)) - pad;
    const t = Math.min(...boxes.map((b) => b.t)) - pad;
    await page.screenshot({
      path: join(SHOTS, "graph.png"),
      clip: {
        x: l,
        y: t,
        width: Math.max(...boxes.map((b) => b.r)) + pad - l,
        height: Math.max(...boxes.map((b) => b.b)) + pad - t,
      },
    });
  });

  test("the chat, after a turn", async ({ app }) => {
    await load(app, { send_message: deferred("send") });
    const page = app.page;
    await converse(app);
    // The header to the end of the reply: not the empty panel under it.
    const root = await page.getByTestId("chat-root").boundingBox();
    const reply = await page.getByTestId("message-bubble").last().boundingBox();
    if (!root || !reply) throw new Error("no chat box");
    await page.screenshot({
      path: join(SHOTS, "chat.png"),
      clip: { x: root.x, y: root.y, width: root.width, height: reply.y + reply.height + 20 - root.y },
    });
    await app.release("send", null);
  });

  test("the plan card, held for approval", async ({ app }) => {
    await load(app, {
      get_plan_first: ok(true),
      send_message: deferred("send"),
      approve_plan: ok(null),
      reject_plan: ok(null),
    });
    const page = app.page;
    await send(app, "Match B's tempo to A, start it 8 bars before A ends, crossfade, then bring the mix to -14 LUFS.");
    await app.emit("agent://plan", {
      steps: [
        { step: 1, tool: "analyze_track", description: "Read the tempo of tracks 1 and 2" },
        { step: 2, tool: "time_stretch", description: "Stretch track 2 from 126 to 120 BPM" },
        { step: 3, tool: "move_clip", description: "Start track 2 at 16.0 s" },
        { step: 4, tool: "set_clip_envelope", description: "Crossfade the overlap" },
        { step: 5, tool: "normalize_loudness", description: "Bring the mix to -14 LUFS" },
      ],
    });
    const card = page.getByTestId("plan-approval-card");
    await expect(card).toBeVisible();
    await page.waitForTimeout(700);
    // The live card, with its Run and Discard. The transcript above it
    // keeps a read-only copy of the same plan, which the crop leaves out.
    const box = await card.boundingBox();
    if (!box) throw new Error("no card box");
    const m = 6;
    await page.screenshot({
      path: join(SHOTS, "plan.png"),
      clip: { x: box.x - m, y: box.y - m, width: box.width + 2 * m, height: box.height + 2 * m },
    });
    await page.getByTestId("plan-discard-button").click();
    await expect.poll(() => app.requestsFor("reject_plan")).toHaveLength(1);
    await app.release("send", null);
  });

  /** Settings open on Account. */
  async function openSettings(app: App) {
    await app.boot(readyToLoad());
    const page = app.page;
    await expect(page.getByTestId("empty-state")).toBeVisible();
    await page.evaluate(() => document.fonts.load('400 14px "Geist Variable"'));
    await page.getByTestId("open-settings-button").click();
    await expect(page.getByTestId("settings-card")).toBeVisible();
    await page.waitForTimeout(700);
    return page;
  }

  test("Settings: Ollama needs no key", async ({ app }) => {
    const page = await openSettings(app);
    // What the backend answers once Ollama is picked. `has_api_key_for`
    // is `provider_is_configured` (`commands.rs`): `true` for a provider
    // that `requires_api_key()` is false for, with no keychain read.
    // `set_active_provider` answers where the choice landed, as the save
    // commands do (`DURABLE_SAVE`). `default_base_url_for` is Ollama's
    // own address.
    await app.become({
      has_api_key_for: ok(true),
      set_active_provider: ok(DURABLE_SAVE),
      default_base_url_for: ok("http://localhost:11434"),
    });
    await page.getByText("Ollama (local)", { exact: true }).click();
    const note = page.getByTestId("settings-no-key-needed");
    await expect(note).toBeVisible();
    // No pointer or focus ring left on a button for the screenshot.
    await page.mouse.move(0, 0);
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
    await page.waitForTimeout(500);
    const box = await page.getByTestId("settings-card").boundingBox();
    const pick = await page.getByTestId("settings-provider-picker").boundingBox();
    const text = await note.boundingBox();
    if (!box || !pick || !text) throw new Error("no box");
    // The providers with Ollama chosen, and the note under them: from
    // just under the tab strip to the note's bottom edge.
    const top = pick.y - 12;
    await page.screenshot({
      path: join(SHOTS, "ollama.png"),
      clip: { x: box.x, y: top, width: box.width, height: text.y + text.height + 8 - top },
    });
  });
});
