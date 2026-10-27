/**
 * The rules and words behind the Punch control (#203): when it is on, what
 * the latency line says, what became of a punch, and what the progress
 * strip reads. Pure functions, so none of it needs the application mounted.
 */

import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

import {
  UNCALIBRATED_LINE,
  UNCALIBRATED_MARKER,
  appliedText,
  isUncalibratedMessage,
  latencyLine,
  punchDisabledReason,
  punchOutcome,
  punchProgressText,
  regionText,
  trackLabel,
  type PunchGate,
} from "../lib/punch";
import type { PunchResult, RecordingLatency } from "../lib/tauri-bridge";

const DEVICES = {
  host: "ALSA",
  output: "Speakers",
  input: "Microphone",
  out_sr: 48_000,
  in_sr: 44_100,
};

const READY: PunchGate = {
  hasProject: true,
  trackCount: 2,
  selection: { start: 12.3, end: 14.8 },
  isRecording: false,
  punching: false,
};

describe("when the Punch control is on", () => {
  it("is on with a region selected and a track to put it on", () => {
    expect(punchDisabledReason(READY)).toBeNull();
  });

  it("is off with no selection, and says to select the region", () => {
    expect(punchDisabledReason({ ...READY, selection: null })).toMatch(/select the region/i);
  });

  it("is off for a selection with no length", () => {
    expect(punchDisabledReason({ ...READY, selection: { start: 3, end: 3 } })).toMatch(
      /select the region/i,
    );
    expect(punchDisabledReason({ ...READY, selection: { start: 4, end: 3 } })).toMatch(
      /select the region/i,
    );
  });

  it("is off with no track, or no project, and says to load one", () => {
    expect(punchDisabledReason({ ...READY, trackCount: 0 })).toMatch(/load a track/i);
    expect(punchDisabledReason({ ...READY, hasProject: false })).toMatch(/load a track/i);
  });

  it("is off while a take is being recorded", () => {
    expect(punchDisabledReason({ ...READY, isRecording: true })).toMatch(/stop the recording/i);
  });

  it("is off while a punch-in is running, whatever else is true", () => {
    expect(punchDisabledReason({ ...READY, punching: true })).toMatch(/punch-in is running/i);
    expect(
      punchDisabledReason({ ...READY, punching: true, selection: null, isRecording: true }),
    ).toMatch(/punch-in is running/i);
  });
});

describe("the latency line", () => {
  const measured: RecordingLatency = {
    devices: DEVICES,
    problem: null,
    latency: { latency_ms: 42.46, spread_ms: 0.31, measured_at: 1_000, source: "measured" },
  };

  it("says Not calibrated, pointing at Settings → Audio, when nothing is kept", () => {
    const line = latencyLine({ devices: DEVICES, problem: null, latency: null });
    expect(line).toEqual({ text: UNCALIBRATED_LINE, ready: false });
    expect(UNCALIBRATED_LINE).toBe("Not calibrated — Settings → Audio");
  });

  it("shows a measured delay, when it was measured, and is ready", () => {
    const line = latencyLine(measured, 1_000 + 7_200);
    expect(line.ready).toBe(true);
    expect(line.text).toContain("42.5 ms");
    expect(line.text).toContain("2 hours ago");
  });

  it("shows a typed delay as typed, and is ready: it counts as calibrated", () => {
    const typed: RecordingLatency = {
      devices: DEVICES,
      problem: null,
      latency: { latency_ms: 60, measured_at: 1_000, source: "manual" },
    };
    const line = latencyLine(typed);
    expect(line.ready).toBe(true);
    expect(line.text).toContain("60 ms");
    expect(line.text).toContain("entered by hand");
    expect(line.text).not.toContain("measured");
  });

  it("is not ready, and says why, with no devices or before it is read", () => {
    expect(latencyLine({ devices: null, problem: "no input device available", latency: null }))
      .toEqual({ text: "no input device available", ready: false });
    expect(latencyLine(undefined).ready).toBe(false);
    expect(latencyLine(undefined).text).toMatch(/checking/i);
  });

  it("treats an unreadable answer as not calibrated rather than guessing", () => {
    expect(latencyLine("failed")).toEqual({ text: UNCALIBRATED_LINE, ready: false });
  });
});

describe("what became of a punch", () => {
  const applied: PunchResult = {
    cancelled: false,
    head: "abc123",
    latency_ms: 75.34,
    latency_source: "measured",
    track: 0,
    start_sec: 1,
    end_sec: 2,
  };

  it("is applied, with the new head and the delay it was placed with", async () => {
    const o = await punchOutcome(async () => applied);
    expect(o).toEqual({ kind: "applied", head: "abc123", latencyMs: 75.34, source: "measured" });
    if (o.kind !== "applied") throw new Error("not applied");
    expect(appliedText(o)).toBe("Punched in, placed with 75.3 ms of latency (measured).");
    expect(appliedText({ ...o, latencyMs: 60, source: "manual" })).toBe(
      "Punched in, placed with 60 ms of latency (entered by hand).",
    );
  });

  it("is cancelled when the backend says so, which is not a failure", async () => {
    expect(await punchOutcome(async () => ({ cancelled: true }))).toEqual({ kind: "cancelled" });
  });

  it("is uncalibrated when the backend refuses for want of a delay", async () => {
    const message =
      "No recording latency is kept for these speakers and this microphone, so a take would " +
      "land late. Open Settings, then Audio, and either press Measure or type the delay.";
    const o = await punchOutcome(async () => {
      throw message;
    });
    expect(o).toEqual({ kind: "uncalibrated", message });
  });

  it("is failed, with the backend's own words, for any other refusal", async () => {
    for (const thrown of [
      "A latency measurement is running. Wait for it to finish.",
      new Error("the audio device dropped out during the take"),
    ]) {
      const o = await punchOutcome(async () => {
        throw thrown;
      });
      expect(o.kind).toBe("failed");
      expect((o as { message: string }).message).toBe(
        thrown instanceof Error ? thrown.message : thrown,
      );
    }
  });

  it("is failed if the backend reports success without a new version", async () => {
    const o = await punchOutcome(async () => ({ ...applied, head: "" }));
    expect(o.kind).toBe("failed");
  });

  it("recognises the backend's refusal by the sentence the Rust source holds", () => {
    // The one place the window looks inside a backend message: this fails if
    // the sentence is reworded on one side only.
    const src = readFileSync(
      resolve(__dirname, "../../../../crates/recorder/src/punch.rs"),
      "utf8",
    );
    expect(src).toContain(UNCALIBRATED_MARKER);
    expect(isUncalibratedMessage(UNCALIBRATED_MARKER + " for these")).toBe(true);
    expect(isUncalibratedMessage("the audio device dropped out")).toBe(false);
  });
});

describe("the progress strip's words", () => {
  it("counts the lead-in down to the recording", () => {
    expect(punchProgressText({ phase: "preroll", remaining_sec: 1.2 })).toBe(
      "Lead-in — recording starts in 2s",
    );
  });

  it("says Recording, and how long is left, inside the region", () => {
    expect(punchProgressText({ phase: "recording", remaining_sec: 2.4 })).toBe(
      "Recording — 3s left",
    );
  });

  it("says Tail, then Finishing, after the region", () => {
    expect(punchProgressText({ phase: "postroll", remaining_sec: 0.6 })).toBe("Tail — 1s");
    expect(punchProgressText({ phase: "postroll", remaining_sec: 0 })).toBe("Finishing…");
  });

  it("says the monitor is being prepared before the first phase", () => {
    expect(punchProgressText({ phase: "preparing" })).toBe("Preparing the monitor…");
    expect(punchProgressText({})).toBe("Preparing the monitor…");
  });
});

describe("the popover's smaller words", () => {
  it("writes the region with its length", () => {
    expect(regionText({ start: 12.3, end: 14.8 })).toBe("0:12.3 – 0:14.8 (2.5 s)");
    expect(regionText({ start: 61, end: 125.5 })).toBe("1:01.0 – 2:05.5 (64.5 s)");
  });

  it("numbers the tracks and never shows a blank name", () => {
    expect(trackLabel("Vocal", 0)).toBe("1. Vocal");
    expect(trackLabel("  ", 2)).toBe("3. Untitled");
  });
});
