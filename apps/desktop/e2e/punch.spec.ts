/**
 * Punch-in, in a real browser (#203).
 *
 * jsdom cannot see where the popover is drawn, whether a drag on the lane
 * really makes the selection the control reads, or what the head does when
 * the take lands. These run the real frontend in Chromium with only the IPC
 * boundary replaced. The rules under test: the control is off until a region
 * is selected; it says what the take will be placed with, or "Not
 * calibrated" and the way to Settings → Audio; Start is one `punch_record`
 * with the region and the track chosen; the new head is adopted and the
 * tracks read again; Cancel is its own command and leaves the head alone.
 */

import {
  NOT_CALIBRATED_TEXT,
  RECORDING_DEVICES,
  manualLatencyCommands,
  nodeId,
  ok,
  projectWith,
  punchCommands,
  recordingLatencyStatus,
  selecting,
  toneTrack,
  type Backend,
} from "./backend";
import { expect, test, type App } from "./fixtures";
import { dragSelect } from "./timeline";

test.use({ viewport: { width: 1280, height: 800 } });

const A = nodeId(1);
const B = nodeId(2);

const MEASURED = {
  latency_ms: 75.3,
  spread_ms: 0.1,
  measured_at: 1_800_000_000,
  source: "measured",
};

async function bootProject(app: App, extra: Backend = {}) {
  await app.boot({
    ...projectWith([toneTrack()], A),
    ...selecting(),
    get_recording_latency: ok(recordingLatencyStatus(MEASURED)),
    ...punchCommands({ head: B }),
    ...extra,
  });
  const page = app.page;
  await expect(page.getByTestId("ruler")).toContainText("0:03");
  await expect(page.getByTestId("timeline-lane-name")).toHaveText(["Track 1"]);
  return page;
}

test("the Punch button is off until a region is selected", async ({ app }) => {
  const page = await bootProject(app);
  const punch = page.getByTestId("punch-btn");
  await expect(punch).toBeDisabled();
  await expect(punch).toHaveAttribute("title", /select the region/i);

  await dragSelect(app, 0.2, 0.5);
  await expect(punch).toBeEnabled();
  // The popover asks nothing of the devices until it is opened.
  await app.settle();
  expect(await app.requestsFor("get_recording_latency")).toEqual([]);
});

test("select a region, Punch, Start: the take is requested and the new head is applied", async ({
  app,
}) => {
  const page = await bootProject(app);
  const span = await dragSelect(app, 0.2, 0.5);

  await page.getByTestId("punch-btn").click();
  await expect(page.getByTestId("punch-panel")).toBeVisible();
  await expect(page.getByTestId("punch-latency")).toContainText("75.3 ms");
  await expect(page.getByTestId("punch-latency")).toContainText("steady to 0.1 ms");
  await expect(page.getByTestId("punch-region")).toContainText("s)");
  await expect(page.getByTestId("punch-start")).toBeEnabled();

  // The punch is in the session from here on: the next read of the tracks
  // and of the head is the new one.
  const reads = (await app.requestsFor("list_tracks")).length;
  await app.become({ get_session_head: ok(B) });
  await page.getByTestId("punch-start").click();

  await expect(page.getByTestId("status-bar-head")).toContainText(B.slice(0, 7));
  const asked = (await app.requestsFor("punch_record")) as Record<string, number>[];
  expect(asked).toHaveLength(1);
  expect(asked[0].track).toBe(0);
  expect(asked[0].prerollSec).toBe(2);
  expect(asked[0].startSec).toBeCloseTo(span.start, 6);
  expect(asked[0].endSec).toBeCloseTo(span.end, 6);
  await expect(page.getByTestId("punch-notice")).toContainText("Punched in");
  await expect(page.getByTestId("punch-notice")).toContainText("75.3 ms");
  await expect(page.getByTestId("render-error")).toHaveCount(0);
  await expect.poll(async () => (await app.requestsFor("list_tracks")).length).toBeGreaterThan(reads);
  // The popover is gone and the control is back.
  await expect(page.getByTestId("punch-panel")).toHaveCount(0);
});

test("the lead-in and track chosen are what is requested", async ({ app }) => {
  const page = await bootProject(app);
  await dragSelect(app, 0.2, 0.5);
  await page.getByTestId("punch-btn").click();
  await page.getByTestId("punch-preroll").fill("4");
  await page.getByTestId("punch-start").click();
  await expect.poll(async () => (await app.requestsFor("punch_record")).length).toBe(1);
  const asked = (await app.requestsFor("punch_record")) as Record<string, number>[];
  expect(asked[0].prerollSec).toBe(4);
});

test("with no delay kept the popover says Not calibrated and Start stays off", async ({ app }) => {
  const page = await bootProject(app, {
    get_recording_latency: ok(recordingLatencyStatus(null)),
  });
  await dragSelect(app, 0.2, 0.5);
  await page.getByTestId("punch-btn").click();

  await expect(page.getByTestId("punch-latency")).toHaveText("Not calibrated — Settings → Audio");
  await expect(page.getByTestId("punch-start")).toBeDisabled();

  // The way out: Settings, on the Audio tab, where the delay is measured or typed.
  await page.getByTestId("punch-open-settings").click();
  await expect(page.getByTestId("settings-card")).toBeVisible();
  await expect(page.getByTestId("recording-latency")).toBeVisible();
  await expect(page.getByTestId("recording-latency-manual-input")).toBeVisible();
  await expect(page.getByTestId("recording-latency-devices")).toContainText(
    RECORDING_DEVICES.host,
  );
  expect(await app.requestsFor("punch_record")).toEqual([]);
});

test("a refusal for want of a delay is the banner's, with the way to the Audio tab", async ({
  app,
}) => {
  // The window believed a delay was kept (the devices changed under it).
  const page = await bootProject(app, punchCommands("refused"));
  await dragSelect(app, 0.2, 0.5);
  await page.getByTestId("punch-btn").click();
  await page.getByTestId("punch-start").click();

  const banner = page.getByTestId("render-error");
  await expect(banner).toContainText(NOT_CALIBRATED_TEXT.slice(0, 60));
  await banner.getByRole("button", { name: "Open Settings → Audio" }).click();
  await expect(page.getByTestId("recording-latency")).toBeVisible();
  await expect(page.getByTestId("render-error")).toHaveCount(0);
  // Nothing was adopted.
  await expect(page.getByTestId("status-bar-head")).toContainText(A.slice(0, 7));
});

test("the strip counts the take down, and Cancel is the punch's own and changes nothing", async ({
  app,
}) => {
  const page = await bootProject(app, punchCommands("held"));
  await dragSelect(app, 0.2, 0.5);
  await page.getByTestId("punch-btn").click();
  await page.getByTestId("punch-start").click();

  await app.emit("tool-progress", { kind: "punch_record", phase: "preroll", remaining_sec: 1.5 });
  await expect(page.getByTestId("tool-progress-count")).toHaveText(
    "Lead-in — recording starts in 2s",
  );
  await app.emit("tool-progress", { kind: "punch_record", phase: "recording", remaining_sec: 4 });
  await expect(page.getByTestId("tool-progress-count")).toHaveText("Recording — 4s left");
  // The control and Record are off while it runs.
  await expect(page.getByTestId("punch-btn")).toBeDisabled();
  await expect(page.getByTestId("record-btn")).toBeDisabled();

  await page.getByTestId("tool-progress-cancel").click();
  await expect.poll(async () => (await app.requestsFor("cancel_punch_record")).length).toBe(1);
  expect(await app.requestsFor("cancel_long_running_tool")).toEqual([]);

  await app.release("punch", { cancelled: true });
  await app.emit("tool-progress", { kind: "punch_record", done: true });
  await expect(page.getByTestId("tool-progress")).toHaveCount(0);
  await expect(page.getByTestId("punch-btn")).toBeEnabled();
  await expect(page.getByTestId("status-bar-head")).toContainText(A.slice(0, 7));
  await expect(page.getByTestId("render-error")).toHaveCount(0);
  await expect(page.getByTestId("punch-notice")).toHaveCount(0);
});

test("Cancel while preparing the monitor uses the punch command and changes nothing", async ({
  app,
}) => {
  const page = await bootProject(app, punchCommands("held"));
  await dragSelect(app, 0.2, 0.5);
  await page.getByTestId("punch-btn").click();
  await page.getByTestId("punch-start").click();

  await app.emit("tool-progress", { kind: "punch_record", phase: "preparing" });
  await expect(page.getByTestId("tool-progress-count")).toHaveText(
    "Preparing the monitor…",
  );
  await expect(page.getByTestId("tool-progress-cancel")).toBeVisible();
  await expect(page.getByTestId("punch-btn")).toBeDisabled();
  await expect(page.getByTestId("record-btn")).toBeDisabled();

  await page.getByTestId("tool-progress-cancel").click();
  await expect.poll(async () => (await app.requestsFor("cancel_punch_record")).length).toBe(1);
  expect(await app.requestsFor("cancel_long_running_tool")).toEqual([]);

  await app.release("punch", { cancelled: true });
  await app.emit("tool-progress", { kind: "punch_record", done: true });
  await expect(page.getByTestId("tool-progress")).toHaveCount(0);
  await expect(page.getByTestId("punch-btn")).toBeEnabled();
  await expect(page.getByTestId("record-btn")).toBeEnabled();
  await expect(page.getByTestId("status-bar-head")).toContainText(A.slice(0, 7));
  await expect(page.getByTestId("render-error")).toHaveCount(0);
  await expect(page.getByTestId("punch-notice")).toHaveCount(0);
});

test("a typed delay is one call and is shown as entered by hand", async ({ app }) => {
  const page = await bootProject(app, {
    get_recording_latency: ok(recordingLatencyStatus(null)),
    ...manualLatencyCommands(62.5),
  });
  await page.getByTestId("open-settings-button").click();
  await page.getByTestId("settings-tab-audio").click();
  await page.getByTestId("recording-latency-manual-input").fill("62.5");
  await page.getByTestId("recording-latency-manual-save").click();
  await expect(page.getByTestId("recording-latency-value")).toContainText(
    "62.5 ms (entered by hand)",
  );
  expect(await app.requestsFor("set_recording_latency")).toEqual([{ latencyMs: 62.5 }]);
});
