/**
 * The Settings line about the ONNX Runtime library (#383).
 *
 * One line, three states a user can be in today: a library found ("ONNX
 * Runtime: 1.23.2 (bundled)"), none in this build ("not installed in this
 * build", which every release build says until the installers carry it),
 * and one that cannot be used (a macOS that is too old, a file that will
 * not load). The wording is checked here, and that none of it claims a
 * model runs: a runtime that loads is not stem separation or
 * transcription, which this build does not ship (#384, #385).
 */

import { render, screen } from "@testing-library/react";
import { StrictMode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mlRuntimeStatusMock = vi.fn();

vi.mock("../lib/tauri-bridge", () => ({
  mlRuntimeStatus: () => mlRuntimeStatusMock(),
}));

import { MlRuntimeLine } from "../components/MlRuntimeLine";
import { ML_RUNTIME_PENDING, mlRuntimeLine } from "../lib/mlRuntimeLine";
import type { MlRuntimeStatus } from "../lib/tauri-bridge";
import { held } from "./held";

const BUNDLED: MlRuntimeStatus = {
  state: "found",
  version: "1.23.2",
  path: "/Applications/edytlab.app/Contents/Frameworks/libonnxruntime.dylib",
  source: "bundled",
};
const MISSING: MlRuntimeStatus = {
  state: "missing",
  searched: ["/usr/lib/edytlab/libonnxruntime.so", "/usr/bin/libonnxruntime.so"],
};
const TOO_OLD: MlRuntimeStatus = {
  state: "unsupportedOs",
  needs: "13.4",
  running: "12.6.1",
};
const BROKEN: MlRuntimeStatus = {
  state: "loadFailed",
  path: "/usr/lib/edytlab/libonnxruntime.so",
  reason: "it has no OrtGetApiBase entry point, so it is not an ONNX Runtime library",
};

describe("MlRuntimeLine", () => {
  beforeEach(() => {
    mlRuntimeStatusMock.mockReset();
  });

  it("names the version of a bundled runtime", async () => {
    mlRuntimeStatusMock.mockResolvedValue(BUNDLED);
    render(<MlRuntimeLine />);

    const line = await screen.findByText("ONNX Runtime: 1.23.2 (bundled)");
    expect(line).toHaveAttribute("data-testid", "settings-ml-runtime");
    expect(line).toHaveAttribute("data-state", "found");
    // The file is text under the line, not a hover: a `title` cannot be
    // reached from the keyboard or read by a screen reader.
    expect(screen.getByTestId("settings-ml-runtime-detail")).toHaveTextContent(
      BUNDLED.state === "found" ? BUNDLED.path : "",
    );
    expect(line).not.toHaveAttribute("title");
  });

  it("says a build with no runtime has none, and where it looked", async () => {
    mlRuntimeStatusMock.mockResolvedValue(MISSING);
    render(<MlRuntimeLine />);

    const line = await screen.findByText("ONNX Runtime: not installed in this build");
    expect(line).toHaveAttribute("data-state", "missing");
    const detail = screen.getByTestId("settings-ml-runtime-detail");
    expect(detail).toHaveTextContent("/usr/lib/edytlab/libonnxruntime.so");
    expect(detail).toHaveTextContent("/usr/bin/libonnxruntime.so");
  });

  it("reports a macOS that is too old and a library that will not load", async () => {
    mlRuntimeStatusMock.mockResolvedValue(TOO_OLD);
    const first = render(<MlRuntimeLine />);
    expect(
      await screen.findByText(
        "ONNX Runtime: needs macOS 13.4 or newer (this Mac is running 12.6.1)",
      ),
    ).toHaveAttribute("data-state", "unsupportedOs");
    expect(screen.queryByTestId("settings-ml-runtime-detail")).toBeNull();
    first.unmount();

    mlRuntimeStatusMock.mockResolvedValue(BROKEN);
    render(<MlRuntimeLine />);
    const line = await screen.findByText(/ONNX Runtime: could not be loaded/);
    expect(line).toHaveTextContent("it has no OrtGetApiBase entry point");
    expect(screen.getByTestId("settings-ml-runtime-detail")).toHaveTextContent(
      "/usr/lib/edytlab/libonnxruntime.so",
    );
  });

  it("shows the command's own error when it cannot be asked", async () => {
    mlRuntimeStatusMock.mockRejectedValue("internal: a command's task did not finish (x)");
    render(<MlRuntimeLine />);

    const line = await screen.findByText(/ONNX Runtime: could not be checked/);
    expect(line).toHaveTextContent("a command's task did not finish");
    expect(line).toHaveAttribute("data-state", "error");
  });

  it("says it is checking until the answer comes, and asks once", async () => {
    const answer = held<MlRuntimeStatus>();
    mlRuntimeStatusMock.mockReturnValue(answer.promise);
    render(<MlRuntimeLine />);

    expect(screen.getByTestId("settings-ml-runtime")).toHaveTextContent(ML_RUNTIME_PENDING);
    expect(screen.getByTestId("settings-ml-runtime")).toHaveAttribute("data-state", "pending");

    await answer.resolve(BUNDLED);
    expect(screen.getByTestId("settings-ml-runtime")).toHaveTextContent(
      "ONNX Runtime: 1.23.2 (bundled)",
    );
    expect(mlRuntimeStatusMock).toHaveBeenCalledTimes(1);
  });

  /**
   * The effect's `cancelled` guard is what keeps an old call's answer from
   * overwriting a newer one. It is observable where an effect really runs
   * twice on one instance: React's StrictMode mounts, cleans up and mounts
   * again. Both calls are in flight, the second answers first, and the
   * first answers last; the line must keep the second's answer. (A guard
   * against setting state after unmount is not observable: React no longer
   * warns about it.)
   */
  it("keeps the newest call's answer when an older call answers last", async () => {
    const first = held<MlRuntimeStatus>();
    const second = held<MlRuntimeStatus>();
    mlRuntimeStatusMock.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    render(
      <StrictMode>
        <MlRuntimeLine />
      </StrictMode>,
    );
    expect(mlRuntimeStatusMock).toHaveBeenCalledTimes(2);

    await second.resolve(MISSING);
    expect(screen.getByTestId("settings-ml-runtime")).toHaveTextContent(
      "ONNX Runtime: not installed in this build",
    );
    await first.resolve(BUNDLED);
    expect(screen.getByTestId("settings-ml-runtime")).toHaveTextContent(
      "ONNX Runtime: not installed in this build",
    );
    expect(screen.getByTestId("settings-ml-runtime")).toHaveAttribute("data-state", "missing");
  });

  /**
   * The line is read when the user gets to it. A live region would read
   * "checking…" and then the result aloud each time the Account tab opens.
   */
  it("is not a live region, in any state", async () => {
    const answer = held<MlRuntimeStatus>();
    mlRuntimeStatusMock.mockReturnValue(answer.promise);
    const { container } = render(<MlRuntimeLine />);
    expect(container.querySelector("[role], [aria-live]")).toBeNull();

    await answer.resolve(BUNDLED);
    expect(container.querySelector("[role], [aria-live]")).toBeNull();
  });
});

describe("mlRuntimeLine", () => {
  it("says where a runtime that is not the installer's came from", () => {
    const found = (source: "environment" | "executable"): MlRuntimeStatus => ({
      state: "found",
      version: "1.23.2",
      path: "/opt/ort/libonnxruntime.so",
      source,
    });
    expect(mlRuntimeLine(found("environment")).text).toBe(
      "ONNX Runtime: 1.23.2 (from ORT_DYLIB_PATH)",
    );
    expect(mlRuntimeLine(found("executable")).text).toBe(
      "ONNX Runtime: 1.23.2 (next to the app)",
    );
  });

  it("marks a fault as a problem and an absent runtime as information", () => {
    expect(mlRuntimeLine(BUNDLED).tone).toBe("ok");
    expect(mlRuntimeLine(MISSING).tone).toBe("info");
    expect(mlRuntimeLine(TOO_OLD).tone).toBe("problem");
    expect(mlRuntimeLine(BROKEN).tone).toBe("problem");
    expect(mlRuntimeLine(null, "boom").tone).toBe("problem");
    expect(mlRuntimeLine(null).tone).toBe("info");
  });

  it("an empty list of places looked in leaves no hover text", () => {
    expect(mlRuntimeLine({ state: "missing", searched: [] }).detail).toBeNull();
  });

  /**
   * CLAUDE.md: stem separation and transcription are not shipped, and the
   * app must not describe them as working. A runtime that loads says
   * nothing about a model, so no state may say so.
   */
  it("never claims a model runs", () => {
    const every: Array<MlRuntimeStatus | null> = [
      BUNDLED,
      MISSING,
      TOO_OLD,
      BROKEN,
      { state: "missing", searched: [] },
      null,
    ];
    for (const status of every) {
      const { text, detail } = mlRuntimeLine(status, status === null ? "e" : null);
      expect(`${text} ${detail ?? ""}`).not.toMatch(
        /\bstems?\b|separat|transcri|whisper|demucs|download|\bready\b|enabled|\bworks?\b/i,
      );
    }
  });
});
