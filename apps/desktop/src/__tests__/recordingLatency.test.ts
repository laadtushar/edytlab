/**
 * The words and outcomes behind Settings → Audio's recording latency row
 * (#203): what a measurement says about itself, and how a refusal reaches
 * the person.
 */

import { describe, expect, it } from "vitest";

import {
  ageText,
  calibrateOutcome,
  devicesText,
  latencyText,
  measuringText,
  rateText,
} from "../lib/recordingLatency";
import type { CalibrationResult, RecordingDevices } from "../lib/tauri-bridge";

const DEVICES: RecordingDevices = {
  host: "ALSA",
  output: "Speakers (Realtek)",
  input: "Microphone (USB)",
  out_sr: 48_000,
  in_sr: 44_100,
};

describe("latencyText", () => {
  it("shows a measured delay to a tenth of a millisecond, with how steady it was", () => {
    expect(
      latencyText({ latency_ms: 42.46, spread_ms: 0.31, measured_at: 0, source: "measured" }),
    ).toBe("42.5 ms (steady to 0.3 ms)");
  });

  it("shows a typed value as typed, with no steadiness to report", () => {
    expect(latencyText({ latency_ms: 120, measured_at: 0, source: "manual" })).toBe(
      "120 ms (entered by hand)",
    );
  });

  it("does not show a minus sign for a measurement that rounds to zero", () => {
    expect(
      latencyText({ latency_ms: -0.02, spread_ms: 0.1, measured_at: 0, source: "measured" }),
    ).toBe("0.0 ms (steady to 0.1 ms)");
  });
});

describe("ageText", () => {
  const now = 1_800_000_000;
  it.each([
    [now - 5, "just now"],
    [now - 90, "1 minute ago"],
    [now - 20 * 60, "20 minutes ago"],
    [now - 3_600, "1 hour ago"],
    [now - 5 * 3_600, "5 hours ago"],
    [now - 86_400, "1 day ago"],
    [now - 9 * 86_400, "9 days ago"],
  ])("%d is %s", (at, text) => {
    expect(ageText(at, now)).toBe(text);
  });

  it("calls a clock that is ahead 'just now' rather than showing a negative age", () => {
    expect(ageText(now + 600, now)).toBe("just now");
  });

  it("gives a date once it is more than a month old", () => {
    expect(ageText(now - 90 * 86_400, now)).toMatch(/^on \d{4}-\d{2}-\d{2}$/);
  });
});

describe("devicesText and rateText", () => {
  it("names the speakers and the microphone and the audio system", () => {
    expect(devicesText(DEVICES)).toBe(
      "Speakers: Speakers (Realtek) · Microphone: Microphone (USB) · ALSA",
    );
  });

  it("writes rates in kHz without trailing zeros", () => {
    expect(rateText(48_000)).toBe("48 kHz");
    expect(rateText(44_100)).toBe("44.1 kHz");
    expect(rateText(96_000)).toBe("96 kHz");
  });
});

describe("measuringText", () => {
  it("says which run is playing", () => {
    expect(measuringText({ kind: "calibrate_latency", run: 2, runs: 3 })).toBe(
      "Measuring… run 2 of 3",
    );
  });

  it("says only that it is measuring before the first run reports", () => {
    expect(measuringText(undefined)).toBe("Measuring…");
    expect(measuringText({ kind: "calibrate_latency" })).toBe("Measuring…");
  });
});

describe("calibrateOutcome", () => {
  const measured: CalibrationResult = {
    cancelled: false,
    devices: DEVICES,
    latency: { latency_ms: 42.5, spread_ms: 0.2, measured_at: 1_800_000_000, source: "measured" },
    sweeps_heard: 15,
    runs: 3,
  };

  it("hands back a measurement", async () => {
    expect(await calibrateOutcome(async () => measured)).toEqual({
      kind: "measured",
      latency: measured.latency,
      devices: DEVICES,
    });
  });

  it("treats Cancel as an outcome, not an error", async () => {
    expect(await calibrateOutcome(async () => ({ cancelled: true }))).toEqual({
      kind: "cancelled",
    });
  });

  it("passes the backend's reason through unchanged, because it already says what to do", async () => {
    const reason =
      "heard 2 of 5 test sweeps. Turn the speakers up, or hold the headphones against the microphone, and try again in a quiet room";
    expect(
      await calibrateOutcome(async () => {
        throw reason;
      }),
    ).toEqual({ kind: "refused", message: reason });
  });

  it("reads an Error's message", async () => {
    expect(
      await calibrateOutcome(async () => {
        throw new Error("no input device available");
      }),
    ).toEqual({ kind: "refused", message: "no input device available" });
  });
});
