/**
 * The speaker model in Settings → Account (#168): `ModelRow` with the
 * speaker-diarization model's id and words (`lib/speakerModel.ts`). Like the
 * speech model's, it downloads only when the user presses Download; the rules
 * are in `ModelRow.tsx`, which holds the only call of `downloadMlModel`.
 */

import {
  SPEAKER_DISCARD_QUESTION,
  SPEAKER_MODEL_ID,
  SPEAKER_MODEL_LICENCES,
  SPEAKER_MODEL_NOTE,
  SPEAKER_MODEL_SOURCE,
  SPEAKER_MODEL_TITLE,
  speakerReadyText,
  speakerRemoveQuestion,
} from "../lib/speakerModel";
import { SPEECH_MODEL_PENDING } from "../lib/speechModel";
import { ModelRow, type ModelCopy } from "./ModelRow";

const COPY: ModelCopy = {
  id: SPEAKER_MODEL_ID,
  prefix: "speaker-model",
  heading: "Speaker model",
  title: SPEAKER_MODEL_TITLE,
  source: SPEAKER_MODEL_SOURCE,
  note: SPEAKER_MODEL_NOTE,
  licences: SPEAKER_MODEL_LICENCES,
  pending: SPEECH_MODEL_PENDING,
  subject: "the speaker model",
  progressLabel: "Speaker model download",
  removeQuestion: speakerRemoveQuestion,
  discardQuestion: SPEAKER_DISCARD_QUESTION,
  readyText: speakerReadyText,
};

export function SpeakerModelRow() {
  return <ModelRow copy={COPY} />;
}
