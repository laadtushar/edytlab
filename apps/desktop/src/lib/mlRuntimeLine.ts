/**
 * The one line Settings shows about the ONNX Runtime library (#383).
 *
 * Kept apart from the component so the wording has one home and a test that
 * reads it without rendering. The words say what was found and nothing
 * about what it is for: a runtime that loads is not a model loaded
 * (`transcribe` loads the speech model when it is asked, and stem
 * separation is not shipped, #384, #385), so no state here promises either.
 */

import type { MlRuntimeStatus } from "./tauri-bridge";

export interface MlRuntimeLineText {
  /** The visible line. */
  text: string;
  /** More, for a hover: the file, or where the loader looked. */
  detail: string | null;
  /** `ok` is a library in use; `info` is none installed; `problem` is a fault. */
  tone: "ok" | "info" | "problem";
}

/** What the line says while the status is still being asked for. */
export const ML_RUNTIME_PENDING = "ONNX Runtime: checking…";

/**
 * The line for `status`, or for the command failing (`error`, the string
 * the backend rejected with).
 */
export function mlRuntimeLine(
  status: MlRuntimeStatus | null,
  error: string | null = null,
): MlRuntimeLineText {
  if (status === null) {
    return error === null
      ? { text: ML_RUNTIME_PENDING, detail: null, tone: "info" }
      : {
          text: `ONNX Runtime: could not be checked (${error})`,
          detail: null,
          tone: "problem",
        };
  }
  switch (status.state) {
    case "found": {
      const where =
        status.source === "bundled"
          ? "bundled"
          : status.source === "environment"
            ? "from ORT_DYLIB_PATH"
            : "next to the app";
      return {
        text: `ONNX Runtime: ${status.version} (${where})`,
        detail: status.path,
        tone: "ok",
      };
    }
    case "missing":
      return {
        text: "ONNX Runtime: not installed in this build",
        detail:
          status.searched.length > 0
            ? `Looked in: ${status.searched.join(", ")}`
            : null,
        tone: "info",
      };
    case "unsupportedOs":
      return {
        text:
          `ONNX Runtime: needs macOS ${status.needs} or newer ` +
          `(this Mac is running ${status.running})`,
        detail: null,
        tone: "problem",
      };
    case "loadFailed":
      return {
        text: `ONNX Runtime: could not be loaded (${status.reason})`,
        detail: status.path,
        tone: "problem",
      };
  }
}
