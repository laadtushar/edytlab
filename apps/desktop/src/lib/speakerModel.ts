/**
 * The words of the Settings row about the speaker model (#168).
 *
 * The same promises as the speech model's (`lib/speechModel.ts`): the files
 * are on disk afterwards, checked against their hashes; the words say what
 * they are for (finding who speaks when, on this computer, once they are
 * there); no state calls the model "enabled" or "installed" as a feature; and
 * the only network request in the row is the one the user's click makes, to
 * huggingface.co, which no copy hides. Using the model makes no request of
 * its own.
 */

import type { MlModelStatus } from "./tauri-bridge";
import { megabytes } from "./speechModel";

/**
 * The id of the speaker model, the one `crates/ml-pipeline/models.json`
 * pins (#168): two ONNX files treated as one model. The model commands take
 * it as `model`.
 */
export const SPEAKER_MODEL_ID = "speaker-diarization";

/** What the model is called in the row. */
export const SPEAKER_MODEL_TITLE = "Speaker diarization (pyannote, WeSpeaker)";

/** Where Download fetches from, as the row names it. */
export const SPEAKER_MODEL_SOURCE = "Hugging Face";

/**
 * The paragraph under the row, whatever its state: what the model is for,
 * what the request is, and what leaves the computer when it is used: no
 * audio; the speaker turns the assistant is given do reach the AI provider,
 * as any tool result does.
 */
export const SPEAKER_MODEL_NOTE =
  "Finding who speaks when runs on this computer with this model: once it " +
  "is downloaded, ask the assistant to tell the speakers apart on a track. " +
  "Download asks huggingface.co for the model's two files. It sees your IP " +
  "address and that the request comes from edytlab, and nothing about your " +
  "audio is sent. Using the model sends no audio anywhere either; the " +
  "speaker turns it finds reach your AI provider in the chat, like any " +
  "tool result.";

/**
 * The credit the two licences ask for. CC BY 4.0 needs the creators named,
 * the licence linked (the Terms page links it) and a note that the files are
 * used as published; MIT needs its notice kept.
 */
export const SPEAKER_MODEL_LICENCES =
  "Speaker turns: pyannote segmentation-3.0 (MIT licence, © CNRS), as an " +
  "ONNX export by the onnx-community. Voiceprints: " +
  "WeSpeaker ResNet34-LM by the WeSpeaker authors (CC BY 4.0). Full credit " +
  "is on the Terms page.";

/** The confirmation before Remove deletes a model that is ready. */
export function speakerRemoveQuestion(size: number): string {
  return (
    `Remove the speaker model from this computer? ` +
    `You can download it again later (${megabytes(size)}).`
  );
}

/** The confirmation before what is on disk of an unfinished model is deleted. */
export const SPEAKER_DISCARD_QUESTION =
  "Delete the files of the speaker model that are on this computer? " +
  "Download would start again from the beginning.";

/** The line for a model that is ready. */
export function speakerReadyText(status: MlModelStatus): string {
  return `Ready: ${megabytes(status.bytes)} on disk. Ask the assistant who speaks when on a track.`;
}
