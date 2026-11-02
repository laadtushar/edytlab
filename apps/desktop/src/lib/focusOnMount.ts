/**
 * A ref callback that puts focus in a field the moment it appears.
 *
 * For the inline editors a person has just asked for (rename a node, rename a
 * track, edit a plan step): the field is new on screen and the next key is
 * meant for it. It does the same as the `autoFocus` attribute, which React
 * implements as a `focus()` at mount, but names the intent, and it is not the
 * attribute that linters (and S9379) are right to flag on a page that loads
 * with focus already taken. Use it only for a field that did not exist until
 * the user acted.
 *
 * Module-level and stable, so React calls it once, when the element mounts.
 */
export function focusOnMount(el: HTMLElement | null): void {
  el?.focus();
}
