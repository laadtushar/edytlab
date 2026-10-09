/**
 * useMarkers — the labels the label lane draws (#453).
 *
 * Owns the list `list_markers` answers and keeps it current. It is read:
 *
 * - at mount, and again on every `marker-changed` event — a label was
 *   added, renamed, moved or removed;
 * - whenever the head moves. Labels are part of the session state, so
 *   the head decides which ones exist. `set_head_to` emits no
 *   `marker-changed`, so an undo that takes back a label (or a redo that
 *   brings it back) would leave the lane showing the head it left. The
 *   head also moves with no label event for an agent edit (the agent's
 *   label tool emits only `node-created`) and for another project
 *   opening. Depending on `head` covers all of them with one read each.
 *
 * The latest request wins, as it does for the track list (#342): reads
 * overlap (a head move while a `marker-changed` read is in flight), and a
 * reply that arrives after a newer request was made is dropped, so an
 * older head's labels never replace a newer one's. A failed read empties
 * the list: `NoSession` is what a project with nothing in it yet answers.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import {
  listMarkers,
  onMarkerChanged,
  type Marker,
  type NodeId,
} from "../lib/tauri-bridge";

export function useMarkers(head: NodeId | null): Marker[] {
  const [markers, setMarkers] = useState<Marker[]>([]);
  const requestRef = useRef(0);

  const refresh = useCallback(() => {
    const request = ++requestRef.current;
    listMarkers()
      .then((next) => {
        if (request === requestRef.current) setMarkers(next);
      })
      .catch(() => {
        if (request === requestRef.current) setMarkers([]);
      });
  }, []);

  // Every head move: undo, redo, "Set as head", an agent edit, another
  // project. `set_head_to` emits no `marker-changed`, so the event alone
  // would leave the lane on the labels of the head just left (#453).
  useEffect(() => {
    refresh();
  }, [head, refresh]);

  // A label changed without the head moving as far as this hook knows.
  // Cleanup that runs before `listen` resolves detaches at once, so a
  // listener is never left behind (StrictMode mounts twice).
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    onMarkerChanged(refresh)
      .then((fn) => {
        if (cancelled) {
          fn();
        } else {
          unlisten = fn;
        }
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [refresh]);

  return markers;
}
