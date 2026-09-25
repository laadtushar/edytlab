/**
 * A volume curve a tool writes draws itself on (#269 §1).
 *
 * A curve that appeared fully formed gave no sign of what had changed —
 * and for `duck_under_speech`, whose whole visible output is the curve,
 * that is all there is to see. It must not animate on first paint, nor
 * for the user's own edit coming back from the backend: only a curve
 * that arrives different from the one on screen draws on.
 */

import type { ClipSummary, TrackSummary } from "../src/lib/tauri-bridge";
import { fixturePath, fixtureSeconds, SAMPLE_RATE } from "./audio-fixtures";
import { deferred, nodeId, projectWith, sessionWith, trackFor } from "./backend";
import { expect, test, type App } from "./fixtures";

const SECONDS = fixtureSeconds("tone3s");
type Point = ClipSummary["volume_envelope"][number];

/** The tone's track with this curve on its one clip. */
function withCurve(points: Point[]): TrackSummary {
  const t = trackFor(fixturePath("tone3s"), SECONDS);
  return { ...t, clips: [{ ...t.clips[0], volume_envelope: points }] };
}

/**
 * A curve as `list_tracks` hands it back: `set_clip_envelope` stores
 * time in the clip's own samples and gain as an `f32`, and the listing
 * converts back — so what returns is close to what was sent, not equal.
 */
function asStored(points: Point[]): Point[] {
  return points.map((p) => ({
    time_sec: Math.round(p.time_sec * SAMPLE_RATE) / SAMPLE_RATE,
    gain_db: Math.fround(p.gain_db),
  }));
}

const FLAT: Point[] = [{ time_sec: 0, gain_db: 0 }];
const DUCKED: Point[] = [
  { time_sec: 0, gain_db: 0 },
  { time_sec: 1, gain_db: -12 },
  { time_sec: 2, gain_db: 0 },
];

async function open(app: App): Promise<void> {
  await app.boot(projectWith([withCurve(FLAT)], nodeId(1)));
  // Attached, not visible: a flat curve is a line with no height.
  await expect(app.page.getByTestId("automation-curve-0")).toBeAttached();
}

const curve = (app: App) => app.page.getByTestId("automation-curve-0");
const animations = (app: App) =>
  curve(app).evaluate((el) => el.getAnimations().map((a) => (a as CSSAnimation).animationName));

test("the first paint does not animate", async ({ app }) => {
  await open(app);
  await expect(curve(app)).toHaveAttribute("data-motion", "none");
  expect(await animations(app)).toEqual([]);
});

test("a curve a tool writes draws itself on", async ({ app }) => {
  await open(app);
  // What the agent's edit leaves behind, then the event it raises.
  await app.become(sessionWith([withCurve(DUCKED)]));
  await app.emit("agent://node-created", { node_id: nodeId(2) });

  await expect(curve(app)).toHaveAttribute("data-motion", "draw-on");
  expect(await animations(app)).toContain("automation-draw-on");
});

test("the user's own edit, coming back from the backend, does not", async ({ app }) => {
  await open(app);
  await app.become({ set_clip_envelope: deferred("envelope") });

  // A click on the lane adds a point, and commits the curve at once.
  const band = app.page.getByTestId("automation-band-0");
  const box = (await band.boundingBox())!;
  await band.click({ position: { x: box.width * 0.5, y: box.height * 0.3 } });
  await expect.poll(() => app.requestsFor("set_clip_envelope")).toHaveLength(1);
  const [{ points }] = (await app.requestsFor("set_clip_envelope")) as { points: Point[] }[];
  expect(points.length).toBeGreaterThan(1);

  // The backend stores it, and the refresh lists it as stored.
  await app.become(sessionWith([withCurve(asStored(points))]));
  await app.release("envelope", nodeId(3));
  await expect.poll(() => app.requestsFor("list_tracks").then((r) => r.length)).toBeGreaterThan(1);
  await app.settle();

  await expect(curve(app)).toHaveAttribute("data-motion", "none");
  expect(await animations(app)).toEqual([]);
});

test("reduced motion shortens it to nothing, through the global block", async ({ app }) => {
  await app.page.emulateMedia({ reducedMotion: "reduce" });
  await open(app);
  await app.become(sessionWith([withCurve(DUCKED)]));
  await app.emit("agent://node-created", { node_id: nodeId(2) });

  await expect(curve(app)).toHaveAttribute("data-motion", "draw-on");
  expect(await curve(app).evaluate((el) => getComputedStyle(el).animationDuration)).toBe("1e-05s");
});
