/**
 * Reading the timeline the way a user sees it.
 *
 * Everything here is measured on screen — the pixels a lane has inked,
 * the box an overlay occupies, the labels the ruler prints — and turned
 * into session seconds with nothing but the view a test expects. None
 * of it asks the app what it thinks it drew. A lane that drew the right
 * audio in the wrong place has to look wrong here.
 */

import type { Page } from "@playwright/test";

import { expect, type App } from "./fixtures";

/** A stretch of the session, in seconds. */
export interface Span {
  start: number;
  end: number;
}

/** WaveSurfer's scroll container for a lane: the lane's time surface. */
export function laneSurface(page: Page, lane: number) {
  return page.getByTestId("timeline-lane-waveform").nth(lane).locator(".scroll");
}

/**
 * Drag across lane 0 from one fraction of its surface to another, as a
 * user selects, and return the selection the app committed.
 *
 * Read back from what the app told the backend (`set_selection_context`,
 * the range the agent edits), at full precision, rather than from the
 * status bar: that prints hundredths, and zoomed in, a hundredth of a
 * second is a dozen pixels — too coarse to check a position against.
 */
export async function dragSelect(app: App, from: number, to: number): Promise<Span> {
  const page = app.page;
  const sent = async () => (await app.requestsFor("set_selection_context")).length;
  const before = await sent();
  const box = await laneSurface(page, 0).boundingBox();
  if (!box) throw new Error("the lane has no box");
  const y = box.y + box.height / 2;
  await page.mouse.move(box.x + box.width * from, y);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width * to, y, { steps: 5 });
  await page.mouse.up();

  await expect.poll(sent, { message: "the selection reaches the backend" }).toBeGreaterThan(before);
  const requests = (await app.requestsFor("set_selection_context")) as {
    range: { start_sec: number; end_sec: number } | null;
  }[];
  const range = requests[requests.length - 1].range;
  if (!range) throw new Error("the drag cleared the selection");
  return { start: range.start_sec, end: range.end_sec };
}

/**
 * The session seconds where a lane visibly draws audio, given the view
 * the lane should be showing, or null when it draws none on screen.
 *
 * Read from the waveform canvases' pixels. Only the columns inside the
 * lane's surface count — the rest are scrolled out of sight — and the
 * progress layer is left out, since it is clipped to nothing until the
 * lane plays.
 *
 * Silence is not blank: a bar of zero height still paints a one-pixel
 * antialiased hairline across the middle. So a column counts as audio
 * only when more of it is inked than that line.
 */
export function inkedSpan(page: Page, lane: number, view: Span): Promise<Span | null> {
  return laneSurface(page, lane).evaluate((scroll, { v, HAIRLINE_PX }) => {
    const surface = scroll.getBoundingClientRect();
    let lo = Infinity;
    let hi = -Infinity;
    for (const canvas of scroll.querySelectorAll<HTMLCanvasElement>(".canvases canvas")) {
      const r = canvas.getBoundingClientRect();
      if (!canvas.width || !canvas.height || !r.width) continue;
      const ctx = canvas.getContext("2d");
      if (!ctx) continue;
      const px = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
      for (let x = 0; x < canvas.width; x++) {
        let inked = 0;
        for (let y = 0; y < canvas.height && inked <= HAIRLINE_PX; y++) {
          if (px[(y * canvas.width + x) * 4 + 3] > 0) inked++;
        }
        if (inked <= HAIRLINE_PX) continue;
        const screenX = r.left + ((x + 0.5) / canvas.width) * r.width;
        if (screenX < surface.left || screenX > surface.right) continue;
        lo = Math.min(lo, screenX);
        hi = Math.max(hi, screenX);
      }
    }
    if (lo === Infinity) return null;
    const pxPerSec = surface.width / (v.end - v.start);
    return {
      start: v.start + (lo - surface.left) / pxPerSec,
      end: v.start + (hi - surface.left) / pxPerSec,
    };
  }, { v: view, HAIRLINE_PX: 2 });
}

/** Where a time sits on screen, for a surface showing `view`. */
export async function screenX(page: Page, sec: number, view: Span): Promise<number> {
  const box = await laneSurface(page, 0).boundingBox();
  if (!box) throw new Error("the lane has no box");
  return box.x + ((sec - view.start) / (view.end - view.start)) * box.width;
}

/** Every time the ruler labels, in seconds. */
export async function rulerTimes(page: Page): Promise<number[]> {
  const labels = await page.getByTestId("ruler").locator("span").allTextContents();
  return labels.map((l) => {
    const [mm, ss] = l.split(":");
    return Number(mm) * 60 + Number(ss);
  });
}
