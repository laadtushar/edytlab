/**
 * The stem model row in Settings → Account (#385).
 *
 * The states and the buttons are `ModelRow`'s, tested through the speech
 * model's row (`SpeechModelRow.test.tsx`); this checks what is the stem
 * row's own: its id on every command, the events of other models ignored,
 * its size, what it is for and what it costs, the licence credit, and that
 * the copy never says the model is enabled or installed. The bridge is mocked.
 */

import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { MlModelStatus } from "../lib/tauri-bridge";

import { bridge, resetBridge } from "./modelBridge";

const { mlModelStatusMock, downloadMock, cancelMock, removeMock } = bridge;

vi.mock("../lib/tauri-bridge", async () => (await import("./modelBridge")).bridgeModule());

import { StemModelRow } from "../components/StemModelRow";
import {
  STEM_MODEL_LICENCES,
  STEM_MODEL_NOTE,
  stemReadyText,
  stemRemoveQuestion,
} from "../lib/stemModel";

const SIZE = 170_747_491;
const status = (fields: Record<string, unknown>): MlModelStatus =>
  ({ model: "htdemucs", bytes: 0, size: SIZE, ...fields }) as MlModelStatus;

let emit: (s: MlModelStatus) => void;

async function renderRow(first: MlModelStatus) {
  mlModelStatusMock.mockResolvedValue(first);
  render(<StemModelRow />);
  await waitFor(() => expect(mlModelStatusMock).toHaveBeenCalled());
}

beforeEach(() => {
  resetBridge((cb) => {
    emit = cb;
  });
});

describe("StemModelRow", () => {
  it("names the model, its size and source, and downloads nothing on its own", async () => {
    await renderRow(status({ state: "not_downloaded" }));

    expect(await screen.findByTestId("stem-model-download")).toHaveTextContent(
      "Download (171 MB)",
    );
    expect(screen.getByTestId("stem-model-about")).toHaveTextContent(
      "Stem separation (HT-Demucs v4), 171 MB, from Hugging Face.",
    );
    expect(mlModelStatusMock).toHaveBeenCalledWith("htdemucs");
    expect(mlModelStatusMock).toHaveBeenCalledTimes(1);
    for (const m of [downloadMock, cancelMock, removeMock]) expect(m).not.toHaveBeenCalled();
  });

  it("says what it is for, what it costs, what the request is, and credits the licences", async () => {
    await renderRow(status({ state: "not_downloaded" }));
    await screen.findByTestId("stem-model-download");

    expect(screen.getByTestId("stem-model-note")).toHaveTextContent(STEM_MODEL_NOTE);
    expect(STEM_MODEL_NOTE).toMatch(/runs on this computer/);
    expect(STEM_MODEL_NOTE).toMatch(/once it is downloaded/);
    expect(STEM_MODEL_NOTE).toMatch(/4 GB of free memory/);
    expect(STEM_MODEL_NOTE).toMatch(/two files/);
    expect(STEM_MODEL_NOTE).toMatch(/huggingface\.co/);
    expect(screen.getByTestId("stem-model-licences")).toHaveTextContent(STEM_MODEL_LICENCES);
    expect(STEM_MODEL_LICENCES).toMatch(/MIT/);
    expect(STEM_MODEL_LICENCES).toMatch(/Meta/);
    expect(STEM_MODEL_LICENCES).toMatch(/Intel/);
  });

  it("sends its own id with Download, Cancel and Remove", async () => {
    const user = userEvent.setup();
    downloadMock.mockResolvedValue(status({ state: "downloading", received: 0, total: SIZE }));
    await renderRow(status({ state: "not_downloaded" }));

    await user.click(await screen.findByTestId("stem-model-download"));
    expect(downloadMock).toHaveBeenCalledExactlyOnceWith("htdemucs");

    cancelMock.mockResolvedValue(status({ state: "downloading", received: 1, total: SIZE }));
    await user.click(await screen.findByTestId("stem-model-cancel"));
    expect(cancelMock).toHaveBeenCalledExactlyOnceWith("htdemucs");

    await act(async () => emit(status({ state: "ready", bytes: SIZE })));
    removeMock.mockResolvedValue(status({ state: "not_downloaded" }));
    vi.spyOn(window, "confirm").mockReturnValue(true);
    await user.click(await screen.findByTestId("stem-model-remove"));
    await waitFor(() => expect(removeMock).toHaveBeenCalledWith("htdemucs"));
  });

  it("follows its own model's events and ignores the other models'", async () => {
    await renderRow(status({ state: "not_downloaded" }));
    await screen.findByTestId("stem-model-download");

    for (const other of ["whisper-base", "speaker-diarization"]) {
      await act(async () =>
        emit({ model: other, state: "ready", bytes: 1, size: 1 } as MlModelStatus),
      );
    }
    expect(screen.getByTestId("settings-stem-model")).toHaveAttribute(
      "data-state",
      "not_downloaded",
    );

    await act(async () =>
      emit(status({ state: "downloading", received: SIZE / 2, total: SIZE, bytes: SIZE / 2 })),
    );
    expect(screen.getByTestId("settings-stem-model")).toHaveAttribute("data-state", "downloading");
    expect(screen.getByTestId("stem-model-status")).toHaveTextContent("85.4 of 170.7 MB (50%)");
    expect(screen.getByRole("progressbar")).toHaveAccessibleName("Stem model download");
  });

  it("offers Retry after a failure, and a Ready model is the one told to be used", async () => {
    await renderRow(status({ state: "failed", reason: "HTTP 503" }));
    expect(await screen.findByTestId("stem-model-retry")).toBeVisible();
    expect(screen.getByTestId("stem-model-status")).toHaveTextContent(
      "The download failed: HTTP 503",
    );

    await act(async () => emit(status({ state: "ready", bytes: SIZE })));
    expect(screen.getByTestId("stem-model-status")).toHaveTextContent(
      stemReadyText(status({ state: "ready", bytes: SIZE })),
    );
    expect(stemReadyText(status({ state: "ready", bytes: SIZE }))).toMatch(/separate the stems/);
    expect(stemRemoveQuestion(SIZE)).toContain("(171 MB)");
  });

  it("never calls the model enabled or installed", () => {
    const CLAIMS = /\b(enabled|activated|installed|available now)\b/i;
    for (const text of [STEM_MODEL_NOTE, STEM_MODEL_LICENCES, stemReadyText(status({}))]) {
      expect(text).not.toMatch(CLAIMS);
    }
  });
});
