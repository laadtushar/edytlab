/**
 * useSession — owns project lifecycle + head-node tracking.
 *
 * Responsibilities:
 *  - holds the currently opened {@link ProjectInfo} (path + head node).
 *  - subscribes to `agent://node-created` so the head pointer follows
 *    every agent-produced edit without an explicit `getSessionHead`
 *    round-trip after each turn — and reads `getSessionHead` once at
 *    mount, for the head that was already there before any event.
 *  - exposes a `renderPreview` helper that resolves the latest node id
 *    to a temp WAV path via the bridge.
 *
 * Pure: the hook only schedules effects in `useEffect` and never
 * mutates state during render. The single `useEffect` that wires the
 * `onNodeCreated` listener returns its async unlisten so React can
 * detach it on unmount; this avoids the duplicated-listener leak that
 * would otherwise happen under StrictMode's double-mount.
 */

import { useCallback, useEffect, useState } from "react";

import {
  getSessionHead,
  onNodeCreated,
  openProject as bridgeOpenProject,
  renderPreview as bridgeRenderPreview,
  type NodeId,
  type ProjectInfo,
} from "../lib/tauri-bridge";

export interface UseSessionResult {
  project: ProjectInfo | null;
  /** Latest known head node id: read once at mount (a returning user's
   * restored head), then kept current off the event stream. */
  head: NodeId | null;
  openProject: (path: string) => Promise<void>;
  /** Render the latest head node to a temp WAV. Throws if no head. */
  renderHead: () => Promise<string>;
  /**
   * Update the locally-tracked head pointer. Used by the M25 graph
   * view when the user clicks a node: the canvas pane keys off `head`
   * for render-preview, so updating it here is enough to make the
   * audio re-render reflect the selected node. Note that until M24
   * lands the backend `set_head` tool, this only changes the
   * frontend's idea of the head — the agent will still operate on
   * the on-disk head. The graph view's "Set as head" right-click
   * action (which would persist) is gated on M24.
   */
  setHeadLocal: (nodeId: NodeId) => void;
  /** Last error to bubble up from a session command, for surface in UI. */
  error: string | null;
}

export function useSession(): UseSessionResult {
  const [project, setProject] = useState<ProjectInfo | null>(null);
  const [head, setHead] = useState<NodeId | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Subscribe to node-created so the head pointer follows the agent's
  // edits. Cleanup detaches the listener — important under StrictMode.
  //
  // And read the head once, at mount. The subscription only reports
  // *changes*; a head that was already there when the app started —
  // every returning user's, since the backend reopens the project and
  // restores `HEAD` from disk — never arrives as an event. So a returning
  // user had no head at all, and "preview" silently did nothing (#332).
  //
  // The event wins a race: if an edit lands before this read resolves,
  // its head is newer than the one read, so the read only fills a head
  // that is still unknown. A rejection leaves it unknown, which is what a
  // fresh project's `NoSession` means and what the boot `listTracks`
  // does with a failure too.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    onNodeCreated((nodeId) => {
      setHead(nodeId);
    }).then((fn) => {
      if (cancelled) {
        fn();
      } else {
        unlisten = fn;
      }
    });
    getSessionHead()
      .then((restored) => {
        if (!cancelled) setHead((current) => current ?? restored);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const openProject = useCallback(async (path: string) => {
    try {
      const info = await bridgeOpenProject(path);
      setProject(info);
      setHead(info.head);
      setError(null);
    } catch (e) {
      setError(String(e));
      throw e;
    }
  }, []);

  const renderHead = useCallback(async (): Promise<string> => {
    if (!head) {
      throw new Error("no node to render");
    }
    return bridgeRenderPreview(head);
  }, [head]);

  const setHeadLocal = useCallback((nodeId: NodeId) => {
    setHead(nodeId);
  }, []);

  return { project, head, openProject, renderHead, setHeadLocal, error };
}
