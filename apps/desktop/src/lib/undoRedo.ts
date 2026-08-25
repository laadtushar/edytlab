/**
 * The subset of a `KeyboardEvent` a chord test reads.
 *
 * Narrowing it to this is what lets the tests call the *same* predicate
 * App.tsx calls. The previous binding lived inline in a `useEffect`
 * inside App.tsx, which nothing could reach, so the only test naming
 * undo re-declared its own handler and asserted against that — deleting
 * the real binding outright left the suite green.
 */
export interface Chord {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
}

/**
 * Undo: Ctrl+Z, or ⌘Z on macOS.
 *
 * `metaKey` was missing, so on macOS the platform-standard chord did
 * nothing for session undo while the very same handler already accepted
 * ⌘ for zoom-to-selection and fit-to-window. #226's native Edit menu
 * made this worse rather than better: ⌘Z now works inside a focused
 * text field via the responder chain and still does nothing on the
 * timeline, so the chord looks live and behaves inconsistently.
 */
export function isUndoChord(e: Chord): boolean {
  if (!(e.metaKey || e.ctrlKey)) return false;
  if (e.shiftKey) return false;
  return e.key.toLowerCase() === "z";
}

/**
 * Redo: Ctrl/⌘+Y, or Ctrl/⌘+Shift+Z.
 *
 * The Shift+Z arm was dead on *every* platform, not just macOS. It
 * compared `e.key === "z"` while requiring `shiftKey`, and `key` carries
 * the shifted value — a Shift+Z press reports `"Z"`. So the branch could
 * not match its own guard, and Ctrl+Y was the only redo that ever
 * worked. Comparing case-insensitively is what makes both arms real.
 */
export function isRedoChord(e: Chord): boolean {
  if (!(e.metaKey || e.ctrlKey)) return false;
  if (e.key.toLowerCase() === "y") return true;
  return e.shiftKey && e.key.toLowerCase() === "z";
}

export function applyUndo(
  head: string,
  parent: string | null,
  redoStack: string[],
): { head: string; redoStack: string[] } | null {
  if (!parent) return null;
  return { head: parent, redoStack: [...redoStack, head] };
}

export function applyRedo(
  redoStack: string[],
): { head: string; redoStack: string[] } | null {
  if (redoStack.length === 0) return null;
  const next = redoStack[redoStack.length - 1];
  return { head: next, redoStack: redoStack.slice(0, -1) };
}
