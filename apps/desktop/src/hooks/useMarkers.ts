/**
 * useMarkers: the labels the label lane draws (#453).
 *
 * The list `list_markers` answers, as the session's `markers` query
 * (`lib/sessionReads.ts`). It is read at mount, and again whenever the
 * session says it may have changed (`useSession`): a `marker-changed` event,
 * and every head move, because labels are part of the session state, so the
 * head decides which ones exist. `set_head_to` emits no `marker-changed`, so
 * an undo that takes back a label would otherwise leave the lane showing the
 * head it left; the head also moves with no label event for an agent edit
 * and for another project opening.
 *
 * A reply older than the newest request is never drawn, as for every query,
 * so an older head's labels never replace a newer one's. A failed read
 * empties the list: `NoSession` is what a project with nothing in it yet
 * answers.
 */

import { useQuery } from "@tanstack/react-query";

import { EMPTY_MARKERS, READS, readMarkers } from "../lib/sessionReads";
import type { Marker } from "../lib/tauri-bridge";

export function useMarkers(): Marker[] {
  const { data } = useQuery({ queryKey: READS.markers, queryFn: readMarkers });
  return data ?? EMPTY_MARKERS;
}
