/**
 * The numbers and sentences of the speech model row (#383).
 */

import { describe, expect, it } from "vitest";

import {
  SPEECH_MODEL_NOTE,
  failedText,
  isFullSize,
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

  it("calls files as big as the model unchecked, not a partial download", () => {
    expect(partialText(SIZE, SIZE)).toContain("have not been checked");
    expect(partialText(SIZE, SIZE)).not.toContain("carries on");
    expect(isFullSize(SIZE, SIZE)).toBe(true);
    expect(isFullSize(SIZE - 1, SIZE)).toBe(false);
    expect(isFullSize(0, 0)).toBe(false);
  });

  it("states what ready means, and no more", () => {
    expect(
      readyText({ model: "whisper-base", state: "ready", bytes: SIZE, size: SIZE }),
    ).toBe("Ready: 139 MB on disk. Ask the assistant to transcribe a track.");
  });

  it("gives the failure's reason as it was reported", () => {
    expect(failedText("HTTP 503")).toBe("The download failed: HTTP 503");
  });

  it("is the same note the pages make: what it is for, three files, what is seen, no audio", () => {
    expect(SPEECH_MODEL_NOTE).toContain("runs on this computer");
    expect(SPEECH_MODEL_NOTE).toContain("once it is downloaded");
    expect(SPEECH_MODEL_NOTE).toContain("huggingface.co");
    expect(SPEECH_MODEL_NOTE).toContain("three files");
    expect(SPEECH_MODEL_NOTE).toContain("IP address and that the request comes from edytlab");
    expect(SPEECH_MODEL_NOTE).toContain("nothing about your audio is sent");
    expect(SPEECH_MODEL_NOTE).not.toContain("one request");
    // The words reach the provider, like any tool result: said, not hidden.
    expect(SPEECH_MODEL_NOTE).toContain("reach your AI provider");
  });
});
