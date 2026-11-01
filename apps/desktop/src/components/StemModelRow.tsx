/**
 * The stem model in Settings → Account (#385): `ModelRow` with the htdemucs
 * model's id and words (`lib/stemModel.ts`). Like the speech and speaker
 * models', it downloads only when the user presses Download; the rules are in
 * `ModelRow.tsx`, which holds the only call of `downloadMlModel`.
 */

import {
  STEM_DISCARD_QUESTION,
  STEM_MODEL_ID,
  STEM_MODEL_LICENCES,
  STEM_MODEL_NOTE,
  STEM_MODEL_SOURCE,
  STEM_MODEL_TITLE,
  stemReadyText,
  stemRemoveQuestion,
} from "../lib/stemModel";
import { SPEECH_MODEL_PENDING } from "../lib/speechModel";
import { ModelRow, type ModelCopy } from "./ModelRow";

const COPY: ModelCopy = {
  id: STEM_MODEL_ID,
  prefix: "stem-model",
  heading: "Stem model",
  title: STEM_MODEL_TITLE,
  source: STEM_MODEL_SOURCE,
  note: STEM_MODEL_NOTE,
  licences: STEM_MODEL_LICENCES,
  pending: SPEECH_MODEL_PENDING,
  subject: "the stem model",
  progressLabel: "Stem model download",
  removeQuestion: stemRemoveQuestion,
  discardQuestion: STEM_DISCARD_QUESTION,
  readyText: stemReadyText,
};

export function StemModelRow() {
  return <ModelRow copy={COPY} />;
}
