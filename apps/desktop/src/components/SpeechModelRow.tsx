/**
 * The speech model in Settings → Account (#383): `ModelRow` with the speech
 * model's id and words (`lib/speechModel.ts`). The download rules are in
 * `ModelRow.tsx`, which holds the only call of `downloadMlModel`.
 */

import {
  DISCARD_QUESTION,
  SPEECH_MODEL_ID,
  SPEECH_MODEL_NOTE,
  SPEECH_MODEL_PENDING,
  SPEECH_MODEL_SOURCE,
  SPEECH_MODEL_TITLE,
  readyText,
  removeQuestion,
} from "../lib/speechModel";
import { ModelRow, type ModelCopy } from "./ModelRow";

const COPY: ModelCopy = {
  id: SPEECH_MODEL_ID,
  prefix: "speech-model",
  heading: "Speech model",
  title: SPEECH_MODEL_TITLE,
  source: SPEECH_MODEL_SOURCE,
  note: SPEECH_MODEL_NOTE,
  pending: SPEECH_MODEL_PENDING,
  subject: "the speech model",
  progressLabel: "Speech model download",
  removeQuestion,
  discardQuestion: DISCARD_QUESTION,
  readyText,
};

export function SpeechModelRow() {
  return <ModelRow copy={COPY} />;
}
