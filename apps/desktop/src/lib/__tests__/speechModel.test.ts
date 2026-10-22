/**
 * The numbers and sentences of the speech model row (#383).
 */

import { describe, expect, it } from "vitest";

import {
  SPEECH_MODEL_NOTE,
  failedText,
  megabytes,
  partialText,
  percent,
  progressText,
  readyText,
  removeQuestion,
} from "../speechModel";

const SIZE = 138_644_904;

describe("speechModel", () => {
  it("writes the pinned size as the pages do: 139 MB", () => {
    expect(megabytes(SIZE)).toBe("139 MB");
    expect(removeQuestion(SIZE)).toContain("(139 MB)");
  });

  it("never rounds a size down to nothing", () => {
    expect(megabytes(10)).toBe("1 MB");
  });

  it("counts a percent down, and never past 100", () => {
    expect(percent(0, SIZE)).toBe(0);
    expect(percent(SIZE / 2, SIZE)).toBe(50);
    expect(percent(SIZE - 1, SIZE)).toBe(99);
    expect(percent(SIZE * 2, SIZE)).toBe(100);
    expect(percent(5, 0)).toBe(0);
  });

  it("shows megabytes to a tenth while it downloads", () => {
    expect(progressText(41_200_000, SIZE)).toBe("41.2 of 138.6 MB (29%)");
    expect(partialText(41_200_000, SIZE)).toBe(
      "41.2 of 138.6 MB already downloaded. Download carries on from there.",
    );
  });

  it("states what ready means, and no more", () => {
    expect(
      readyText({ model: "whisper-base", state: "ready", bytes: SIZE, size: SIZE }),
    ).toBe("Ready: 139 MB on disk. Nothing uses it yet.");
  });

  it("gives the failure's reason as it was reported", () => {
    expect(failedText("HTTP 503")).toBe("The download failed: HTTP 503");
  });

  it("is the same note the pages make: no transcription yet, one request, no audio", () => {
    expect(SPEECH_MODEL_NOTE).toContain("Transcription is not available yet");
    expect(SPEECH_MODEL_NOTE).toContain("does not turn anything on");
    expect(SPEECH_MODEL_NOTE).toContain("huggingface.co");
    expect(SPEECH_MODEL_NOTE).toContain("nothing about your audio is sent");
  });
});
