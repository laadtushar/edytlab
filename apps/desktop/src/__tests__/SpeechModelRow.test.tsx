/**
 * The speech model row in Settings → Account (#383).
 *
 * One test per state the backend can report (not downloaded, partly
 * downloaded, downloading, ready, failed), the buttons each one offers,
 * and the rules around them: nothing is requested until Download is
 * pressed, Remove asks first, the row copy never says transcription works,
 * and an answer that was overtaken by an event is dropped. The bridge is
 * mocked, so no event or command leaves the test; the event handler the
 * row registers is kept and called by hand, the way the backend's
 * `ml-model-progress` event would.
 */

import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { StrictMode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from "vitest";

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

import { SpeechModelRow } from "../components/SpeechModelRow";
import {
  DISCARD_QUESTION,
  SPEECH_MODEL_NOTE,
  SPEECH_MODEL_PENDING,
  removeQuestion,
} from "../lib/speechModel";
import { held } from "./held";

const SIZE = 138_644_904;

const NOT_DOWNLOADED: MlModelStatus = {
  model: "whisper-base",
  state: "not_downloaded",
  bytes: 0,
  size: SIZE,
};
const PARTIAL: MlModelStatus = { ...NOT_DOWNLOADED, bytes: 41_200_000 };
const downloading = (received: number): MlModelStatus => ({
  model: "whisper-base",
  state: "downloading",
  received,
  total: SIZE,
  bytes: received,
  size: SIZE,
});
const READY: MlModelStatus = {
  model: "whisper-base",
  state: "ready",
  bytes: SIZE,
  size: SIZE,
};
const FAILED: MlModelStatus = {
  model: "whisper-base",
  state: "failed",
  reason: "downloading https://huggingface.co/x failed (HTTP 503): Service Unavailable",
  bytes: 0,
  size: SIZE,
};

/** The handler the row registered for `ml-model-progress`. */
let emit: (status: MlModelStatus) => void;
let unlisten: Mock<() => void>;

async function emitEvent(status: MlModelStatus) {
  await act(async () => {
    emit(status);
  });
}

async function renderRow(status: MlModelStatus | Promise<MlModelStatus> = NOT_DOWNLOADED) {
  mlModelStatusMock.mockReturnValue(Promise.resolve(status));
  const view = render(<SpeechModelRow />);
  // The row subscribes first and asks for the status after it.
  await waitFor(() => expect(mlModelStatusMock).toHaveBeenCalled());
  return view;
}

const row = () => screen.getByTestId("settings-speech-model");

beforeEach(() => {
  mlModelStatusMock.mockReset();
  downloadMock.mockReset();
  cancelMock.mockReset();
  removeMock.mockReset();
  unlisten = vi.fn<() => void>();
  onProgressMock.mockReset().mockImplementation((cb: (s: MlModelStatus) => void) => {
    emit = cb;
    return Promise.resolve(unlisten);
  });
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("SpeechModelRow", () => {
  it("says it is checking until the status comes, and asks once", async () => {
    const answer = held<MlModelStatus>();
    mlModelStatusMock.mockReturnValue(answer.promise);
    render(<SpeechModelRow />);

    expect(row()).toHaveAttribute("data-state", "pending");
    expect(screen.getByTestId("speech-model-status")).toHaveTextContent(SPEECH_MODEL_PENDING);
    expect(screen.queryByTestId("speech-model-download")).toBeNull();

    await answer.resolve(NOT_DOWNLOADED);
    expect(row()).toHaveAttribute("data-state", "not_downloaded");
    expect(mlModelStatusMock).toHaveBeenCalledTimes(1);
    expect(mlModelStatusMock).toHaveBeenCalledWith("whisper-base");
  });

  describe("not downloaded", () => {
    it("names the model, its size and where it comes from, and offers Download", async () => {
      await renderRow();

      expect(await screen.findByTestId("speech-model-download")).toHaveTextContent(
        "Download (139 MB)",
      );
      expect(screen.getByTestId("speech-model-about")).toHaveTextContent(
        "Whisper base, 139 MB, from Hugging Face.",
      );
      expect(screen.queryByTestId("speech-model-cancel")).toBeNull();
      expect(screen.queryByTestId("speech-model-remove")).toBeNull();
      expect(screen.queryByTestId("speech-model-progress")).toBeNull();
    });

    it("downloads nothing until Download is pressed", async () => {
      await renderRow();
      await screen.findByTestId("speech-model-download");

      expect(downloadMock).not.toHaveBeenCalled();
      expect(cancelMock).not.toHaveBeenCalled();
      expect(removeMock).not.toHaveBeenCalled();
    });

    it("starts the download when Download is pressed, and shows it running", async () => {
      downloadMock.mockResolvedValue(downloading(0));
      await renderRow();

      await userEvent.click(await screen.findByTestId("speech-model-download"));

      expect(downloadMock).toHaveBeenCalledTimes(1);
      expect(downloadMock).toHaveBeenCalledWith("whisper-base");
      expect(await screen.findByTestId("speech-model-progress")).toBeInTheDocument();
      expect(row()).toHaveAttribute("data-state", "downloading");
    });

    it("says honestly what the model is for, and that downloading turns nothing on", async () => {
      await renderRow();
      await screen.findByTestId("speech-model-download");

      const note = screen.getByTestId("speech-model-note");
      expect(note).toHaveTextContent(SPEECH_MODEL_NOTE);
      expect(note).toHaveTextContent("Transcription is not available yet");
      expect(note).toHaveTextContent("does not turn anything on");
      // Where the button's requests go, and what they do and do not carry.
      expect(note).toHaveTextContent("huggingface.co");
      expect(note).toHaveTextContent("three files");
      expect(note).toHaveTextContent("your IP address");
      expect(note).toHaveTextContent("comes from edytlab");
      expect(note).toHaveTextContent("nothing about your audio is sent");
      // Not "one request": it asks for three files, and again on a resume.
      expect(note).not.toHaveTextContent("one request");
    });
  });

  describe("partly downloaded", () => {
    it("says how much is there and offers to resume or to delete it", async () => {
      await renderRow(PARTIAL);

      expect(await screen.findByTestId("speech-model-status")).toHaveTextContent(
        "41.2 of 138.6 MB already downloaded",
      );
      expect(screen.getByTestId("speech-model-download")).toHaveTextContent("Resume download");
      expect(screen.getByTestId("speech-model-discard")).toHaveTextContent(
        "Delete partial download",
      );
    });

    it("does not call a full-size copy that has not been checked a partial download", async () => {
      // Files of the right size whose hash is wrong, or a download cut off
      // after its last byte: `not_downloaded` with `bytes` equal to `size`.
      await renderRow({ ...NOT_DOWNLOADED, bytes: SIZE });

      const line = await screen.findByTestId("speech-model-status");
      expect(line).toHaveTextContent("have not been checked");
      expect(line).toHaveTextContent("fetches again what is wrong");
      // Not "carries on from there": that is what a partial file gets.
      expect(line).not.toHaveTextContent("already downloaded");
      expect(screen.getByTestId("speech-model-download")).toHaveTextContent("Check and download");
      expect(screen.getByTestId("speech-model-discard")).toHaveTextContent("Delete these files");
    });

    it("deletes the partial download only after asking", async () => {
      removeMock.mockResolvedValue(NOT_DOWNLOADED);
      const confirm = vi.spyOn(window, "confirm");
      await renderRow(PARTIAL);
      const discard = await screen.findByTestId("speech-model-discard");

      confirm.mockReturnValue(false);
      await userEvent.click(discard);
      expect(confirm).toHaveBeenCalledWith(DISCARD_QUESTION);
      expect(removeMock).not.toHaveBeenCalled();

      confirm.mockReturnValue(true);
      await userEvent.click(discard);
      await waitFor(() => expect(removeMock).toHaveBeenCalledWith("whisper-base"));
      await waitFor(() =>
        expect(screen.getByTestId("speech-model-download")).toHaveTextContent("Download (139 MB)"),
      );
      expect(screen.queryByTestId("speech-model-discard")).toBeNull();
    });
  });

  describe("downloading", () => {
    it("shows a progress bar with the bytes so far, and Cancel", async () => {
      await renderRow(downloading(41_200_000));

      const bar = await screen.findByTestId("speech-model-progress");
      expect(bar).toHaveAttribute("role", "progressbar");
      expect(bar).toHaveAttribute("aria-valuenow", "29");
      expect(bar).toHaveAccessibleName("Speech model download");
      expect(screen.getByTestId("speech-model-progress-fill")).toHaveStyle({ width: "29%" });
      expect(screen.getByTestId("speech-model-status")).toHaveTextContent(
        "41.2 of 138.6 MB (29%)",
      );
      expect(screen.getByTestId("speech-model-cancel")).toHaveTextContent("Cancel");
      expect(screen.queryByTestId("speech-model-download")).toBeNull();
      expect(screen.queryByTestId("speech-model-remove")).toBeNull();
    });

    it("moves with the events", async () => {
      await renderRow(downloading(1_000_000));
      await screen.findByTestId("speech-model-progress");

      await emitEvent(downloading(69_322_452));
      expect(screen.getByTestId("speech-model-progress")).toHaveAttribute("aria-valuenow", "50");
      expect(screen.getByTestId("speech-model-status")).toHaveTextContent(
        "69.3 of 138.6 MB (50%)",
      );

      await emitEvent(READY);
      expect(row()).toHaveAttribute("data-state", "ready");
      expect(screen.queryByTestId("speech-model-progress")).toBeNull();
    });

    it("stops the download when Cancel is pressed, and waits for the backend to say it has", async () => {
      cancelMock.mockResolvedValue(downloading(5_000_000));
      await renderRow(downloading(5_000_000));

      await userEvent.click(await screen.findByTestId("speech-model-cancel"));
      expect(cancelMock).toHaveBeenCalledTimes(1);
      expect(cancelMock).toHaveBeenCalledWith("whisper-base");
      // Still downloading as far as the backend has said, and not pressed twice.
      await waitFor(() =>
        expect(screen.getByTestId("speech-model-cancel")).toHaveTextContent("Cancelling…"),
      );
      expect(screen.getByTestId("speech-model-cancel")).toBeDisabled();

      await emitEvent({ ...NOT_DOWNLOADED, bytes: 5_300_000 });
      expect(row()).toHaveAttribute("data-state", "not_downloaded");
      // What was received is kept, and Download resumes it.
      expect(screen.getByTestId("speech-model-download")).toHaveTextContent("Resume download");
      expect(screen.queryByTestId("speech-model-cancel")).toBeNull();
    });

    it("shows a download that was already running when Settings opened", async () => {
      await renderRow(downloading(80_000_000));
      expect(await screen.findByTestId("speech-model-progress")).toHaveAttribute(
        "aria-valuenow",
        "57",
      );
      // It did not start another.
      expect(downloadMock).not.toHaveBeenCalled();
    });
  });

  describe("ready", () => {
    it("says so, with its size on disk, and offers Remove", async () => {
      await renderRow(READY);

      const line = await screen.findByTestId("speech-model-status");
      expect(line).toHaveTextContent("Ready: 139 MB on disk.");
      // Honest about what Ready means.
      expect(line).toHaveTextContent("Nothing uses it yet.");
      expect(screen.getByTestId("speech-model-remove")).toHaveTextContent("Remove");
      expect(screen.queryByTestId("speech-model-download")).toBeNull();
      expect(row()).toHaveAttribute("data-state", "ready");
    });

    it("removes it only after asking, and Cancel in the question removes nothing", async () => {
      removeMock.mockResolvedValue(NOT_DOWNLOADED);
      const confirm = vi.spyOn(window, "confirm");
      await renderRow(READY);
      const remove = await screen.findByTestId("speech-model-remove");

      confirm.mockReturnValue(false);
      await userEvent.click(remove);
      expect(confirm).toHaveBeenCalledWith(removeQuestion(SIZE));
      expect(removeMock).not.toHaveBeenCalled();
      expect(row()).toHaveAttribute("data-state", "ready");

      confirm.mockReturnValue(true);
      await userEvent.click(remove);
      await waitFor(() => expect(removeMock).toHaveBeenCalledWith("whisper-base"));
      await waitFor(() => expect(row()).toHaveAttribute("data-state", "not_downloaded"));
      expect(screen.getByTestId("speech-model-download")).toBeInTheDocument();
    });

    it("waits for the question's answer, as the packaged app's confirm is async", async () => {
      removeMock.mockResolvedValue(NOT_DOWNLOADED);
      // The packaged app's `window.confirm` answers with a Promise (#544).
      const answer = held<boolean>();
      vi.spyOn(window, "confirm").mockReturnValue(answer.promise as unknown as boolean);
      await renderRow(READY);

      await userEvent.click(await screen.findByTestId("speech-model-remove"));
      expect(removeMock).not.toHaveBeenCalled();
      await answer.resolve(false);
      expect(removeMock).not.toHaveBeenCalled();
    });
  });

  describe("failed", () => {
    it("shows the reason and offers Retry, which downloads again", async () => {
      downloadMock.mockResolvedValue(downloading(0));
      await renderRow(FAILED);

      const reason = await screen.findByTestId("speech-model-status");
      expect(reason).toHaveAttribute("role", "alert");
      expect(reason).toHaveTextContent("The download failed:");
      expect(reason).toHaveTextContent("HTTP 503");
      expect(row()).toHaveAttribute("data-state", "failed");
      expect(screen.queryByTestId("speech-model-discard")).toBeNull();

      await userEvent.click(screen.getByTestId("speech-model-retry"));
      expect(downloadMock).toHaveBeenCalledTimes(1);
      expect(await screen.findByTestId("speech-model-progress")).toBeInTheDocument();
    });

    it("lets a failed download that left part of the file be deleted", async () => {
      await renderRow({ ...FAILED, bytes: 12_000_000 });
      expect(await screen.findByTestId("speech-model-discard")).toBeInTheDocument();
      expect(screen.getByTestId("speech-model-retry")).toBeInTheDocument();
    });

    it("arrives as an event while downloading", async () => {
      await renderRow(downloading(3_000_000));
      await screen.findByTestId("speech-model-progress");

      await emitEvent(FAILED);
      expect(screen.getByTestId("speech-model-status")).toHaveTextContent("HTTP 503");
      expect(screen.getByTestId("speech-model-retry")).toBeInTheDocument();
      expect(screen.queryByTestId("speech-model-progress")).toBeNull();
    });
  });

  describe("when the backend cannot answer", () => {
    it("shows why the status could not be read, and can ask again", async () => {
      mlModelStatusMock.mockRejectedValueOnce(
        "models cannot be downloaded: the app has no data folder to keep them in",
      );
      render(<SpeechModelRow />);

      const line = await screen.findByText(/Could not check the speech model/);
      expect(line).toHaveTextContent("no data folder");
      expect(row()).toHaveAttribute("data-state", "error");
      expect(screen.queryByTestId("speech-model-download")).toBeNull();

      mlModelStatusMock.mockResolvedValue(NOT_DOWNLOADED);
      await userEvent.click(screen.getByTestId("speech-model-check"));
      expect(await screen.findByTestId("speech-model-download")).toBeInTheDocument();
    });

    it("shows a refused command next to the state it left alone", async () => {
      downloadMock.mockRejectedValue("unknown model \"whisper-base\"");
      await renderRow();

      await userEvent.click(await screen.findByTestId("speech-model-download"));
      expect(await screen.findByTestId("speech-model-problem")).toHaveTextContent(
        'unknown model "whisper-base"',
      );
      // Still not downloaded, and can be tried again.
      expect(screen.getByTestId("speech-model-download")).not.toBeDisabled();
    });
  });

  describe("events and answers racing", () => {
    it("keeps an event that arrives before the status answer", async () => {
      const answer = held<MlModelStatus>();
      mlModelStatusMock.mockReturnValue(answer.promise);
      render(<SpeechModelRow />);
      await waitFor(() => expect(mlModelStatusMock).toHaveBeenCalled());

      await emitEvent(READY);
      // The older answer (read before the download finished) must not win.
      await answer.resolve(downloading(1_000_000));
      expect(row()).toHaveAttribute("data-state", "ready");
    });

    it("keeps an event that arrives while Download's answer is on the way", async () => {
      const answer = held<MlModelStatus>();
      downloadMock.mockReturnValue(answer.promise);
      await renderRow();

      await userEvent.click(await screen.findByTestId("speech-model-download"));
      await emitEvent(downloading(30_000_000));
      await answer.resolve(downloading(0));
      expect(screen.getByTestId("speech-model-progress")).toHaveAttribute("aria-valuenow", "21");
    });

    it("ignores an event about another model", async () => {
      await renderRow();
      await screen.findByTestId("speech-model-download");

      await emitEvent({ ...READY, model: "something-else" });
      expect(row()).toHaveAttribute("data-state", "not_downloaded");
    });
  });

  describe("listening", () => {
    it("stops listening when the row goes away", async () => {
      const view = await renderRow();
      await screen.findByTestId("speech-model-download");

      view.unmount();
      expect(unlisten).toHaveBeenCalledTimes(1);
    });

    it("stops listening even if it goes away before the listener is registered", async () => {
      const registered = held<() => void>();
      onProgressMock.mockReturnValue(registered.promise);
      mlModelStatusMock.mockResolvedValue(NOT_DOWNLOADED);
      const view = render(<SpeechModelRow />);

      view.unmount();
      await registered.resolve(unlisten);
      expect(unlisten).toHaveBeenCalledTimes(1);
    });

    it("works under StrictMode's double mount and listens once at the end", async () => {
      mlModelStatusMock.mockResolvedValue(NOT_DOWNLOADED);
      render(
        <StrictMode>
          <SpeechModelRow />
        </StrictMode>,
      );
      expect(await screen.findByTestId("speech-model-download")).toBeInTheDocument();
      // One listener was let go (the first mount's), one is still live.
      await waitFor(() => expect(onProgressMock).toHaveBeenCalledTimes(2));
      await waitFor(() => expect(unlisten).toHaveBeenCalledTimes(1));
    });
  });

  describe("the copy", () => {
    const CLAIMS = /\b(enabled|activated|installed|available now|ready to transcribe)\b/i;

    it.each([
      ["not downloaded", NOT_DOWNLOADED],
      ["partly downloaded", PARTIAL],
      ["downloading", downloading(10_000_000)],
      ["ready", READY],
      ["failed", FAILED],
    ])("never says transcription works or is on while %s", async (_name, status) => {
      await renderRow(status);
      await screen.findByTestId("speech-model-note");

      expect(row().textContent ?? "").not.toMatch(CLAIMS);
      // The honest line is there in every state.
      expect(screen.getByTestId("speech-model-note")).toHaveTextContent(
        "Transcription is not available yet",
      );
    });
  });
});
