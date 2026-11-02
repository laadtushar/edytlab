import { useLayoutEffect, useRef, type ComponentPropsWithoutRef } from "react";

type Entry = {
  element: HTMLDivElement;
  layer: HTMLElement;
  zIndex: string;
  close: () => void;
  opener: HTMLElement | null;
  outsideDismiss: boolean;
};
const stack: Entry[] = [];
let restoreBackground = () => {};

/** Read at event time, including the key event that opens or closes a dialog. */
export function hasOpenOverlay(): boolean {
  return stack.length > 0;
}

function focusable(element: HTMLElement): HTMLElement[] {
  return Array.from(element.querySelectorAll<HTMLElement>(
    'button, input, select, textarea, a[href], [tabindex], [contenteditable="true"]',
  )).filter((node) => {
    if (node.tabIndex < 0 || node.matches(':disabled, input[type="hidden"]') ||
      node.closest('[inert], [hidden], [aria-hidden="true"]') ||
      getComputedStyle(node).visibility === "hidden") return false;
    for (let parent: HTMLElement | null = node; parent; parent = parent.parentElement) {
      if (getComputedStyle(parent).display === "none") return false;
      if (parent === element) break;
    }
    return true;
  });
}

function focusInside(entry: Entry) {
  const initial = entry.element.querySelector<HTMLElement>("[data-dialog-initial-focus]");
  (initial ?? focusable(entry.element)[0] ?? entry.element).focus();
}

/** Inert siblings along the ancestor path, so this also works for inline popovers. */
function syncBackground() {
  restoreBackground();
  const top = stack[stack.length - 1];
  if (!top) return;
  const saved: { element: Element; inert: string | null }[] = [];
  let child: Element = top.element;
  while (child.parentElement) {
    for (const sibling of child.parentElement.children) {
      if (sibling === child) continue;
      saved.push({ element: sibling, inert: sibling.getAttribute("inert") });
      sibling.setAttribute("inert", "");
    }
    child = child.parentElement;
    if (child === document.body) break;
  }
  for (const [index, entry] of stack.entries()) {
    entry.layer.style.zIndex = String(50 + index);
    if (entry === top) entry.element.setAttribute("aria-modal", "true");
    else entry.element.removeAttribute("aria-modal");
  }
  restoreBackground = () => {
    for (const { element, inert } of saved) {
      // Animated overlays own their inactive state. A sibling can close or
      // reopen while covered; never restore its obsolete pre-stack state.
      if (element.matches("[data-overlay-layer], [data-overlay-dialog]")) {
        element.toggleAttribute("inert", element.getAttribute("aria-hidden") === "true");
      } else if (inert === null) element.removeAttribute("inert");
      else element.setAttribute("inert", inert);
    }
    restoreBackground = () => {};
  };
}

function onKeyDown(event: KeyboardEvent) {
  const top = stack[stack.length - 1];
  if (!top) return;
  if (event.key === "Escape") {
    event.preventDefault();
    // Capture once, before component/document/window listeners can also close.
    event.stopImmediatePropagation();
    if (!event.repeat) top.close();
  } else if (event.key === "Tab") {
    event.preventDefault();
    const items = focusable(top.element);
    const index = items.indexOf(document.activeElement as HTMLElement);
    const next = event.shiftKey
      ? (index <= 0 ? items.length - 1 : index - 1)
      : (index + 1) % items.length;
    (items[next] ?? top.element).focus();
  }
}

function onFocus(event: FocusEvent) {
  const top = stack[stack.length - 1];
  if (top && !top.element.contains(event.target as Node)) focusInside(top);
}

function onMouseDown(event: MouseEvent) {
  const top = stack[stack.length - 1];
  if (top?.outsideDismiss && !top.element.contains(event.target as Node)) {
    event.preventDefault();
    event.stopImmediatePropagation();
    top.close();
  }
}

interface OverlayDialogProps extends ComponentPropsWithoutRef<"div"> {
  active: boolean;
  onClose: () => void;
  outsideDismiss?: boolean;
  "aria-labelledby": string;
}

/** Shared dialog lifetime. A leaving animation is inert and no longer owns keys/focus. */
export function OverlayDialog({ active, onClose, outsideDismiss = false, children, ...props }: OverlayDialogProps) {
  const ref = useRef<HTMLDivElement>(null);
  const close = useRef(onClose);
  close.current = onClose;
  useLayoutEffect(() => {
    const element = ref.current;
    if (!active || !element) return;
    const layer = element.closest<HTMLElement>("[data-overlay-layer]") ?? element;
    const entry: Entry = {
      layer, zIndex: layer.style.zIndex, element, close: () => close.current(), outsideDismiss,
      opener: document.activeElement instanceof HTMLElement ? document.activeElement : null,
    };
    stack.push(entry);
    syncBackground();
    focusInside(entry);
    if (stack.length === 1) {
      window.addEventListener("keydown", onKeyDown, true);
      window.addEventListener("focusin", onFocus, true);
      window.addEventListener("mousedown", onMouseDown, true);
    }
    // Newly mounted siblings must be inert as well as those present on open.
    const observer = new MutationObserver(syncBackground);
    observer.observe(document.body, { childList: true, subtree: true });
    return () => {
      observer.disconnect();
      const wasTop = stack[stack.length - 1] === entry;
      stack.splice(stack.indexOf(entry), 1);
      element.removeAttribute("aria-modal");
      layer.style.zIndex = entry.zIndex;
      syncBackground();
      if (!stack.length) {
        window.removeEventListener("keydown", onKeyDown, true);
        window.removeEventListener("focusin", onFocus, true);
        window.removeEventListener("mousedown", onMouseDown, true);
      }
      if (wasTop) {
        const top = stack[stack.length - 1];
        // React restores the pre-commit focus after layout cleanups. Restore
        // ours afterwards, unless another dialog has opened in the meantime.
        queueMicrotask(() => {
          if (stack[stack.length - 1] !== top) return;
          if (entry.opener?.isConnected && !entry.opener.closest("[inert]")) entry.opener.focus();
          else if (top) focusInside(top);
        });
      }
    };
  }, [active, outsideDismiss]);

  return <div {...props} ref={ref} data-overlay-dialog role="dialog" tabIndex={-1}
    inert={!active} aria-hidden={!active || undefined}
    onClick={(event) => { event.stopPropagation(); props.onClick?.(event); }}>
    {children}
  </div>;
}
