/**
 * The zoom verbs frame what they say they frame.
 *
 * Zoom-to-selection used to set the zoom and then scroll the lane's own
 * wrapper to the selection. The wrapper never scrolls: WaveSurfer draws
 * into a `.scroll` container of its own, inside a shadow root, and the
 * wrapper only ever holds a pane-wide box. So the waveform zoomed in on
 * its first half-second, wherever the selection was, and the user was
 * left to scroll and hunt for it — the exact chore the verb exists to
 * remove. Found by selecting 1.8–2.4 s of a 3-second file and pressing
 * the button: the pane read 0:00.0 → 0:00.5.
 *
 * Measured on WaveSurfer's scroll container, which is what the user
 * sees move. jsdom has no layout, so none of this is visible to vitest.
 */

import type { Page } from "@playwright/test";

import { fixturePath, fixtureSeconds } from "./audio-fixtures";
import { nodeId, readyToLoad, selecting, sessionWith, trackFor } from "./backend";
import { expect, test, type App } from "./fixtures";

const SECONDS = fixtureSeconds("tone3s");

/** Load the 3-second tone the way the agent's `load` tool does. */
async function loadTone(app: App): Promise<void> {
  await app.boot({ ...readyToLoad(), ...selecting() });
  await app.become(sessionWith([trackFor(fixturePath("tone3s"), SECONDS)]));
  await app.emit("agent://node-created", { node_id: nodeId(1) });
  await expect(app.page.getByTestId("ruler")).toContainText("0:03");
}

/**
 * Drag across the lane from one fraction of its width to another, as a
 * user selects. Returns the selection the app committed, read back from
 * the status bar rather than computed here, so the test checks the
 * framing of whatever the app actually selected.
 */
async function dragSelect(page: Page, from: number, to: number): Promise<{ start: number; end: number }> {
  const lane = page.getByTestId("timeline-lane-waveform");
  const box = await lane.boundingBox();
  if (!box) throw new Error("the lane has no box");
  const y = box.y + box.height / 2;
  await page.mouse.move(box.x + box.width * from, y);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width * to, y, { steps: 5 });
  await page.mouse.up();

  const text = await page.getByTestId("status-bar-selection").textContent();
  const m = /sel (\d+):(\d+\.\d+) → (\d+):(\d+\.\d+)/.exec(text ?? "");
  if (!m) throw new Error(`no selection in the status bar: ${text}`);
  return {
    start: Number(m[1]) * 60 + Number(m[2]),
    end: Number(m[3]) * 60 + Number(m[4]),
  };
}

/** The span of audio a lane's pane is showing, in seconds. */
function visibleWindow(page: Page, lane = 0): Promise<{ start: number; end: number }> {
  return page
    .getByTestId("timeline-lane-waveform")
    .nth(lane)
    .locator(".scroll")
    .evaluate((el, seconds) => {
      const pxPerSec = el.scrollWidth / seconds;
      return {
        start: el.scrollLeft / pxPerSec,
        end: (el.scrollLeft + el.clientWidth) / pxPerSec,
      };
    }, SECONDS);
}

/**
 * The status bar prints two decimals, and the pane is a whole number of
 * pixels wide. Framing within one hundredth either way is exact to what
 * the user can read.
 */
const SLACK = 0.02;

/** Wait until lane 0's pane shows exactly the selection. */
async function expectFramed(page: Page, sel: { start: number; end: number }): Promise<void> {
  await expect
    .poll(async () => {
      const v = await visibleWindow(page);
      return Math.abs(v.start - sel.start) <= SLACK && Math.abs(v.end - sel.end) <= SLACK;
    }, { message: "the pane frames the selection" })
    .toBe(true);
}

test.describe("zoom to selection", () => {
  test("fills the pane with the selection, not the start of the file", async ({ app }) => {
    await loadTone(app);
    const page = app.page;
    const sel = await dragSelect(page, 0.6, 0.8);
    // Well clear of the start, so a pane left at 0 cannot pass.
    expect(sel.start).toBeGreaterThan(1);

    await page.getByTestId("zoom-to-selection-btn").click();

    await expectFramed(page, sel);

    // And the ruler, which follows the pane (#323), says so: every label
    // it draws is inside the selection.
    // Retried, not read once: the ruler redraws from the pane's scroll
    // event, a React render after the scroll itself, so a single read can
    // still see the labels from before the zoom.
    await expect(async () => {
      const labels = (await page.getByTestId("ruler").locator("span").allTextContents()).map(
        (l) => {
          const [mm, ss] = l.split(":");
          return Number(mm) * 60 + Number(ss);
        },
      );
      expect(labels.length, "the ruler labels the framed window").toBeGreaterThan(1);
      for (const t of labels) {
        expect(t).toBeGreaterThanOrEqual(sel.start - SLACK);
        expect(t).toBeLessThanOrEqual(sel.end + SLACK);
      }
    }).toPass();
  });

  test("Ctrl+E does the same as the button", async ({ app }) => {
    await loadTone(app);
    const page = app.page;
    const sel = await dragSelect(page, 0.6, 0.8);

    await page.keyboard.press("Control+e");

    await expectFramed(page, sel);
  });

  /**
   * From review. The request used to be a one-off that lived on in the
   * timeline's state, and a lane that mounted later — a track added, or
   * renamed, since lanes are keyed by name — applied it then. The new
   * lane jumped to a selection framed long before, wherever the user
   * had panned since.
   *
   * There is one scroll for the whole timeline now (#344), so a new lane
   * shows what every other lane shows: where the user is, not where they
   * once zoomed.
   */
  test("a track added afterwards shows where the user is, not an old framing", async ({ app }) => {
    await loadTone(app);
    const page = app.page;
    const sel = await dragSelect(page, 0.6, 0.8);
    await page.getByTestId("zoom-to-selection-btn").click();
    await expectFramed(page, sel);

    // Pan back to the start, away from the selection.
    await page.getByTestId("timeline-hscroll").evaluate((el) => {
      el.scrollLeft = 0;
    });
    await expect.poll(async () => (await visibleWindow(page)).start).toBe(0);

    const first = trackFor(fixturePath("tone3s"), SECONDS);
    const second = { ...first, id: "9c1d2e3f-4a5b-4c6d-8e7f-0a1b2c3d4e5f", name: "Track 2" };
    await app.become(sessionWith([first, second]));
    await app.emit("agent://node-created", { node_id: nodeId(2) });
    await expect(page.getByTestId("timeline-lane-waveform")).toHaveCount(2);

    // Drawn at the zoom, at the start with lane 0 — not at the selection.
    // Given time to decode and draw before reading.
    await expect
      .poll(async () => {
        const v = await visibleWindow(page, 1);
        return v.end - v.start < 1 ? v.start : null;
      }, { message: "the new lane, drawn at the zoom where lane 0 is" })
      .toBe(0);
  });

  test("keeps a selection too short to fill the pane in the middle of it", async ({ app }) => {
    // Zoom stops at 2000 px/s, so a selection narrower than the pane at
    // that density cannot fill it. Scrolling to its start would park it
    // against the left edge; centring it is what "frame" means when
    // filling is impossible.
    await loadTone(app);
    const page = app.page;
    const sel = await dragSelect(page, 0.7, 0.73);
    const lanePx = (await page.getByTestId("timeline-lane-waveform").boundingBox())!.width;
    expect(sel.end - sel.start).toBeLessThan(lanePx / 2000);

    await page.getByTestId("zoom-to-selection-btn").click();

    await expect
      .poll(async () => {
        const v = await visibleWindow(page);
        const mid = (v.start + v.end) / 2;
        return (
          v.start <= sel.start &&
          v.end >= sel.end &&
          Math.abs(mid - (sel.start + sel.end) / 2) <= SLACK
        );
      }, { message: "the selection sits in the middle of the pane" })
      .toBe(true);
  });
});

test.describe("fit to window", () => {
  test("shows the whole file again after zooming in", async ({ app }) => {
    await loadTone(app);
    const page = app.page;
    await dragSelect(page, 0.6, 0.8);
    await page.getByTestId("zoom-to-selection-btn").click();
    await expect
      .poll(async () => {
        const v = await visibleWindow(page);
        return v.end - v.start;
      })
      .toBeLessThan(1);

    await page.getByTestId("fit-to-window-btn").click();

    await expect
      .poll(async () => {
        const v = await visibleWindow(page);
        return v.start === 0 && Math.abs(v.end - SECONDS) <= SLACK;
      }, { message: "the whole file is on screen" })
      .toBe(true);
    await expect(page.getByTestId("ruler")).toContainText("0:03");
  });
});
