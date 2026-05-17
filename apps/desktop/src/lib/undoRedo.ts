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
