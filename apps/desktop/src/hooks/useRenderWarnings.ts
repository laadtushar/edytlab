/**
 * useRenderWarnings: what a render of the node shown leaves out (#532).
 *
 * A stored effect the renderer cannot run is left out of the mix and said
 * so by `get_render_warnings`, which reads the node's chains and renders
 * nothing. It is asked for whenever the node changes, because
 * `render_preview` answers only a path, a cached preview has no report,
 * and a head move (`set_head_to`, undo) emits no event: the shown node is
 * the one thing every move, edit and preview changes.
 *
 * A node id hashes the state, so the answer for an id never changes. After
 * the effect is removed or bypassed the head is another node, whose answer
 * is empty, and the old one is not shown while the new one is read.
 *
 * A read that fails shows nothing. This is a notice about the mix, not a
 * command the user ran, and the render and the other reads report a
 * session that cannot be read.
 */

import { useEffect, useState } from "react";

import { getRenderWarnings, type NodeId, type RenderWarning } from "../lib/tauri-bridge";

const NONE: readonly RenderWarning[] = [];

export function useRenderWarnings(node: NodeId | null): readonly RenderWarning[] {
  const [answer, setAnswer] = useState<{
    node: NodeId;
    warnings: readonly RenderWarning[];
  } | null>(null);

  useEffect(() => {
    if (!node) return;
    let cancelled = false;
    getRenderWarnings(node)
      .then((warnings) => (Array.isArray(warnings) ? warnings : NONE))
      .catch(() => NONE)
      .then((warnings) => {
        if (!cancelled) setAnswer({ node, warnings });
      });
    return () => {
      cancelled = true;
    };
  }, [node]);

  return node !== null && answer?.node === node ? answer.warnings : NONE;
}
