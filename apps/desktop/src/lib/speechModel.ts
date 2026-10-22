/**
 * The words of the Settings row about the speech model (#383).
 *
 * Kept apart from the component so the wording has one home and a test
 * can read it without rendering. The row is honest about what a download
 * is: the files are on disk afterwards, checked against their hashes. The
 * words say what they are for (transcription, on this computer, once they
 * are there) and no state calls the model "enabled" or "installed" as a
 * feature; and the only network request in the row is the one the user's
 * click makes, to huggingface.co, which no copy hides. Transcribing makes no
 * request of its own (#384).
 */

import type { MlModelStatus } from "./tauri-bridge";

/**
 * The id of the speech model, the one `crates/ml-pipeline/models.json` pins
 * (#383). The model commands take it as `model`.
 */
export const SPEECH_MODEL_ID = "whisper-base";

/** What the model is called in the row. */
export const SPEECH_MODEL_TITLE = "Whisper base";

/** Where Download fetches from, as the row names it. */
export const SPEECH_MODEL_SOURCE = "Hugging Face";

/**
 * The paragraph under the row, whatever its state. It says what the model
 * is for, what the request is, and what leaves the computer when it is used:
 * no audio; the words the assistant is given do reach the AI provider, as
 * any tool result does.
 */
export const SPEECH_MODEL_NOTE =
  "Transcription runs on this computer with this model: once it is " +
  "downloaded, ask the assistant to transcribe a track. Download asks " +
  "huggingface.co for the model's three files. It sees your IP address " +
  "and that the request comes from edytlab, and nothing about your audio " +
  "is sent. Transcribing sends no audio anywhere either; the words it " +
  "finds reach your AI provider in the chat, like any tool result.";

/** Shown while the status is being asked for. */
export const SPEECH_MODEL_PENDING = "Checking…";

/** The confirmation before Remove deletes a model that is ready. */
export function removeQuestion(size: number): string {
  return (
    `Remove the speech model from this computer? ` +
    `You can download it again later (${megabytes(size)}).`
  );
}

/** The confirmation before what is on disk of an unfinished model is deleted. */
export const DISCARD_QUESTION =
  "Delete the files of the speech model that are on this computer? " +
  "Download would start again from the beginning.";

/** A whole number of megabytes (1 MB = 1,000,000 bytes): `139 MB`. */
export function megabytes(bytes: number): string {
  return `${Math.max(1, Math.round(bytes / 1_000_000))} MB`;
}

/** One decimal of a megabyte, no unit: `41.2`. */
function tenths(bytes: number): string {
  return (bytes / 1_000_000).toFixed(1);
}

/** Whole percent of `received` in `total`, never over 100. */
export function percent(received: number, total: number): number {
  if (total <= 0) return 0;
  return Math.min(100, Math.floor((received / total) * 100));
}

/** `41.2 of 138.6 MB (29%)`. */
export function progressText(received: number, total: number): string {
  return `${tenths(received)} of ${tenths(total)} MB (${percent(received, total)}%)`;
}

/**
 * Whether what is on disk of a model that is not ready is as big as the
 * model. That is a copy that has not been checked, not a partial download:
 * the backend says `not_downloaded` for files of the right size whose hash
 * is wrong (something changed them), and for a download cut off after its
 * last byte. Download re-checks them, and fetches again what does not match.
 */
export function isFullSize(bytes: number, size: number): boolean {
  return size > 0 && bytes >= size;
}

/** The line about what is already on disk of a model that is not ready. */
export function partialText(bytes: number, size: number): string {
  if (isFullSize(bytes, size)) {
    return (
      "Files the size of the model are on disk, but they have not been " +
      "checked. Download checks them, and fetches again what is wrong."
    );
  }
  return (
    `${tenths(bytes)} of ${tenths(size)} MB already downloaded. ` +
    `Download carries on from there.`
  );
}

/** The line for a model that is ready. */
export function readyText(status: MlModelStatus): string {
  return `Ready: ${megabytes(status.bytes)} on disk. Ask the assistant to transcribe a track.`;
}

/** The line for a download that failed. */
export function failedText(reason: string): string {
  return `The download failed: ${reason}`;
}
