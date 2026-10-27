/**
 * The words of the Settings row about the stem model (#385).
 *
 * The same promises as the speech model's (`lib/speechModel.ts`) and the
 * speaker model's (`lib/speakerModel.ts`): the files are on disk afterwards,
 * checked against their hashes; the words say what they are for (splitting a
 * recording into vocals, drums, bass and other, on this computer, once they
 * are there); no state calls the model "enabled" or "installed" as a feature;
 * and the only network request in the row is the one the user's click makes,
 * to huggingface.co, which no copy hides. Using the model makes no request of
 * its own.
 */

import type { MlModelStatus } from "./tauri-bridge";
import { megabytes } from "./speechModel";

/**
 * The id of the stem model, the one `crates/ml-pipeline/models.json` pins
 * (#385): the htdemucs graph and its weights, two files treated as one model.
 * The model commands take it as `model`.
 */
export const STEM_MODEL_ID = "htdemucs";

/** What the model is called in the row. */
export const STEM_MODEL_TITLE = "Stem separation (HT-Demucs v4)";

/** Where Download fetches from, as the row names it. */
export const STEM_MODEL_SOURCE = "Hugging Face";

/**
 * The paragraph under the row, whatever its state: what the model is for,
 * what the request is, what it costs to run, and what leaves the computer
 * when it is used: no audio; the stems are files in the project, and the
 * assistant is told where they are, as any tool result is.
 */
export const STEM_MODEL_NOTE =
  "Splitting a recording into vocals, drums, bass and other runs on this " +
  "computer with this model: once it is downloaded, ask the assistant to " +
  "separate the stems of a track. It needs about 4 GB of free memory and " +
  "takes about half the length of the audio. Download asks huggingface.co " +
  "for the model's two files. It sees your IP address and that the request " +
  "comes from edytlab, and nothing about your audio is sent. Using the " +
  "model sends no audio anywhere either; the stems are written into your " +
  "project, and the assistant is told where, like any tool result.";

/**
 * The credit the licences ask for: MIT needs its notice kept, and the Terms
 * page has it.
 */
export const STEM_MODEL_LICENCES =
  "HT-Demucs by Meta (MIT licence, © Meta Platforms, Inc. and affiliates), " +
  "as an ONNX export by Intel (MIT licence). Full credit is on the Terms page.";

/** The confirmation before Remove deletes a model that is ready. */
export function stemRemoveQuestion(size: number): string {
  return (
    `Remove the stem model from this computer? ` +
    `You can download it again later (${megabytes(size)}).`
  );
}

/** The confirmation before what is on disk of an unfinished model is deleted. */
export const STEM_DISCARD_QUESTION =
  "Delete the files of the stem model that are on this computer? " +
  "Download would start again from the beginning.";

/** The line for a model that is ready. */
export function stemReadyText(status: MlModelStatus): string {
  return `Ready: ${megabytes(status.bytes)} on disk. Ask the assistant to separate the stems of a track.`;
}
