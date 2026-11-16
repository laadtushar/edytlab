/**
 * useTranscript: the transcript at the current head, and where it came from.
 *
 * The session's `transcript` query (`lib/sessionReads.ts`), read again
 * whenever the head moves: a cut shifts every later word, and a
 * transcription the agent just made arrives as a head move, so the pane
 * must not go on showing the timings (or the lack of a transcript) from
 * before. No session yet reads as no transcript; the pane's empty state is
 * the right answer.
 */

import { useQuery } from "@tanstack/react-query";

import { READS, readTranscript } from "../lib/sessionReads";
import { NO_TRANSCRIPT } from "../lib/transcriptSource";
import type { Transcript } from "../lib/tauri-bridge";

export function useTranscript(): Transcript {
  const { data } = useQuery({ queryKey: READS.transcript, queryFn: readTranscript });
  return data ?? NO_TRANSCRIPT;
}
