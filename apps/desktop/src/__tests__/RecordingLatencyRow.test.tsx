/**
 * The recording latency row in Settings → Audio (#203).
 *
 * One test per state the backend can leave it in (nothing measured, a value
 * kept, no devices, a measurement running, a refusal) and for the rules
 * around them: nothing plays until Measure is pressed, a refusal is shown in
 * place and leaves the old value alone, Cancel is not an error, and the
 * progress the backend reports is the progress shown. The bridge is mocked;
 * the `tool-progress` handler the row registers is kept and called by hand.
 */

import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { StrictMode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { RecordingLatency, ToolProgress } from "../lib/tauri-bridge";

const getMock = vi.fn();
const calibrateMock = vi.fn();
const cancelMock = vi.fn();
const onProgressMock = vi.fn();

vi.mock("../lib/tauri-bridge", () => ({
  getRecordingLatency: () => getMock(),
  calibrateRecordingLatency: () => calibrateMock(),
  cancelLongRunningTool: () => cancelMock(),
  onToolProgress: (cb: (p: ToolProgress) => void) => onProgressMock(cb),
}));

import { RecordingLatencyRow } from "../components/RecordingLatencyRow";
import { SETUP_LINE } from "../lib/recordingLatency";
import { held } from "./held";

const DEVICES = {
  host: "ALSA",
  output: "Speakers (Realtek)",
  input: "Microphone (USB)",
  out_sr: 48_000,
  in_sr: 44_100,
};

const NOTHING: RecordingLatency = { devices: DEVICES, problem: null, latency: null };
const MEASURED: RecordingLatency = {
  devices: DEVICES,
  problem: null,
  latency: {
    latency_ms: 42.46,
    spread_ms: 0.31,
    measured_at: Math.floor(Date.now() / 1000) - 7_200,
    source: "measured",
  },
};

let emit: (p: ToolProgress) => void;

async function renderRow(status: RecordingLatency | Promise<RecordingLatency> = NOTHING) {
  getMock.mockReturnValue(Promise.resolve(status));
  const view = render(<RecordingLatencyRow />);
  await waitFor(() => expect(getMock).toHaveBeenCalled());
  // Wait on what the row draws, not on the call: its answer lands after
  // the call, and React draws it later still.
  await waitFor(() =>
    expect(screen.getByTestId("recording-latency-value").textContent).not.toBe("Checking…"),
  );
  return view;
}

describe("RecordingLatencyRow", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    onProgressMock.mockImplementation((cb: (p: ToolProgress) => void) => {
      emit = cb;
      return Promise.resolve(() => {});
    });
    cancelMock.mockResolvedValue(undefined);
  });

  it("says nothing is measured yet, names the devices and shows the setup line", async () => {
    await renderRow();
    expect(screen.getByTestId("recording-latency-value").textContent).toMatch(/not measured/i);
    expect(screen.getByTestId("recording-latency-devices").textContent).toContain(
      "Speakers (Realtek)",
    );
    expect(screen.getByTestId("recording-latency-setup").textContent).toBe(SETUP_LINE);
    expect(screen.getByTestId("recording-latency-measure").textContent).toBe("Measure");
  });

  it("plays nothing until Measure is pressed", async () => {
    await renderRow();
    expect(calibrateMock).not.toHaveBeenCalled();
  });

  it("shows a kept value with how steady it was and how long ago", async () => {
    await renderRow(MEASURED);
    const text = screen.getByTestId("recording-latency-value").textContent ?? "";
    expect(text).toContain("42.5 ms");
    expect(text).toContain("steady to 0.3 ms");
    expect(text).toContain("2 hours ago");
    expect(screen.getByTestId("recording-latency-measure").textContent).toBe("Measure again");
  });

  it("with no speakers or microphone, gives the reason and offers nothing to press", async () => {
    await renderRow({ devices: null, problem: "no input device available", latency: null });
    expect(screen.getByTestId("recording-latency-value").textContent).toMatch(
      /no input device available/,
    );
    expect(screen.queryByTestId("recording-latency-measure")).toBeNull();
    expect(screen.queryByTestId("recording-latency-setup")).toBeNull();
  });

  it("measures, shows the run being played, and then the new value", async () => {
    const user = userEvent.setup();
    await renderRow();
    const answer = held<{ cancelled: false } & Record<string, unknown>>();
    calibrateMock.mockReturnValue(answer.promise);

    await user.click(screen.getByTestId("recording-latency-measure"));
    expect(screen.getByTestId("recording-latency-measure")).toBeDisabled();
    expect(screen.getByTestId("recording-latency-value").textContent).toMatch(/Measuring…$/);

    act(() => emit({ kind: "calibrate_latency", run: 2, runs: 3 }));
    expect(screen.getByTestId("recording-latency-value").textContent).toBe(
      "Measuring… run 2 of 3",
    );
    // Progress of other jobs on the same channel is not this row's.
    act(() => emit({ kind: "transcribe", index: 1, total: 4 }));
    expect(screen.getByTestId("recording-latency-value").textContent).toBe(
      "Measuring… run 2 of 3",
    );

    await answer.resolve({
      cancelled: false,
      devices: DEVICES,
      latency: {
        latency_ms: 75.3,
        spread_ms: 0.1,
        measured_at: Math.floor(Date.now() / 1000),
        source: "measured",
      },
      sweeps_heard: 15,
      runs: 3,
    });
    const text = screen.getByTestId("recording-latency-value").textContent ?? "";
    expect(text).toContain("75.3 ms");
    expect(text).toContain("just now");
    expect(screen.getByTestId("recording-latency-measure")).toBeEnabled();
  });

  it("shows a refusal in place, with its advice, and keeps the value that was there", async () => {
    const user = userEvent.setup();
    await renderRow(MEASURED);
    calibrateMock.mockRejectedValue(
      "heard 2 of 5 test sweeps. Turn the speakers up, or hold the headphones against the microphone, and try again in a quiet room",
    );
    await user.click(screen.getByTestId("recording-latency-measure"));

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toMatch(/heard 2 of 5/);
    expect(alert.textContent).toMatch(/hold the headphones against the microphone/);
    expect(screen.getByTestId("recording-latency-value").textContent).toContain("42.5 ms");
    expect(screen.getByTestId("recording-latency-measure")).toBeEnabled();
  });

  it("clears the refusal when the next measurement starts", async () => {
    const user = userEvent.setup();
    await renderRow();
    calibrateMock.mockRejectedValueOnce("heard 0 of 5 test sweeps");
    await user.click(screen.getByTestId("recording-latency-measure"));
    await screen.findByRole("alert");

    const answer = held<{ cancelled: true }>();
    calibrateMock.mockReturnValue(answer.promise);
    await user.click(screen.getByTestId("recording-latency-measure"));
    expect(screen.queryByRole("alert")).toBeNull();
    await answer.resolve({ cancelled: true });
  });

  it("Cancel asks the backend to stop, and a cancelled run changes nothing and says nothing", async () => {
    const user = userEvent.setup();
    await renderRow(MEASURED);
    const answer = held<{ cancelled: true }>();
    calibrateMock.mockReturnValue(answer.promise);
    await user.click(screen.getByTestId("recording-latency-measure"));

    await user.click(screen.getByTestId("recording-latency-cancel"));
    expect(cancelMock).toHaveBeenCalledTimes(1);

    await answer.resolve({ cancelled: true });
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByTestId("recording-latency-value").textContent).toContain("42.5 ms");
    expect(screen.queryByTestId("recording-latency-cancel")).toBeNull();
  });

  it("says it could not read the latency when the backend refuses to", async () => {
    getMock.mockRejectedValue("the latency file is from a newer version of the app");
    render(<RecordingLatencyRow />);
    await waitFor(() =>
      expect(screen.getByTestId("recording-latency-value")).toHaveTextContent(
        /could not read.*newer version/i,
      ),
    );
  });

  it("lets go of its progress listener, also under StrictMode's second mount", async () => {
    const unlisten = vi.fn();
    onProgressMock.mockImplementation((cb: (p: ToolProgress) => void) => {
      emit = cb;
      return Promise.resolve(unlisten);
    });
    getMock.mockResolvedValue(NOTHING);
    const view = render(
      <StrictMode>
        <RecordingLatencyRow />
      </StrictMode>,
    );
    await act(async () => {});
    view.unmount();
    await act(async () => {});
    // Every listener that was registered was let go.
    expect(unlisten).toHaveBeenCalledTimes(onProgressMock.mock.calls.length);
  });
});
