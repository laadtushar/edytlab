/**
 * The Punch control and its popover (#203): on only with a region and a
 * track and no take running; says "Not calibrated" and keeps Start off
 * with no delay kept; starts a punch with the track and lead-in chosen.
 */

import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { RecordingLatency } from "../lib/tauri-bridge";

const getMock = vi.fn();

vi.mock("../lib/tauri-bridge", () => ({
  getRecordingLatency: () => getMock(),
}));

import { AppHeader } from "../components/AppHeader";
import { held } from "./held";

const DEVICES = {
  host: "ALSA",
  output: "Speakers",
  input: "Microphone",
  out_sr: 48_000,
  in_sr: 44_100,
};

const UNMEASURED: RecordingLatency = { devices: DEVICES, problem: null, latency: null };
const MEASURED: RecordingLatency = {
  devices: DEVICES,
  problem: null,
  latency: { latency_ms: 42.5, spread_ms: 0.3, measured_at: 1_000, source: "measured" },
};
const TYPED: RecordingLatency = {
  devices: DEVICES,
  problem: null,
  latency: { latency_ms: 60, measured_at: 1_000, source: "manual" },
};

function header(
  props: {
    selection?: { start: number; end: number } | null;
    tracks?: { name: string }[];
    isRecording?: boolean;
    punching?: boolean;
    timerArmed?: boolean;
    hasProject?: boolean;
  } = {},
) {
  const onPunch = vi.fn();
  const onOpenAudioSettings = vi.fn();
  const {
    selection = { start: 12.3, end: 14.8 },
    tracks = [{ name: "Vocal" }, { name: "Guitar" }],
    isRecording = false,
    punching = false,
    timerArmed = false,
    hasProject = true,
  } = props;
  const view = render(
    <AppHeader
      leftView="timeline"
      onSelectView={vi.fn()}
      onOpen={vi.fn()}
      onSettings={vi.fn()}
      isRecording={isRecording}
      onRecord={vi.fn()}
      onTimerRecord={vi.fn()}
      hasProject={hasProject}
      punch={{ tracks, selection, punching, timerArmed, onPunch, onOpenAudioSettings }}
    />,
  );
  return { onPunch, onOpenAudioSettings, ...view };
}

async function openPopover(status: RecordingLatency = MEASURED) {
  getMock.mockResolvedValue(status);
  fireEvent.click(screen.getByTestId("punch-btn"));
  await waitFor(() => expect(getMock).toHaveBeenCalled());
  await waitFor(() =>
    expect(screen.getByTestId("punch-latency").textContent).not.toMatch(/checking/i),
  );
}

describe("the Punch button", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("is on with a selection and a track", () => {
    header();
    expect(screen.getByTestId("punch-btn")).toBeEnabled();
  });

  it("is off with no selection, and its title says to select the region", () => {
    header({ selection: null });
    const b = screen.getByTestId("punch-btn");
    expect(b).toBeDisabled();
    expect(b.getAttribute("title")).toMatch(/select the region/i);
  });

  it("is off with no track", () => {
    header({ tracks: [] });
    expect(screen.getByTestId("punch-btn")).toBeDisabled();
  });

  it("is off while a take is being recorded, and the timer and Record are as they were", () => {
    header({ isRecording: true });
    expect(screen.getByTestId("punch-btn")).toBeDisabled();
    expect(screen.getByTestId("record-btn")).toBeEnabled();
    expect(screen.getByTestId("timer-record-btn")).toBeDisabled();
  });

  it("is off while a scheduled recording is armed, with its own reason", () => {
    header({ isRecording: true, timerArmed: true });
    const b = screen.getByTestId("punch-btn");
    expect(b).toBeDisabled();
    expect(b.getAttribute("title")).toMatch(/scheduled recording/i);
  });

  it("is off while a punch-in runs, and takes Record and the timer with it", () => {
    header({ punching: true });
    expect(screen.getByTestId("punch-btn")).toBeDisabled();
    expect(screen.getByTestId("record-btn")).toBeDisabled();
    expect(screen.getByTestId("timer-record-btn")).toBeDisabled();
  });

  it("is not drawn for a header given no punch (every other caller)", () => {
    render(
      <AppHeader
        leftView="timeline"
        onSelectView={vi.fn()}
        onOpen={vi.fn()}
        onSettings={vi.fn()}
        isRecording={false}
        onRecord={vi.fn()}
      />,
    );
    expect(screen.queryByTestId("punch-btn")).toBeNull();
    expect(getMock).not.toHaveBeenCalled();
  });

  it("asks nothing of the devices until the popover is opened", () => {
    header();
    expect(getMock).not.toHaveBeenCalled();
  });
});

describe("the popover", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("says Not calibrated with no delay kept, points at Settings → Audio and keeps Start off", async () => {
    const { onOpenAudioSettings, onPunch } = header();
    await openPopover(UNMEASURED);
    expect(screen.getByTestId("punch-latency").textContent).toBe(
      "Not calibrated — Settings → Audio",
    );
    expect(screen.getByTestId("punch-start")).toBeDisabled();
    fireEvent.click(screen.getByTestId("punch-start"));
    expect(onPunch).not.toHaveBeenCalled();

    fireEvent.click(screen.getByTestId("punch-open-settings"));
    expect(onOpenAudioSettings).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId("punch-panel")).toBeNull();
  });

  it("shows the region and the delay it will be placed with, and Start is on", async () => {
    header();
    await openPopover(MEASURED);
    expect(screen.getByTestId("punch-region").textContent).toBe("0:12.3 – 0:14.8 (2.5 s)");
    const line = screen.getByTestId("punch-latency").textContent ?? "";
    expect(line).toContain("42.5 ms");
    expect(screen.getByTestId("punch-start")).toBeEnabled();
    expect(screen.queryByTestId("punch-open-settings")).toBeNull();
  });

  it("counts a typed delay as calibrated", async () => {
    header();
    await openPopover(TYPED);
    expect(screen.getByTestId("punch-latency").textContent).toContain("entered by hand");
    expect(screen.getByTestId("punch-start")).toBeEnabled();
  });

  it("starts with the first track and a lead-in of two seconds by default", async () => {
    const { onPunch } = header();
    await openPopover();
    fireEvent.click(screen.getByTestId("punch-start"));
    expect(onPunch).toHaveBeenCalledWith({ track: 0, prerollSec: 2 });
    expect(screen.queryByTestId("punch-panel")).toBeNull();
  });

  it("starts with the track and the lead-in chosen", async () => {
    const { onPunch } = header();
    await openPopover();
    fireEvent.change(screen.getByTestId("punch-track"), { target: { value: "1" } });
    fireEvent.change(screen.getByTestId("punch-preroll"), { target: { value: "3.5" } });
    fireEvent.click(screen.getByTestId("punch-start"));
    expect(onPunch).toHaveBeenCalledWith({ track: 1, prerollSec: 3.5 });
  });

  it("keeps Start off for a lead-in that is not a time, and says so", async () => {
    header();
    await openPopover();
    for (const bad of ["-1", "31", "1e9"]) {
      fireEvent.change(screen.getByTestId("punch-preroll"), { target: { value: bad } });
      expect(screen.getByTestId("punch-start")).toBeDisabled();
      expect(screen.getByRole("alert").textContent).toMatch(/0 to 30 seconds/);
    }
    fireEvent.change(screen.getByTestId("punch-preroll"), { target: { value: "" } });
    expect(screen.getByTestId("punch-start")).toBeEnabled();
  });

  it("keeps Start off until the delay has been read, and an unreadable one is not guessed", async () => {
    header();
    const answer = held<RecordingLatency>();
    getMock.mockReturnValue(answer.promise);
    fireEvent.click(screen.getByTestId("punch-btn"));
    expect(screen.getByTestId("punch-latency").textContent).toMatch(/checking/i);
    expect(screen.getByTestId("punch-start")).toBeDisabled();
    await answer.reject("the latency file is from a newer version");
    expect(screen.getByTestId("punch-latency").textContent).toBe(
      "Not calibrated — Settings → Audio",
    );
    expect(screen.getByTestId("punch-start")).toBeDisabled();
  });

  it("reads the delay again each time it opens, because the devices may have changed", async () => {
    header();
    await openPopover(MEASURED);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByTestId("punch-panel")).toBeNull();
    await openPopover(UNMEASURED);
    expect(getMock).toHaveBeenCalledTimes(2);
    expect(screen.getByTestId("punch-latency").textContent).toMatch(/not calibrated/i);
  });

  it("closes on a click outside", async () => {
    header();
    await openPopover();
    fireEvent.mouseDown(document.body);
    expect(screen.queryByTestId("punch-panel")).toBeNull();
  });

  it("closes itself when the selection is cleared under it", async () => {
    const { rerender } = header();
    await openPopover();
    expect(screen.getByTestId("punch-panel")).toBeTruthy();
    await act(async () => {
      rerender(
        <AppHeader
          leftView="timeline"
          onSelectView={vi.fn()}
          onOpen={vi.fn()}
          onSettings={vi.fn()}
          isRecording={false}
          onRecord={vi.fn()}
          hasProject
          punch={{
            tracks: [{ name: "Vocal" }],
            selection: null,
            punching: false,
            onPunch: vi.fn(),
          }}
        />,
      );
    });
    expect(screen.queryByTestId("punch-panel")).toBeNull();
    expect(screen.getByTestId("punch-btn")).toBeDisabled();
  });

  it("warns that a long region can drift", async () => {
    header({ selection: { start: 0, end: 90 } });
    await openPopover();
    expect(screen.getByTestId("punch-long").textContent).toMatch(/drift/i);
  });

  it("does not warn for a short one", async () => {
    header();
    await openPopover();
    expect(screen.queryByTestId("punch-long")).toBeNull();
  });
});
