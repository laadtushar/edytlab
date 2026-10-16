/**
 * The window says what a render leaves out (#532).
 *
 * A project saved by an older build can hold a reverb the renderer cannot
 * run in a chain. #531 made the render skip it and report it instead of
 * failing every render; the agent's tools pass the report on, but the
 * window played and exported without the effect and said nothing.
 *
 * Read from what a user sees in the real frontend: the notice above the
 * timeline when the head holds such an effect (so before Play, and after
 * it), the note on an export's result, and the two sides of an A/B compare.
 * The backend's answers are `get_render_warnings`' (from the node's stored
 * chains), `render_range`'s and `prepare_compare`'s, shaped from their
 * sources in `backend.ts`.
 */

import { fixturePath, fixtureSeconds } from "./audio-fixtures";
import { comparing, leftOut, nodeId, ok, projectWith, selecting, trackFor } from "./backend";
import { expect, test, type App } from "./fixtures";
import { dragSelect } from "./timeline";

const TONE = fixturePath("tone3s");
const SESSION = fixtureSeconds("tone3s");
const HEAD = nodeId(1);
const EDITED = nodeId(2);

const track = () => trackFor(TONE, SESSION, { name: "Vocals" });
const reverbOnVocals = leftOut({ type: "track", index: 0, name: "Vocals" }, "reverb", {
  effectIndex: 1,
});
const reverbOnMaster = leftOut({ type: "master" }, "reverb");

/** Open a project whose head stores `warnings`' effects, and wait for it to draw. */
async function open(app: App, warnings: unknown[], more: Parameters<App["boot"]>[0] = {}) {
  await app.boot({
    ...projectWith([track()], HEAD),
    get_render_warnings: ok(warnings),
    render_preview: ok(TONE),
    ...more,
  });
  await expect(app.page.getByTestId("ruler")).toContainText("0:03");
}

test.describe("Play on a project that stores an effect the render leaves out", () => {
  test("names the track and the effect, and plays all the same", async ({ app }) => {
    await open(app, [reverbOnVocals]);
    const page = app.page;
    const notice = page.getByTestId("render-warnings");

    // Said before anything is rendered: Play is about to leave it out.
    await expect(notice).toBeVisible();
    await expect(notice).toContainText(
      'Reverb on track 0 "Vocals" is left out: this build can\'t run it in a chain yet. ' +
        "Remove or bypass it to clear this.",
    );
    expect(await app.requestsFor("get_render_warnings")).toEqual([{ node: HEAD }]);
    expect(await app.requestsFor("render_preview")).toEqual([]);
    // Above the timeline, where a render error would be, and not over it.
    const noticeBox = await notice.boundingBox();
    const laneBox = await page.getByTestId("timeline-lane-waveform").first().boundingBox();
    expect(noticeBox && laneBox && noticeBox.y + noticeBox.height <= laneBox.y + 1).toBe(true);

    const play = page.getByTestId("play-pause-button");
    await play.click();
    await expect(play).toHaveAccessibleName("Pause");
    expect(await app.requestsFor("render_preview")).toEqual([{ node: HEAD }]);

    // Playing did not clear it, and is not an error.
    await expect(notice).toBeVisible();
    await expect(page.getByTestId("render-error")).toHaveCount(0);
    await play.click();
    await expect(play).toHaveAccessibleName("Play");
  });

  test("counts several and lists each one", async ({ app }) => {
    await open(app, [reverbOnVocals, reverbOnMaster]);
    const notice = app.page.getByTestId("render-warnings");

    await expect(notice).toContainText("2 effects are left out of the mix");
    const items = app.page.getByTestId("render-warnings-list").locator("li");
    await expect(items).toHaveText([
      'Reverb on track 0 "Vocals": this build can\'t run it in a chain yet.',
      "Reverb on the master bus: this build can't run it in a chain yet.",
    ]);
  });

  test("clears when the effect is removed or bypassed", async ({ app }) => {
    await open(app, [reverbOnVocals]);
    const page = app.page;
    await expect(page.getByTestId("render-warnings")).toBeVisible();

    // The assistant removes it: a new node, with nothing left out.
    await app.become({ get_render_warnings: ok([]) });
    await app.emit("agent://node-created", { node_id: EDITED });

    await expect(page.getByTestId("render-warnings")).toHaveCount(0);
    expect(await app.requestsFor("get_render_warnings")).toEqual([{ node: HEAD }, { node: EDITED }]);
  });

  test("shows nothing at all when nothing is left out", async ({ app }) => {
    await open(app, []);
    const page = app.page;

    await page.getByTestId("play-pause-button").click();
    await expect(page.getByTestId("play-pause-button")).toHaveAccessibleName("Pause");

    await expect(page.getByTestId("render-warnings")).toHaveCount(0);
    await expect(page.getByTestId("export-warnings")).toHaveCount(0);
    await expect(page.getByTestId("render-error")).toHaveCount(0);
    await page.getByTestId("play-pause-button").click();
  });
});

test.describe("Export Selection from such a project", () => {
  /** `render_range_inner`'s answer; `warnings` is what the file lacks. */
  const exported = (path: string, warnings: unknown[]) =>
    ok({
      path,
      frames_written: 48_000,
      sample_rate: 48_000,
      channels: 1,
      peak_dbfs: -10.5,
      summary: `Exported selection (0.60s–1.80s) → ${path}`,
      warnings,
    });

  test("says the file was written without it, and the note can be dismissed", async ({ app }) => {
    const out = "/home/user/Music/take.wav";
    await open(app, [reverbOnVocals], {
      ...selecting(),
      "plugin:dialog|save": ok(out),
      render_range: exported(out, [reverbOnVocals]),
    });
    const page = app.page;
    const span = await dragSelect(app, 0.2, 0.6);

    await page.getByTestId("export-selection-btn").click();

    const note = page.getByTestId("export-warnings");
    await expect(note).toBeVisible();
    await expect(note).toContainText(
      'Exported take.wav without Reverb on track 0 "Vocals": this build can\'t run it in a chain yet.',
    );
    expect(await app.requestsFor("render_range")).toEqual([
      { nodeId: HEAD, startSec: span.start, endSec: span.end, outPath: out },
    ]);
    // The file was written; this is not an error.
    await expect(page.getByTestId("render-error")).toHaveCount(0);

    await note.getByRole("button", { name: "Dismiss warning" }).click();
    await expect(note).toHaveCount(0);
    // The standing notice about the head is not the export's to dismiss.
    await expect(page.getByTestId("render-warnings")).toBeVisible();
  });

  test("is dropped when the head moves on: it was about the file made from the old one", async ({ app }) => {
    const out = "/home/user/Music/take.wav";
    await open(app, [reverbOnVocals], {
      ...selecting(),
      "plugin:dialog|save": ok(out),
      render_range: exported(out, [reverbOnVocals]),
    });
    const page = app.page;
    await dragSelect(app, 0.2, 0.6);
    await page.getByTestId("export-selection-btn").click();
    await expect(page.getByTestId("export-warnings")).toBeVisible();

    // An edit that leaves the reverb where it is: the head is another node.
    await app.emit("agent://node-created", { node_id: EDITED });

    await expect(page.getByTestId("export-warnings")).toHaveCount(0);
    await expect(page.getByTestId("render-warnings")).toBeVisible();
  });

  test("adds nothing when the file lacks nothing", async ({ app }) => {
    const out = "/home/user/Music/take.wav";
    await open(app, [], {
      ...selecting(),
      "plugin:dialog|save": ok(out),
      render_range: exported(out, []),
    });
    const page = app.page;
    await dragSelect(app, 0.2, 0.6);

    await page.getByTestId("export-selection-btn").click();

    await expect.poll(() => app.requestsFor("render_range")).toHaveLength(1);
    await expect(page.getByTestId("export-selection-btn")).toBeEnabled();
    await expect(page.getByTestId("export-warnings")).toHaveCount(0);
    await expect(page.getByTestId("render-error")).toHaveCount(0);
  });
});

test.describe("A/B compare of such a project", () => {
  const A = HEAD;
  const B = nodeId(3);

  test("lists what each side leaves out, in place of the head's own notice", async ({ app }) => {
    await open(app, [reverbOnVocals], {
      // `get_graph`: a load, and one edit after it.
      get_graph: ok({
        nodes: [
          { id: A, parent: null, label: "load", tool: "load", created_at: "2026-09-25T00:00:00Z" },
          { id: B, parent: A, label: "gain", tool: "gain", created_at: "2026-09-25T00:01:00Z" },
        ],
        head: A,
      }),
      // A stores the reverb; B, the edit after it, does not.
      ...comparing({
        a_path: TONE,
        b_path: fixturePath("tone3sB"),
        a_warnings: [reverbOnVocals],
        b_warnings: [],
      }),
    });
    const page = app.page;
    await expect(page.getByTestId("render-warnings")).toBeVisible();

    await page.getByTestId("tab-graph").click();
    await page.getByTestId("graph-bubble").nth(1).click({ button: "right" });
    await page.getByTestId("graph-context-menu").getByText("Compare with…").click();
    await expect(page.getByTestId("ab-compare-bar")).toBeVisible();

    const sides = page.getByTestId("ab-render-warnings");
    await expect(sides).toContainText('A · Reverb on track 0 "Vocals"');
    await expect(sides).not.toContainText("B ·");
    // The renders being compared are not the head's, so its notice steps aside.
    await expect(page.getByTestId("render-warnings")).toHaveCount(0);

    await page.getByTestId("ab-close").click();
    await expect(page.getByTestId("ab-render-warnings")).toHaveCount(0);
    await expect(page.getByTestId("render-warnings")).toBeVisible();
    // Closing the bar ends the compare, so the cache may evict its renders
    // again (#575).
    await expect.poll(() => app.requestsFor("end_compare")).toEqual([{ compareId: 1 }]);
  });
});
