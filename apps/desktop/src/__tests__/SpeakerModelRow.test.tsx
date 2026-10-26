/**
 * The speaker model row in Settings → Account (#168).
 *
 * The states and the buttons are `ModelRow`'s, tested through the speech
 * model's row (`SpeechModelRow.test.tsx`); this checks what is the speaker
 * row's own: its id on every command, the events of other models ignored,
 * its size, what it is for, the licence credit, and that the copy never
 * says the model is enabled or installed. The bridge is mocked.
 */

import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { MlModelStatus } from "../lib/tauri-bridge";

const mlModelStatusMock = vi.fn();
const downloadMock = vi.fn();
const cancelMock = vi.fn();
const removeMock = vi.fn();
const onProgressMock = vi.fn();

vi.mock("../lib/tauri-bridge", () => ({
  mlModelStatus: (model: string) => mlModelStatusMock(model),
  downloadMlModel: (model: string) => downloadMock(model),
  cancelMlModelDownload: (model: string) => cancelMock(model),
  removeMlModel: (model: string) => removeMock(model),
  onMlModelProgress: (cb: (s: MlModelStatus) => void) => onProgressMock(cb),
}));

import { SpeakerModelRow } from "../components/SpeakerModelRow";
import {
  SPEAKER_MODEL_LICENCES,
  SPEAKER_MODEL_NOTE,
  speakerReadyText,
  speakerRemoveQuestion,
} from "../lib/speakerModel";

const SIZE = 32_517_217;
const status = (fields: Record<string, unknown>): MlModelStatus =>
  ({ model: "speaker-diarization", bytes: 0, size: SIZE, ...fields }) as MlModelStatus;

let emit: (s: MlModelStatus) => void;

async function renderRow(first: MlModelStatus) {
  mlModelStatusMock.mockResolvedValue(first);
  render(<SpeakerModelRow />);
  await waitFor(() => expect(mlModelStatusMock).toHaveBeenCalled());
}

beforeEach(() => {
  mlModelStatusMock.mockReset();
  downloadMock.mockReset();
  cancelMock.mockReset();
  removeMock.mockReset();
  onProgressMock.mockReset().mockImplementation((cb: (s: MlModelStatus) => void) => {
    emit = cb;
    return Promise.resolve(() => undefined);
  });
});

describe("SpeakerModelRow", () => {
  it("names the model, its size and source, and downloads nothing on its own", async () => {
    await renderRow(status({ state: "not_downloaded" }));

    expect(await screen.findByTestId("speaker-model-download")).toHaveTextContent(
      "Download (33 MB)",
    );
    expect(screen.getByTestId("speaker-model-about")).toHaveTextContent(
      "Speaker diarization (pyannote, WeSpeaker), 33 MB, from Hugging Face.",
    );
    expect(mlModelStatusMock).toHaveBeenCalledWith("speaker-diarization");
    expect(mlModelStatusMock).toHaveBeenCalledTimes(1);
    for (const m of [downloadMock, cancelMock, removeMock]) expect(m).not.toHaveBeenCalled();
  });

  it("says what it is for, what the request is, and credits both licences", async () => {
    await renderRow(status({ state: "not_downloaded" }));
    await screen.findByTestId("speaker-model-download");

    expect(screen.getByTestId("speaker-model-note")).toHaveTextContent(SPEAKER_MODEL_NOTE);
    expect(SPEAKER_MODEL_NOTE).toMatch(/runs on this computer/);
    expect(SPEAKER_MODEL_NOTE).toMatch(/once it is downloaded/);
    expect(SPEAKER_MODEL_NOTE).toMatch(/two files/);
    expect(SPEAKER_MODEL_NOTE).toMatch(/huggingface\.co/);
    expect(screen.getByTestId("speaker-model-licences")).toHaveTextContent(
      SPEAKER_MODEL_LICENCES,
    );
    expect(SPEAKER_MODEL_LICENCES).toMatch(/MIT/);
    expect(SPEAKER_MODEL_LICENCES).toMatch(/CC BY 4\.0/);
    expect(SPEAKER_MODEL_LICENCES).toMatch(/WeSpeaker/);
    expect(SPEAKER_MODEL_LICENCES).toMatch(/pyannote/);
  });

  it("sends its own id with Download, Cancel and Remove", async () => {
    const user = userEvent.setup();
    downloadMock.mockResolvedValue(status({ state: "downloading", received: 0, total: SIZE }));
    await renderRow(status({ state: "not_downloaded" }));

    await user.click(await screen.findByTestId("speaker-model-download"));
    expect(downloadMock).toHaveBeenCalledExactlyOnceWith("speaker-diarization");

    cancelMock.mockResolvedValue(status({ state: "downloading", received: 1, total: SIZE }));
    await user.click(await screen.findByTestId("speaker-model-cancel"));
    expect(cancelMock).toHaveBeenCalledExactlyOnceWith("speaker-diarization");

    await act(async () => emit(status({ state: "ready", bytes: SIZE })));
    removeMock.mockResolvedValue(status({ state: "not_downloaded" }));
    vi.spyOn(window, "confirm").mockReturnValue(true);
    await user.click(await screen.findByTestId("speaker-model-remove"));
    await waitFor(() => expect(removeMock).toHaveBeenCalledWith("speaker-diarization"));
  });

  it("follows its own model's events and ignores the speech model's", async () => {
    await renderRow(status({ state: "not_downloaded" }));
    await screen.findByTestId("speaker-model-download");

    await act(async () =>
      emit({ model: "whisper-base", state: "ready", bytes: 1, size: 1 } as MlModelStatus),
    );
    expect(screen.getByTestId("settings-speaker-model")).toHaveAttribute(
      "data-state",
      "not_downloaded",
    );

    await act(async () =>
      emit(status({ state: "downloading", received: SIZE / 2, total: SIZE, bytes: SIZE / 2 })),
    );
    expect(screen.getByTestId("settings-speaker-model")).toHaveAttribute(
      "data-state",
      "downloading",
    );
    expect(screen.getByTestId("speaker-model-status")).toHaveTextContent("16.3 of 32.5 MB (50%)");
    expect(screen.getByRole("progressbar")).toHaveAccessibleName("Speaker model download");
  });

  it("offers Retry after a failure, and a Ready model is the one told to be used", async () => {
    await renderRow(status({ state: "failed", reason: "HTTP 503" }));
    expect(await screen.findByTestId("speaker-model-retry")).toBeVisible();
    expect(screen.getByTestId("speaker-model-status")).toHaveTextContent(
      "The download failed: HTTP 503",
    );

    await act(async () => emit(status({ state: "ready", bytes: SIZE })));
    expect(screen.getByTestId("speaker-model-status")).toHaveTextContent(
      speakerReadyText(status({ state: "ready", bytes: SIZE })),
    );
    expect(speakerReadyText(status({ state: "ready", bytes: SIZE }))).toMatch(/who speaks when/);
    expect(speakerRemoveQuestion(SIZE)).toContain("(33 MB)");
  });

  it("never calls the model enabled or installed", async () => {
    const CLAIMS = /\b(enabled|activated|installed|available now)\b/i;
    for (const text of [SPEAKER_MODEL_NOTE, SPEAKER_MODEL_LICENCES, speakerReadyText(status({}))]) {
      expect(text).not.toMatch(CLAIMS);
    }
  });
});
