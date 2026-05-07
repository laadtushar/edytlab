/**
 * useSession — owns project lifecycle + head-node tracking.
 *
 * Responsibilities:
 *  - holds the currently opened {@link ProjectInfo} (path + head node).
 *  - subscribes to `agent://node-created` so the head pointer follows
 *    every agent-produced edit without an explicit `getSessionHead`
 *    round-trip after each turn.
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
  onNodeCreated,
  openProject as bridgeOpenProject,
  renderPreview as bridgeRenderPreview,
  type NodeId,
  type ProjectInfo,
} from "../lib/tauri-bridge";

export interface UseSessionResult {
  project: ProjectInfo | null;
  /** Latest known head node id. Mirrors `project.head` but updated
   * eagerly off the event stream. */
  head: NodeId | null;
  openProject: (path: string) => Promise<void>;
  /** Render the latest head node to a temp WAV. Throws if no head. */
  renderHead: () => Promise<string>;
  /** Last error to bubble up from a session command, for surface in UI. */
  error: string | null;
}

export function useSession(): UseSessionResult {
  const [project, setProject] = useState<ProjectInfo | null>(null);
  const [head, setHead] = useState<NodeId | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Subscribe to node-created so the head pointer follows the agent's
  // edits. Cleanup detaches the listener — important under StrictMode.
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

  return { project, head, openProject, renderHead, error };
}
