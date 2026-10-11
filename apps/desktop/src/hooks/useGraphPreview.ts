/**
 * useGraphPreview — the node the history graph is previewing, kept apart
 * from the head (#522).
 *
 * A click on a bubble in the graph shows that node's mix; it does not move
 * the session. The store's head stays where it was, so the window's head
 * must too: the head is what undo, redo and the saved path
 * (`.audiograph/trail.json`, #453) follow, and a node that was only looked
 * at is not on that path. "Set as head" is the one way the graph moves it.
 *
 * A preview belongs to the head it was made under. It ends the moment the
 * head changes (an edit, an undo, Set as head, another project), because
 * the mix it rendered then belongs to a session that has moved on. It is
 * derived as well as cleared, so the render in between cannot show it.
 *
 * Clicking the head itself previews nothing apart from it: `previewId` is
 * `null` then, and the head's own mix is what is shown.
 */

import { useCallback, useEffect, useState } from "react";

import type { NodeId } from "../lib/tauri-bridge";

interface Chosen {
  id: NodeId;
  /** The head the window had when the node was chosen. */
  head: NodeId | null;
}

export interface GraphPreview {
  /** The previewed node; `null` when none is, or when it is the head. */
  previewId: NodeId | null;
  /** Preview `id`, under the head the window has now. */
  preview: (id: NodeId) => void;
  /** End the preview without a head move (a project opening). */
  clearPreview: () => void;
}

export function useGraphPreview(head: NodeId | null): GraphPreview {
  const [chosen, setChosen] = useState<Chosen | null>(null);

  const previewId =
    chosen !== null && chosen.head === head && chosen.id !== head ? chosen.id : null;

  // The head moved: what was chosen under the old one is over, and must
  // not come back if the head returns to where it was (undo after an edit).
  useEffect(() => {
    setChosen((c) => (c !== null && c.head !== head ? null : c));
  }, [head]);

  const preview = useCallback((id: NodeId) => setChosen({ id, head }), [head]);
  const clearPreview = useCallback(() => setChosen(null), []);

  return { previewId, preview, clearPreview };
}
