/**
 * The speaker model row (#168) and the stem model row (#385) in Settings →
 * Account.
 *
 * The states and the buttons are `ModelRow`'s, tested through the speech
 * model's row (`SpeechModelRow.test.tsx`); this checks what is each row's
 * own, with one table entry per row: its id on every command, the events of
 * other models ignored, its size, what it is for (and, for the stem model,
 * what it costs), the licence credit, and that the copy never says the model
 * is enabled or installed. The bridge is mocked.
 */

import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ComponentType } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { MlModelStatus } from "../lib/tauri-bridge";

import { bridge, resetBridge } from "./modelBridge";

const { mlModelStatusMock, downloadMock, cancelMock, removeMock } = bridge;

vi.mock("../lib/tauri-bridge", async () => (await import("./modelBridge")).bridgeModule());

import { SpeakerModelRow } from "../components/SpeakerModelRow";
import { StemModelRow } from "../components/StemModelRow";
import {
  SPEAKER_MODEL_LICENCES,
  SPEAKER_MODEL_NOTE,
  speakerReadyText,
  speakerRemoveQuestion,
} from "../lib/speakerModel";
import {
  STEM_MODEL_LICENCES,
  STEM_MODEL_NOTE,
  stemReadyText,
  stemRemoveQuestion,
} from "../lib/stemModel";

interface Row {
  name: string;
  Component: ComponentType;
  id: string;
  prefix: string;
  size: number;
  downloadText: string;
  about: string;
  note: string;
  /** What the note must say, beyond what every row's says. */
  noteParts: RegExp[];
  licences: string;
  licenceParts: RegExp[];
  readyText: (s: MlModelStatus) => string;
  readyAsk: RegExp;
  removeQuestion: (size: number) => string;
  removeSize: string;
  halfText: string;
  progressLabel: string;
}

const ROWS: Row[] = [
  {
    name: "SpeakerModelRow",
    Component: SpeakerModelRow,
    id: "speaker-diarization",
    prefix: "speaker-model",
    size: 32_517_217,
    downloadText: "Download (33 MB)",
    about: "Speaker diarization (pyannote, WeSpeaker), 33 MB, from Hugging Face.",
    note: SPEAKER_MODEL_NOTE,
    noteParts: [],
    licences: SPEAKER_MODEL_LICENCES,
    licenceParts: [/MIT/, /CC BY 4\.0/, /WeSpeaker/, /pyannote/],
    readyText: speakerReadyText,
    readyAsk: /who speaks when/,
    removeQuestion: speakerRemoveQuestion,
    removeSize: "(33 MB)",
    halfText: "16.3 of 32.5 MB (50%)",
    progressLabel: "Speaker model download",
  },
  {
    name: "StemModelRow",
    Component: StemModelRow,
    id: "htdemucs",
    prefix: "stem-model",
    size: 170_747_491,
    downloadText: "Download (171 MB)",
    about: "Stem separation (HT-Demucs v4), 171 MB, from Hugging Face.",
    note: STEM_MODEL_NOTE,
    noteParts: [/4 GB of free memory/],
    licences: STEM_MODEL_LICENCES,
    licenceParts: [/MIT/, /Meta/, /Intel/],
    readyText: stemReadyText,
    readyAsk: /separate the stems/,
    removeQuestion: stemRemoveQuestion,
    removeSize: "(171 MB)",
    halfText: "85.4 of 170.7 MB (50%)",
    progressLabel: "Stem model download",
  },
];

const ALL_IDS = ["whisper-base", ...ROWS.map((r) => r.id)];

let emit: (s: MlModelStatus) => void;

beforeEach(() => {
  resetBridge((cb) => {
    emit = cb;
  });
});

describe.each(ROWS)("$name", (row) => {
  const SIZE = row.size;
  const tid = (part: string) => `${row.prefix}-${part}`;
  const status = (fields: Record<string, unknown>): MlModelStatus =>
    ({ model: row.id, bytes: 0, size: SIZE, ...fields }) as MlModelStatus;

  async function renderRow(first: MlModelStatus) {
    mlModelStatusMock.mockResolvedValue(first);
    render(<row.Component />);
    await waitFor(() => expect(mlModelStatusMock).toHaveBeenCalled());
  }

  it("names the model, its size and source, and downloads nothing on its own", async () => {
    await renderRow(status({ state: "not_downloaded" }));

    expect(await screen.findByTestId(tid("download"))).toHaveTextContent(row.downloadText);
    expect(screen.getByTestId(tid("about"))).toHaveTextContent(row.about);
    expect(mlModelStatusMock).toHaveBeenCalledWith(row.id);
    expect(mlModelStatusMock).toHaveBeenCalledTimes(1);
    for (const m of [downloadMock, cancelMock, removeMock]) expect(m).not.toHaveBeenCalled();
  });

  it("says what it is for, what the request is, and credits the licences", async () => {
    await renderRow(status({ state: "not_downloaded" }));
    await screen.findByTestId(tid("download"));

    expect(screen.getByTestId(tid("note"))).toHaveTextContent(row.note);
    for (const part of [
      /runs on this computer/,
      /once it is downloaded/,
      /two files/,
      /huggingface\.co/,
      ...row.noteParts,
    ]) {
      expect(row.note).toMatch(part);
    }
    expect(screen.getByTestId(tid("licences"))).toHaveTextContent(row.licences);
    for (const part of row.licenceParts) expect(row.licences).toMatch(part);
  });

  it("sends its own id with Download, Cancel and Remove", async () => {
    const user = userEvent.setup();
    downloadMock.mockResolvedValue(status({ state: "downloading", received: 0, total: SIZE }));
    await renderRow(status({ state: "not_downloaded" }));

    await user.click(await screen.findByTestId(tid("download")));
    expect(downloadMock).toHaveBeenCalledExactlyOnceWith(row.id);

    cancelMock.mockResolvedValue(status({ state: "downloading", received: 1, total: SIZE }));
    await user.click(await screen.findByTestId(tid("cancel")));
    expect(cancelMock).toHaveBeenCalledExactlyOnceWith(row.id);

    await act(async () => emit(status({ state: "ready", bytes: SIZE })));
    removeMock.mockResolvedValue(status({ state: "not_downloaded" }));
    vi.spyOn(window, "confirm").mockReturnValue(true);
    await user.click(await screen.findByTestId(tid("remove")));
    await waitFor(() => expect(removeMock).toHaveBeenCalledWith(row.id));
  });

  it("follows its own model's events and ignores the other models'", async () => {
    await renderRow(status({ state: "not_downloaded" }));
    await screen.findByTestId(tid("download"));

    for (const other of ALL_IDS.filter((id) => id !== row.id)) {
      await act(async () =>
        emit({ model: other, state: "ready", bytes: 1, size: 1 } as MlModelStatus),
      );
    }
    const settingsRow = screen.getByTestId(`settings-${row.prefix}`);
    expect(settingsRow).toHaveAttribute("data-state", "not_downloaded");

    await act(async () =>
      emit(status({ state: "downloading", received: SIZE / 2, total: SIZE, bytes: SIZE / 2 })),
    );
    expect(settingsRow).toHaveAttribute("data-state", "downloading");
    expect(screen.getByTestId(tid("status"))).toHaveTextContent(row.halfText);
    expect(screen.getByRole("progressbar")).toHaveAccessibleName(row.progressLabel);
  });

  it("offers Retry after a failure, and a Ready model is the one told to be used", async () => {
    await renderRow(status({ state: "failed", reason: "HTTP 503" }));
    expect(await screen.findByTestId(tid("retry"))).toBeVisible();
    expect(screen.getByTestId(tid("status"))).toHaveTextContent("The download failed: HTTP 503");

    const ready = status({ state: "ready", bytes: SIZE });
    await act(async () => emit(ready));
    expect(screen.getByTestId(tid("status"))).toHaveTextContent(row.readyText(ready));
    expect(row.readyText(ready)).toMatch(row.readyAsk);
    expect(row.removeQuestion(SIZE)).toContain(row.removeSize);
  });

  it("never calls the model enabled or installed", () => {
    const CLAIMS = /\b(enabled|activated|installed|available now)\b/i;
    for (const text of [row.note, row.licences, row.readyText(status({}))]) {
      expect(text).not.toMatch(CLAIMS);
    }
  });
});
