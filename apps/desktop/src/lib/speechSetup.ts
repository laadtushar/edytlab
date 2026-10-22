/**
 * The words of the card under a `transcribe` call that could not run (#384).
 *
 * `transcribe` answers four setup problems with an error that begins with a
 * marker; the backend turns the marker into a view (`ToolView` of type
 * `speech_setup`, with a `problem`), and the window switches on that. This is
 * what each problem says, and which of them can be fixed from Settings. Kept
 * apart from the card so the wording has one home and a test reads it without
 * rendering.
 *
 * Each says what the user does. None says to install anything for the speech
 * model: the user presses a button, and the app does the rest.
 */

import type { SpeechProblem } from "./tauri-bridge";

export interface SpeechSetupText {
  /** The card's heading. */
  title: string;
  /** What is wrong and what to do about it. */
  body: string;
  /** Whether Settings → Account is where it is fixed. */
  inSettings: boolean;
}

export const SPEECH_SETUP: Record<SpeechProblem, SpeechSetupText> = {
  model_not_downloaded: {
    title: "The speech model is not downloaded",
    body:
      "Transcription runs on this computer with a speech model, and it is not " +
      "here yet. Open Settings → Account and press Download beside it (about " +
      "139 MB, from Hugging Face; nothing is downloaded until you do). Then " +
      "ask for the transcript again.",
    inSettings: true,
  },
  runtime_unavailable: {
    title: "ONNX Runtime is not available",
    body:
      "Transcription needs the ONNX Runtime library, which the installers " +
      "include and a build from source does not. For a build from source, set " +
      "ORT_DYLIB_PATH to a copy of ONNX Runtime 1.23.2 (see docs/development-" +
      "guide.md, ML Model Files).",
    inSettings: false,
  },
  model_damaged: {
    title: "The speech model will not load",
    body:
      "The speech model is on this computer but could not be loaded, so the " +
      "download is damaged. Open Settings → Account, press Remove beside it, " +
      "then Download it again.",
    inSettings: true,
  },
  model_path_invalid: {
    title: "WHISPER_MODEL_PATH is not the speech model",
    body:
      "WHISPER_MODEL_PATH is set to something that is not the speech model's " +
      "folder (it holds onnx/encoder_model.onnx, " +
      "onnx/decoder_model_merged_int8.onnx and tokenizer.json). Correct it or " +
      "unset it; the model downloaded in Settings is used when it is unset.",
    inSettings: false,
  },
};
