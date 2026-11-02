import { useId, useLayoutEffect, useRef } from "react";

type Entry = { node: HTMLDivElement; close: () => void; previous: HTMLElement | null };
const stack: Entry[] = [];
const madeInert = new Set<Element>();

export function hasOpenDialog(): boolean {
  return stack.length > 0;
}

export function isTopDialog(node: HTMLElement | null): boolean {
  return stack[stack.length - 1]?.node === node;
}

// Inert siblings on every level, including dialogs below the top one.
// Keep existing inert attributes intact when the stack changes or empties.
function updateInert() {
  for (const node of madeInert) node.removeAttribute("inert");
  madeInert.clear();
  let branch: Element | undefined = stack[stack.length - 1]?.node;
  while (branch && branch !== document.body) {
    const parent: HTMLElement | null = branch.parentElement;
    if (!parent) break;
    for (const sibling of parent.children) {
      if (sibling !== branch && !sibling.hasAttribute("inert")) {
        sibling.setAttribute("inert", "");
        madeInert.add(sibling);
      }
    }
    branch = parent;
  }
}

function focusable(node: HTMLElement): HTMLElement[] {
  return Array.from(node.querySelectorAll<HTMLElement>(
    'button, input, select, textarea, a[href], [tabindex], [contenteditable="true"]',
  )).filter((el) => {
    if (el.tabIndex < 0 || el.matches(":disabled") || el.closest("[inert], [hidden]")) return false;
    for (let current: HTMLElement | null = el; current; current = current.parentElement) {
      const style = getComputedStyle(current);
      if (style.display === "none" || style.visibility === "hidden") return false;
      if (current === node) break;
    }
    return true;
  }).sort((a, b) => (a.tabIndex || Infinity) - (b.tabIndex || Infinity));
}

function focusInside(node: HTMLElement) {
  (focusable(node)[0] ?? node).focus();
}

function onKey(event: KeyboardEvent) {
  const top = stack[stack.length - 1];
  if (!top || event.isComposing) return;
  if (event.key === "Escape") {
    event.preventDefault();
    // Capture before any component's document/window shortcut listener.
    // The same event must not reach a lower dialog after React closes this one.
    event.stopImmediatePropagation();
    if (!event.repeat) top.close();
  } else if (event.key === "Tab") {
    event.preventDefault();
    const items = focusable(top.node);
    const index = items.indexOf(document.activeElement as HTMLElement);
    const next = index < 0 ? (event.shiftKey ? items.length - 1 : 0)
      : (index + (event.shiftKey ? -1 : 1) + items.length) % items.length;
    (items[next] ?? top.node).focus();
  }
}

function onFocus(event: FocusEvent) {
  const top = stack[stack.length - 1];
  if (top && !top.node.contains(event.target as Node)) focusInside(top.node);
}

/** Attach to the dialog card, and put headingId on its heading. */
export function useDialog(open: boolean, onClose: () => void) {
  const ref = useRef<HTMLDivElement>(null);
  const headingId = useId();
  const close = useRef(onClose);
  useLayoutEffect(() => { close.current = onClose; });

  useLayoutEffect(() => {
    const node = ref.current;
    if (!open || !node) return;
    const entry: Entry = {
      node,
      close: () => close.current(),
      previous: document.activeElement instanceof HTMLElement ? document.activeElement : null,
    };
    stack.push(entry);
    if (stack.length === 1) {
      window.addEventListener("keydown", onKey, true);
      window.addEventListener("focusin", onFocus, true);
    }
    updateInert();
    // Observe new background siblings too (toasts, portals, app panels).
    const observer = new MutationObserver(updateInert);
    observer.observe(document.body, { childList: true, subtree: true });
    const initial = node.querySelector<HTMLElement>("[data-dialog-initial-focus]");
    if (initial) initial.focus();
    else focusInside(node);
    return () => {
      observer.disconnect();
      const wasTop = stack[stack.length - 1] === entry;
      stack.splice(stack.indexOf(entry), 1);
      updateInert();
      if (!stack.length) {
        window.removeEventListener("keydown", onKey, true);
        window.removeEventListener("focusin", onFocus, true);
      }
      // React can restore the old focused node during its commit. Restore
      // after that commit, and never steal focus from a newly opened dialog.
      if (wasTop) queueMicrotask(() => {
        const top = stack[stack.length - 1];
        if (top?.node === node) return;
        if (entry.previous?.isConnected && !entry.previous.closest("[inert]") &&
            (!top || top.node.contains(entry.previous))) entry.previous.focus();
        else if (top) focusInside(top.node);
      });
    };
  }, [open]);

  return {
    ref,
    headingId,
    dialogProps: {
      ref,
      role: "dialog" as const,
      tabIndex: -1,
      "aria-labelledby": headingId,
      "aria-modal": open ? true : undefined,
      "aria-hidden": !open ? true : undefined,
      inert: !open,
    },
  };
}
