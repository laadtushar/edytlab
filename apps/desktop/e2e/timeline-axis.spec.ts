/**
 * Every row of the timeline is drawn on one axis: the session's (#344,
 * #348).
 *
 * Each lane used to be its own WaveSurfer, drawing its own file across
 * the pane and scrolling on its own. A lane lined up with the ruler
 * only when its audio started at zero and ran the whole session — so a
 * shorter track, or a clip placed later, showed audio from some other
 * time under every ruler label. Once zoomed, the selection overlay, the
 * playhead and a drag all went on mapping pixels as if the pane still
 * held the whole session: selecting a word on a zoomed lane selected a
 * second and a half the user never saw.
 *
 * These are measured on screen — inked pixels, element boxes, printed
 * labels — against the view each test expects. jsdom has no layout, so
 * none of this is visible to vitest.
 */

import { fixturePath, fixtureSeconds } from "./audio-fixtures";
import {
  nodeId,
  ok,
  placedTrack,
  readyToLoad,
  selecting,
  sessionWith,
  trackFor,
  type Backend,
} from "./backend";
import { expect, test, type App } from "./fixtures";
import type { TrackSummary } from "../src/lib/tauri-bridge";
import {
  dragSelect,
  inkedSpan,
  laneSurface,
  rulerTimes,
  screenX,
  type Span,
} from "./timeline";

const SESSION = fixtureSeconds("tone3s");
const WHOLE: Span = { start: 0, end: SESSION };

const long = () => trackFor(fixturePath("tone3s"), SESSION, { name: "Long" });
/** One second of tone at zero: a track that ends early. */
const short = () =>
  trackFor(fixturePath("tone1s"), 1, {
    name: "Short",
    id: "5a6b7c8d-9e0f-4a1b-8c2d-3e4f5a6b7c8d",
  });
/** The same second of tone, placed one second in. */
const late = () =>
  placedTrack({
    source: fixturePath("tone1s"),
    startSec: 1,
    lengthSec: 1,
    lanePath: fixturePath("tone1sAt1s"),
    name: "Late",
    id: "7d8e9f0a-1b2c-4d3e-9f4a-5b6c7d8e9f0a",
  });

/**
 * A pixel column is a third of a bar at the narrowest pane these run in,
 * and a bar is 3px: within 0.05 s is exact to what is drawn.
 */
const SLACK = 0.05;

async function load(app: App, tracks: TrackSummary[], extra: Backend = {}): Promise<void> {
  await app.boot({ ...readyToLoad(), ...selecting(), ...extra });
  await app.become({ ...sessionWith(tracks), ...extra });
  await app.emit("agent://node-created", { node_id: nodeId(1) });
  await expect(app.page.getByTestId("timeline-lane-waveform")).toHaveCount(tracks.length);
  await expect(app.page.getByTestId("ruler")).toContainText("0:03");
}

/** Wait until `lane` has inked `want` on screen, for a surface showing `view`. */
async function expectInked(app: App, lane: number, view: Span, want: Span): Promise<void> {
  await expect(async () => {
    const got = await inkedSpan(app.page, lane, view);
    expect(got, `lane ${lane} inked nothing`).not.toBeNull();
    expect(Math.abs(got!.start - want.start), `lane ${lane} starts at ${got!.start}`).toBeLessThanOrEqual(SLACK);
    expect(Math.abs(got!.end - want.end), `lane ${lane} ends at ${got!.end}`).toBeLessThanOrEqual(SLACK);
  }).toPass();
}

/** Select, then zoom to it; returns what the pane now frames. */
async function zoomTo(app: App, from: number, to: number): Promise<Span> {
  const sel = await dragSelect(app, from, to);
  await app.page.getByTestId("zoom-to-selection-btn").click();
  return sel;
}

test.describe("lanes on the session's axis", () => {
  test("at fit, a shorter track and a placed clip sit where the ruler says", async ({ app }) => {
    await load(app, [long(), short(), late()]);

    await expectInked(app, 0, WHOLE, { start: 0, end: 3 });
    // Stretched across the pane, both of these used to run 0 → 3.
    await expectInked(app, 1, WHOLE, { start: 0, end: 1 });
    await expectInked(app, 2, WHOLE, { start: 1, end: 2 });
  });

  test("zoomed in, every lane shows the same stretch of the session", async ({ app }) => {
    await load(app, [long(), short(), late()]);
    // Across the second where the short track ends and the late one
    // starts.
    const sel = await zoomTo(app, 0.25, 0.45);
    expect(sel.start).toBeLessThan(1 - 0.1);
    expect(sel.end).toBeGreaterThan(1 + 0.1);

    await expectInked(app, 0, sel, sel);
    await expectInked(app, 1, sel, { start: sel.start, end: 1 });
    await expectInked(app, 2, sel, { start: 1, end: sel.end });
  });

  test("the ruler reads the session's view, not the first lane's", async ({ app }) => {
    // Lane 0 ends at 1 s; the selection is past it.
    await load(app, [short(), long()]);
    const sel = await zoomTo(app, 0.7, 0.85);
    expect(sel.start).toBeGreaterThan(1.5);

    await expectInked(app, 1, sel, sel);
    await expect(async () => {
      const times = await rulerTimes(app.page);
      expect(times.length).toBeGreaterThan(1);
      for (const t of times) {
        expect(t).toBeGreaterThanOrEqual(sel.start - SLACK);
        expect(t).toBeLessThanOrEqual(sel.end + SLACK);
      }
    }).toPass();
    // And lane 0, whose track is over by then, shows nothing at all.
    await expect.poll(() => inkedSpan(app.page, 0, sel)).toBeNull();
  });
});

test.describe("the overlay, the drag and the playhead follow the zoom", () => {
  test("the selection overlay covers the selection, zoomed or not", async ({ app }) => {
    await load(app, [long()]);
    const sel = await dragSelect(app, 0.6, 0.8);
    const overlay = app.page.getByTestId("timeline-selection-overlay");

    const atFit = await overlay.boundingBox();
    expect(Math.abs(atFit!.x - (await screenX(app.page, sel.start, WHOLE)))).toBeLessThanOrEqual(2);

    await app.page.getByTestId("zoom-to-selection-btn").click();
    // The selection now fills the pane, and so must its overlay.
    await expect(async () => {
      const surface = (await laneSurface(app.page, 0).boundingBox())!;
      const box = (await overlay.boundingBox())!;
      expect(Math.abs(box.x - surface.x)).toBeLessThanOrEqual(2);
      expect(Math.abs(box.x + box.width - (surface.x + surface.width))).toBeLessThanOrEqual(2);
    }).toPass();
  });

  test("a drag on a zoomed lane selects the times under the pointer", async ({ app }) => {
    await load(app, [long()]);
    const framed = await zoomTo(app, 0.6, 0.8);
    const span = framed.end - framed.start;
    // Wait for the zoom to land before dragging on it.
    await expectInked(app, 0, framed, framed);

    const sel = await dragSelect(app, 0.25, 0.75);

    // It used to select 25–75% of the *file*: 0:00.75 → 0:02.25.
    expect(Math.abs(sel.start - (framed.start + 0.25 * span))).toBeLessThanOrEqual(0.02);
    expect(Math.abs(sel.end - (framed.start + 0.75 * span))).toBeLessThanOrEqual(0.02);
  });

  test("the playhead sits on the time being played", async ({ app }) => {
    const tone = fixturePath("tone3s");
    // The mix the transport plays: a render of the head.
    await load(app, [long()], { render_preview: ok(tone) });
    await app.page.getByTestId("render-preview-button").click();

    const framed = await zoomTo(app, 0.2, 0.5);
    await expectInked(app, 0, framed, framed);
    // Home, then one second on: inside the framed stretch.
    await app.page.keyboard.press("Home");
    await app.page.keyboard.press("Shift+ArrowRight");
    const playhead = app.page.getByTestId("timeline-playhead");
    await expect(playhead).toHaveAttribute("data-playhead-sec", "1");

    await expect(async () => {
      const box = (await playhead.boundingBox())!;
      expect(Math.abs(box.x - (await screenX(app.page, 1, framed)))).toBeLessThanOrEqual(2);
    }).toPass();
  });
});

test.describe("one horizontal scroll for the whole timeline", () => {
  test("panning moves every lane and the ruler together", async ({ app }) => {
    await load(app, [long(), late()]);
    const framed = await zoomTo(app, 0.1, 0.3);
    const span = framed.end - framed.start;
    await expectInked(app, 0, framed, framed);

    // Half a pane to the right, on the timeline's own scrollbar.
    const surface = (await laneSurface(app.page, 0).boundingBox())!;
    await app.page
      .getByTestId("timeline-hscroll")
      .evaluate((el, px) => {
        el.scrollLeft += px;
      }, surface.width / 2);

    const panned = { start: framed.start + span / 2, end: framed.end + span / 2 };
    await expectInked(app, 0, panned, panned);
    await expectInked(app, 1, panned, {
      start: Math.max(1, panned.start),
      end: Math.min(2, panned.end),
    });
    await expect(async () => {
      for (const t of await rulerTimes(app.page)) {
        expect(t).toBeGreaterThanOrEqual(panned.start - SLACK);
        expect(t).toBeLessThanOrEqual(panned.end + SLACK);
      }
    }).toPass();
  });

  test("a horizontal wheel over a lane pans", async ({ app }) => {
    await load(app, [long()]);
    const framed = await zoomTo(app, 0.1, 0.3);
    await expectInked(app, 0, framed, framed);

    const surface = (await laneSurface(app.page, 0).boundingBox())!;
    await app.page.mouse.move(surface.x + surface.width / 2, surface.y + surface.height / 2);
    await app.page.mouse.wheel(surface.width / 4, 0);

    await expect
      .poll(() => app.page.getByTestId("timeline-hscroll").evaluate((el) => el.scrollLeft))
      .toBeGreaterThan(0);
  });
});

test.describe("clips and markers follow the zoom", () => {
  test("a clip chip spans its clip's time on screen", async ({ app }) => {
    await load(app, [long(), late()]);
    const framed = await zoomTo(app, 0.25, 0.45);
    await expectInked(app, 0, framed, framed);

    // Track "Late" holds one clip, 1 → 2 s; the frame starts before it
    // and ends inside it, so the chip runs from 1 s to the pane's edge.
    const chip = app.page.getByTestId("clip-strip").nth(1).getByTestId("clip-chip-0");
    const surface = (await laneSurface(app.page, 0).boundingBox())!;
    // Moved by the zoom at once, like the audio under it. A chip that
    // eased there trailed the waveform by the length of its motion.
    expect(await chip.evaluate((el) => el.getAnimations().length)).toBe(0);
    await expect(async () => {
      const box = (await chip.boundingBox())!;
      expect(Math.abs(box.x - (await screenX(app.page, 1, framed)))).toBeLessThanOrEqual(2);
      // Clipped at the pane's right edge, not drawn past it.
      const visibleRight = Math.min(box.x + box.width, surface.x + surface.width);
      expect(Math.abs(visibleRight - (surface.x + surface.width))).toBeLessThanOrEqual(2);
    }).toPass();
  });

  test("a marker and its label stay on their time when zoomed", async ({ app }) => {
    const marker = {
      id: "0b1c2d3e-4f5a-4b6c-8d7e-9f0a1b2c3d4e",
      name: "cue",
      kind: "marker",
      time_sec: 1.2,
    };
    // `list_markers` serialises each `Annotation` as-is.
    await load(app, [long()], { list_markers: ok([marker]) });
    const framed = await zoomTo(app, 0.3, 0.5);
    expect(framed.start).toBeLessThan(marker.time_sec);
    expect(framed.end).toBeGreaterThan(marker.time_sec);
    await expectInked(app, 0, framed, framed);

    // The flag over the lanes, and its chip in the label lane under them.
    const flag = app.page.getByTestId("marker-flag");
    const chip = app.page.getByTestId("label-chip");
    await expect(async () => {
      const x = await screenX(app.page, marker.time_sec, framed);
      expect(Math.abs((await flag.boundingBox())!.x - x)).toBeLessThanOrEqual(2);
      expect(Math.abs((await chip.boundingBox())!.x - x)).toBeLessThanOrEqual(2);
    }).toPass();
  });
});
