/**
 * Scheduled recording, from the control to the countdown (#225 §4).
 *
 * `timer_record` shipped in #203 §2 with no caller. The feature exists
 * to run when nobody is at the machine, and that was the one thing you
 * could not ask it to do.
 */

import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { TimerRecord } from "../components/TimerRecord";
import {
  formatSeconds,
  scheduledTake,
  timerLabel,
} from "../lib/recording";

describe("arming a scheduled take", () => {
  function open(onArm = vi.fn()) {
    render(<TimerRecord onArm={onArm} />);
    fireEvent.click(screen.getByTestId("timer-record-btn"));
    return onArm;
  }

  it("refuses a schedule with neither half set", () => {
    open();
    // Mirrors `Schedule::is_armed`, which returns the same refusal —
    // catching it here turns a round-trip and an error banner into a
    // disabled button with the reason beside it.
    expect(screen.getByTestId("timer-record-arm")).toBeDisabled();
    expect(screen.getByTestId("timer-record-hint").textContent).toMatch(
      /delay, a duration, or both/i,
    );
  });

  it("arms with only a delay", () => {
    const onArm = open();
    fireEvent.change(screen.getByTestId("timer-start-min"), {
      target: { value: "10" },
    });
    fireEvent.click(screen.getByTestId("timer-record-arm"));

    expect(onArm).toHaveBeenCalledWith({
      startAfterSec: 600,
      durationSec: undefined,
    });
  });

  it("arms with only a duration", () => {
    const onArm = open();
    fireEvent.change(screen.getByTestId("timer-duration-sec"), {
      target: { value: "30" },
    });
    fireEvent.click(screen.getByTestId("timer-record-arm"));

    expect(onArm).toHaveBeenCalledWith({
      startAfterSec: undefined,
      durationSec: 30,
    });
  });

  it("combines minutes and seconds", () => {
    const onArm = open();
    fireEvent.change(screen.getByTestId("timer-start-min"), {
      target: { value: "1" },
    });
    fireEvent.change(screen.getByTestId("timer-start-sec"), {
      target: { value: "30" },
    });
    fireEvent.click(screen.getByTestId("timer-record-arm"));

    expect(onArm.mock.calls[0][0].startAfterSec).toBe(90);
  });

  it("treats a blank as unset, not as zero", () => {
    // A zero delay is a legitimate "start now, stop after N". Reading a
    // blank as 0 would arm that silently.
    const onArm = open();
    fireEvent.change(screen.getByTestId("timer-duration-sec"), {
      target: { value: "5" },
    });
    fireEvent.click(screen.getByTestId("timer-record-arm"));

    expect(onArm.mock.calls[0][0].startAfterSec).toBeUndefined();
  });

  it("does accept an explicit zero delay", () => {
    const onArm = open();
    fireEvent.change(screen.getByTestId("timer-start-sec"), {
      target: { value: "0" },
    });
    fireEvent.change(screen.getByTestId("timer-duration-sec"), {
      target: { value: "5" },
    });
    fireEvent.click(screen.getByTestId("timer-record-arm"));

    expect(onArm.mock.calls[0][0].startAfterSec).toBe(0);
  });

  it("refuses a negative value rather than sending it", () => {
    const onArm = open();
    fireEvent.change(screen.getByTestId("timer-start-min"), {
      target: { value: "-5" },
    });
    expect(screen.getByTestId("timer-record-arm")).toBeDisabled();
    expect(onArm).not.toHaveBeenCalled();
  });

  it("is unavailable while a take is already running", () => {
    render(<TimerRecord busy onArm={vi.fn()} />);
    // The recorder refuses a second take, and a control that does
    // nothing is worse than one that is not offered.
    expect(screen.getByTestId("timer-record-btn")).toBeDisabled();
  });

  it("closes on Escape", async () => {
    open();
    expect(screen.getByTestId("timer-record-panel")).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() =>
      expect(screen.queryByTestId("timer-record-panel")).toBeNull(),
    );
  });
});

describe("what became of a scheduled take", () => {
  const loaded = vi.fn(() => Promise.resolve({ last_node_id: "n1" }));

  it("imports the take and reports the new head", async () => {
    const out = await scheduledTake(
      () => Promise.resolve({ path: "/tmp/take.wav", cancelled: false }),
      loaded,
    );
    expect(out).toEqual({
      kind: "loaded",
      path: "/tmp/take.wav",
      nodeId: "n1",
    });
  });

  it("treats a cancel as an outcome, not a failure", async () => {
    // The user pressed Cancel. No take was meant to exist, so there is
    // nothing to report as an error.
    const out = await scheduledTake(
      () => Promise.resolve({ cancelled: true }),
      loaded,
    );
    expect(out).toEqual({ kind: "cancelled" });
  });

  it("names the path when the take was written but not imported", async () => {
    // The audio is on disk. Calling that lost would be wrong.
    const out = await scheduledTake(
      () => Promise.resolve({ path: "/tmp/take.wav", cancelled: false }),
      () => Promise.reject(new Error("decode failed")),
    );
    expect(out.kind).toBe("loadFailed");
    if (out.kind === "loadFailed") {
      expect(out.path).toBe("/tmp/take.wav");
      expect(out.message).toContain("/tmp/take.wav");
    }
  });

  it("reports a recorder failure", async () => {
    const out = await scheduledTake(
      () => Promise.reject(new Error("no input device")),
      loaded,
    );
    expect(out.kind).toBe("failed");
    if (out.kind === "failed") {
      expect(out.message).toContain("no input device");
    }
  });

  it("does not claim success when no file came back", async () => {
    const out = await scheduledTake(
      () => Promise.resolve({ cancelled: false }),
      loaded,
    );
    expect(out.kind).toBe("failed");
  });
});

describe("the countdown label", () => {
  it("counts down to the start before recording", () => {
    expect(timerLabel({ recording: false, remaining_sec: 9.2 })).toBe(
      "Recording starts in 10s",
    );
  });

  it("says it is recording, unmistakably", () => {
    // The feature exists for takes nobody is watching; this is the
    // line that says one is in progress.
    expect(timerLabel({ recording: true, remaining_sec: 12 })).toBe(
      "Recording — 12s left",
    );
  });

  it("handles a schedule with no end to count down to", () => {
    expect(timerLabel({ recording: true })).toMatch(/until you stop it/i);
  });

  it("rounds up, so a running timer never reads 0s", () => {
    expect(formatSeconds(0.2)).toBe("1s");
    expect(formatSeconds(0)).toBe("0s");
  });

  it("switches to m:ss past a minute", () => {
    expect(formatSeconds(61)).toBe("1:01");
    expect(formatSeconds(600)).toBe("10:00");
  });
});
