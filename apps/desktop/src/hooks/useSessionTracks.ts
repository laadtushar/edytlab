import { skipToken, useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useLayoutEffect, useRef } from "react";
import type { Claim, OwnedControls } from "../lib/ownedControls";
import { isNoSession, listTracks, type TrackSummary } from "../lib/tauri-bridge";

const TRACKS_KEY = ["session", "tracks"] as const;
const EMPTY_TRACKS: TrackSummary[] = [];
interface TrackSnapshot {
  ticket: number;
  tracks: TrackSummary[];
}

/** The one track observer. Boot and command ordering remain the caller's. */
export function useSessionTracks(owner?: OwnedControls) {
  const client = useQueryClient();
  const requestRef = useRef(0);
  const lifetimeRef = useRef(0);
  const requestLifetimeRef = useRef(0);
  const activeRef = useRef(false);
  const current = useCallback(
    (ticket: number, lifetime: number) =>
      activeRef.current && lifetime === lifetimeRef.current && ticket === requestRef.current,
    [],
  );
  // Never rely on final-observer removal to cancel native promises: the
  // query function does not consume an AbortSignal. Cancellation prevents
  // cache adoption; it does not stop the already-running Tauri command.
  useLayoutEffect(() => {
    activeRef.current = true;
    return () => {
      activeRef.current = false;
      lifetimeRef.current++;
      requestRef.current++;
      void client.cancelQueries({ queryKey: TRACKS_KEY, exact: true });
    };
  }, [client]);

  const { data: snapshot } = useQuery<TrackSnapshot>({
    queryKey: TRACKS_KEY,
    queryFn: skipToken,
    enabled: false,
  });

  // A fresh ticket survives structural sharing of identical track arrays.
  // Only a list adopted by the observer may hand back controls, so a failed
  // read or an older cached success cannot confirm a newer claim.
  useLayoutEffect(() => {
    if (snapshot && current(snapshot.ticket, requestLifetimeRef.current)) {
      owner?.applied(snapshot.ticket);
    }
  }, [snapshot, owner, current]);

  const refreshTracks = useCallback(async (claim?: Claim): Promise<void> => {
    const ticket = ++requestRef.current;
    const lifetime = lifetimeRef.current;
    requestLifetimeRef.current = lifetime;
    if (claim) {
      if (!owner) throw new Error("Track confirmation requires its owning controls");
      owner.finish(claim, ticket);
    }
    try {
      // An explicit refresh starts an independent read, even before the
      // first data exists. Invalidation/refetch alone can reuse that read.
      await client.cancelQueries({ queryKey: TRACKS_KEY, exact: true });
      if (!current(ticket, lifetime)) return;
      await client.fetchQuery({
        queryKey: TRACKS_KEY,
        staleTime: 0,
        queryFn: async (): Promise<TrackSnapshot> => {
          try {
            return { ticket, tracks: await listTracks() };
          } catch (error) {
            if (isNoSession(error)) return { ticket, tracks: [] };
            throw error;
          }
        },
      });
    } catch (error) {
      if (current(ticket, lifetime)) throw error;
    }
  }, [client, current, owner]);

  const resetTracks = useCallback(() => {
    // Keep the counter monotonic across projects with identical head/IDs.
    requestRef.current++;
    void client.cancelQueries({ queryKey: TRACKS_KEY, exact: true });
  }, [client]);

  return { tracks: snapshot?.tracks ?? EMPTY_TRACKS, refreshTracks, resetTracks };
}
