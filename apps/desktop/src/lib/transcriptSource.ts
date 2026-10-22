/**
 * Which track the Transcript tab may cut, and why it may not (#384).
 *
 * A transcript's word times are seconds of the audio its words were made from.
 * Cut a track at those times and it is that track's audio that goes, so the tab
 * cuts only the track the transcript is of, and says plainly when there is
 * none. Cutting track 0 whatever the transcript was of would delete audio from
 * the wrong track the day `transcribe` can be asked about track 1.
 */

import type { Transcript, TranscriptSource } from "./tauri-bridge";

/** No transcript: no words, and a source that allows no cut. */
export const NO_TRANSCRIPT: Transcript = {
  words: [],
  source: { kind: "unknown" },
};

/** The track to cut by this transcript's words, or `null` if there is none. */
export function cutTrack(source: TranscriptSource): number | null {
  return source.kind === "track" ? source.track : null;
}

/**
 * Why the tab does not cut by this transcript, in one line, or `null` when it
 * does.
 */
export function cutBlockedReason(source: TranscriptSource): string | null {
  switch (source.kind) {
    case "track":
      return null;
    case "file":
      return (
        "This transcript was made from a file, so its times are the file's, " +
        "not a track's. Cutting by it is off here; ask the assistant to " +
        "transcribe the track itself."
      );
    case "gone":
      return (
        "The track this transcript was made from has been removed, so its " +
        "words describe no audio here. Cutting by it is off."
      );
    case "unknown":
      return (
        "This transcript does not say which track it was made from, so " +
        "cutting by it is off here. Ask the assistant to transcribe the " +
        "track again."
      );
  }
}
