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

/** What a keyboard event's target reads as, for `isTextEntry`. */
export interface KeyTarget {
  tagName?: string;
  type?: string;
  isContentEditable?: boolean;
}

/** `<input>` types that take no text, so have no text undo of their own. */
const NON_TEXT_INPUT_TYPES = new Set([
  "range",
  "checkbox",
  "radio",
  "button",
  "submit",
  "reset",
  "image",
  "color",
  "file",
]);

/**
 * Whether the focused element is somewhere text is typed, where Ctrl+Z
 * belongs to the field's own undo and not to the session's.
 *
 * Undo used to be skipped for every `INPUT`, which includes a range
 * slider. A fader is the control a person nudges and then wants to take
 * back, and with it focused Ctrl+Z did nothing until they clicked
 * somewhere else. Only inputs known to take no text are let through;
 * an unknown or missing `type` is text, so a new kind of field keeps
 * the field's own undo rather than losing it.
 */
export function isTextEntry(target: KeyTarget | null | undefined): boolean {
  if (!target) return false;
  if (target.isContentEditable) return true;
  if (target.tagName === "TEXTAREA") return true;
  if (target.tagName !== "INPUT") return false;
  return !NON_TEXT_INPUT_TYPES.has((target.type ?? "text").toLowerCase());
}
