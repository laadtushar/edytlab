/**
 * The webview's half of the opt-in crash reports (#958): what an uncaught
 * error hands to the backend, how often, and the Settings switch.
 *
 * The consent check is the backend's (`crash_reports.rs` pins that nothing
 * is built or sent while the switch is off, or in a build with no DSN); the
 * window only forwards, and these tests pin that it forwards an error's
 * name, message and stack and nothing else, and that the switch reads and
 * writes the backend's choice and starts off.
 */

import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const getCrashReportsMock = vi.fn();
const setCrashReportsMock = vi.fn();
const reportWebviewErrorMock = vi.fn();

vi.mock("../lib/tauri-bridge", () => ({
  getCrashReports: () => getCrashReportsMock(),
  setCrashReports: (enabled: boolean) => setCrashReportsMock(enabled),
  reportWebviewError: (report: unknown) => reportWebviewErrorMock(report),
}));

import { CrashReportsRow } from "../components/CrashReportsRow";
import { MAX_REPORTS, forwardWebviewErrors, reportFrom } from "../lib/crashReports";
import { CRASH_REPORTS_COPY } from "../lib/crashReportsCopy";

describe("forwarding uncaught errors", () => {
  let stop: () => void = () => undefined;
  const send = vi.fn(() => Promise.resolve());

  beforeEach(() => {
    stop();
    send.mockClear();
    stop = forwardWebviewErrors(window, send);
  });

  const raise = (error: unknown, message = "") =>
    window.dispatchEvent(new ErrorEvent("error", { error, message }));

  const reject = (reason: unknown) => {
    const event = new Event("unhandledrejection") as Event & { reason?: unknown };
    event.reason = reason;
    window.dispatchEvent(event);
  };

  it("hands over the name, message and stack of an uncaught error", () => {
    const error = new TypeError("x is undefined");
    error.stack = "TypeError: x is undefined\n    at render (tauri://localhost/assets/a.js:1:2)";
    raise(error, "Uncaught TypeError: x is undefined");
    expect(send).toHaveBeenCalledTimes(1);
    expect(send).toHaveBeenCalledWith({
      kind: "error",
      name: "TypeError",
      message: "x is undefined",
      stack: error.stack,
    });
  });

  it("hands over an unhandled rejection, a thrown string as its message", () => {
    reject("plain words");
    expect(send).toHaveBeenCalledWith({
      kind: "unhandledrejection",
      name: "Error",
      message: "plain words",
      stack: null,
    });
  });

  it("forwards each distinct error once, and no more than the cap in a launch", () => {
    raise(new Error("same"));
    raise(new Error("same"));
    expect(send).toHaveBeenCalledTimes(1);
    for (let i = 0; i < MAX_REPORTS + 10; i++) raise(new Error(`error ${i}`));
    expect(send).toHaveBeenCalledTimes(MAX_REPORTS);
  });

  it("never throws when the backend cannot be reached", () => {
    stop();
    stop = forwardWebviewErrors(window, () => {
      throw new Error("no Tauri here");
    });
    expect(() => raise(new Error("boom"))).not.toThrow();
    stop();
    stop = forwardWebviewErrors(window, () => Promise.reject(new Error("refused")));
    expect(() => raise(new Error("boom again"))).not.toThrow();
  });

  it("reads what a non-error reason says about itself", () => {
    expect(reportFrom("error", undefined, "Script error.")).toEqual({
      kind: "error",
      name: "Error",
      message: "Script error.",
      stack: null,
    });
    expect(reportFrom("unhandledrejection", { toString: () => "an object" }).message).toBe("an object");
  });
});

describe("the Send crash reports switch", () => {
  beforeEach(() => {
    getCrashReportsMock.mockReset();
    setCrashReportsMock.mockReset();
  });

  it("starts off, and says what a report holds and what is removed", async () => {
    getCrashReportsMock.mockResolvedValue({ enabled: false, available: true });
    render(<CrashReportsRow />);
    const toggle = await screen.findByTestId("settings-crash-reports-toggle");
    await waitFor(() => expect(toggle).not.toBeDisabled());
    expect(toggle).not.toBeChecked();
    expect(screen.getByLabelText(CRASH_REPORTS_COPY.label)).toBe(toggle);
    const row = screen.getByTestId("settings-crash-reports");
    expect(row.textContent).toContain("Off unless you turn it on");
    expect(row.textContent).toContain("Sentry");
    expect(row.textContent).toContain("paths");
    expect(row.textContent).toContain("API keys");
    expect(row.textContent).toContain("email addresses");
    expect(row.textContent).toContain("never contains audio");
  });

  it("turns reporting on and off through the backend", async () => {
    getCrashReportsMock.mockResolvedValue({ enabled: false, available: true });
    setCrashReportsMock.mockImplementation((enabled: boolean) => Promise.resolve({ enabled, available: true }));
    render(<CrashReportsRow />);
    const toggle = await screen.findByTestId("settings-crash-reports-toggle");
    await waitFor(() => expect(toggle).not.toBeDisabled());

    await act(async () => {
      fireEvent.click(toggle);
    });
    expect(setCrashReportsMock).toHaveBeenLastCalledWith(true);
    await waitFor(() => expect(toggle).toBeChecked());

    await act(async () => {
      fireEvent.click(toggle);
    });
    expect(setCrashReportsMock).toHaveBeenLastCalledWith(false);
    await waitFor(() => expect(toggle).not.toBeChecked());
  });

  it("is off and disabled in a build that cannot send, and says so", async () => {
    getCrashReportsMock.mockResolvedValue({ enabled: true, available: false });
    render(<CrashReportsRow />);
    expect(await screen.findByTestId("settings-crash-reports-unavailable")).toHaveTextContent(
      CRASH_REPORTS_COPY.unavailable,
    );
    const toggle = screen.getByTestId("settings-crash-reports-toggle");
    expect(toggle).toBeDisabled();
    expect(toggle).not.toBeChecked();
  });

  it("shows a save that failed, and what is in force after it", async () => {
    getCrashReportsMock.mockResolvedValueOnce({ enabled: false, available: true });
    setCrashReportsMock.mockRejectedValue(
      new Error("could not save the crash reports setting (denied); it applies until edytlab quits"),
    );
    getCrashReportsMock.mockResolvedValueOnce({ enabled: true, available: true });
    render(<CrashReportsRow />);
    const toggle = await screen.findByTestId("settings-crash-reports-toggle");
    await waitFor(() => expect(toggle).not.toBeDisabled());
    await act(async () => {
      fireEvent.click(toggle);
    });
    expect(await screen.findByTestId("settings-crash-reports-error")).toHaveTextContent(
      "could not save the crash reports setting",
    );
    await waitFor(() => expect(toggle).toBeChecked());
  });
});
