/**
 * The words of the Settings row about the speech model (#383).
 *
 * Kept apart from the component so the wording has one home and a test
 * can read it without rendering. The row is honest about what a download
 * is: the model is on disk afterwards, and that is all. Transcription is
 * not shipped (#384), so no state here says it works, is enabled or is
 * "installed" as a feature; and the only network request in the row is the
 * one the user's click makes, to huggingface.co, which no copy hides.
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
 * is for, that downloading turns nothing on, and what the request is.
 */
export const SPEECH_MODEL_NOTE =
  "Transcription is not available yet. This is the model it will use, " +
  "and downloading it does not turn anything on. Download makes one " +
  "request to huggingface.co; nothing about your audio is sent.";

/** Shown while the status is being asked for. */
export const SPEECH_MODEL_PENDING = "Checking…";

/** The confirmation before Remove deletes a model that is ready. */
export function removeQuestion(size: number): string {
  return (
    `Remove the speech model from this computer? ` +
    `You can download it again later (${megabytes(size)}).`
  );
}

/** The confirmation before a partial download is deleted. */
export const DISCARD_QUESTION =
  "Delete the part of the speech model that has been downloaded? " +
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

/** The line about what is already on disk of a model that is not complete. */
export function partialText(bytes: number, size: number): string {
  return (
    `${tenths(bytes)} of ${tenths(size)} MB already downloaded. ` +
    `Download carries on from there.`
  );
}

/** The line for a model that is ready. */
export function readyText(status: MlModelStatus): string {
  return `Ready: ${megabytes(status.bytes)} on disk. Nothing uses it yet.`;
}

/** The line for a download that failed. */
export function failedText(reason: string): string {
  return `The download failed: ${reason}`;
}
