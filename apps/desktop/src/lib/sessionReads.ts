/**
 * What the window reads of the session, and what changes each read (ADR 0001).
 *
 * Every read is a TanStack query under `['session', …]`, made once and shared
 * by whatever shows it. A session-changing command does not tell each view to
 * read again: it names the reads it can change (`reads` in
 * `lib/sessionCommands.ts`), and `useSession` invalidates exactly those when
 * the command settles. That one table replaces the numbered track reads and
 * the `graphVersion` the views used to follow (#602).
 *
 * A read that cannot be answered is not a fault of the window. `NoSession` is
 * what a project with nothing in it yet says, and each read below turns it,
 * and a failed read of a view that only draws, into the empty answer.
 */

import {
  getGraph,
  getRenderWarnings,
  getTranscript,
  isNoSession,
  listMarkers,
  listTracks,
  type GraphSummary,
  type Marker,
  type NodeId,
  type RenderWarning,
  type TrackSummary,
  type Transcript,
} from "./tauri-bridge";
import { NO_TRANSCRIPT } from "./transcriptSource";

const SESSION = ["session"] as const;

/** The query key of each read. `warnings` is read per node: `[...WARNINGS, node]`. */
export const READS = {
  tracks: [...SESSION, "tracks"],
  graph: [...SESSION, "graph"],
  markers: [...SESSION, "markers"],
  transcript: [...SESSION, "transcript"],
  warnings: [...SESSION, "warnings"],
} as const;

export type ReadName = keyof typeof READS;

/**
 * What a head move can change: the lanes, the history graph, the labels and
 * the transcript. (A render's warnings are read per node, and the answer for
 * a node id never changes.)
 */
export const HEAD_READS: readonly ReadName[] = ["tracks", "graph", "markers", "transcript"];

/** A different project is open: nothing read of the last one applies. */
export const PROJECT_READS: readonly ReadName[] = [...HEAD_READS, "warnings"];

export const EMPTY_TRACKS: TrackSummary[] = [];
export const EMPTY_MARKERS: Marker[] = [];
export const NO_WARNINGS: readonly RenderWarning[] = [];

export async function readTracks(): Promise<TrackSummary[]> {
  try {
    return await listTracks();
  } catch (error) {
    if (isNoSession(error)) return EMPTY_TRACKS;
    throw error;
  }
}

/** The graph; a failed read is the view's to show, so it is not caught here. */
export function readGraph(): Promise<GraphSummary> {
  return getGraph();
}

/** A failed read empties the list: `NoSession` is a project with nothing in it yet. */
export async function readMarkers(): Promise<Marker[]> {
  try {
    return await listMarkers();
  } catch {
    return EMPTY_MARKERS;
  }
}

/** No session yet: the pane's empty state is the right answer. */
export async function readTranscript(): Promise<Transcript> {
  try {
    return await getTranscript();
  } catch {
    return NO_TRANSCRIPT;
  }
}

/** A read that fails shows nothing: this is a notice about the mix, not a command. */
export async function readWarnings(node: NodeId): Promise<readonly RenderWarning[]> {
  try {
    const warnings = await getRenderWarnings(node);
    return Array.isArray(warnings) ? warnings : NO_WARNINGS;
  } catch {
    return NO_WARNINGS;
  }
}
