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
 * A node id hashes the state, so the answer for an id never changes: the
 * query is keyed by the node and never goes stale, and it is read again
 * only when a different project opens. After the effect is removed or
 * bypassed the head is another node, whose answer is empty, and the old one
 * is not shown while the new one is read.
 *
 * A read that fails shows nothing. This is a notice about the mix, not a
 * command the user ran, and the render and the other reads report a
 * session that cannot be read.
 */

import { useQuery } from "@tanstack/react-query";

import { NO_WARNINGS, READS, readWarnings } from "../lib/sessionReads";
import type { NodeId, RenderWarning } from "../lib/tauri-bridge";

export function useRenderWarnings(node: NodeId | null): readonly RenderWarning[] {
  const { data } = useQuery({
    queryKey: [...READS.warnings, node],
    queryFn: () => readWarnings(node as NodeId),
    enabled: node !== null,
  });
  return node !== null && data ? data : NO_WARNINGS;
}
