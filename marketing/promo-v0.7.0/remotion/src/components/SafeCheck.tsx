import React from 'react';
import {useLayoutEffect, useRef} from 'react';
import {inside, type Box, type Layout} from '../layout.ts';

/** Where `el` sits in the composition, from the layout offsets (offsetLeft/Top ignore transforms, so an entrance
 *  animation in flight does not matter) up to the element marked `data-cut-root`. */
export function layoutRect(el: HTMLElement): Box {
  let x = 0;
  let y = 0;
  let cur: HTMLElement | null = el;
  while (cur && !cur.hasAttribute('data-cut-root')) {
    x += cur.offsetLeft;
    y += cur.offsetTop;
    cur = cur.offsetParent as HTMLElement | null;
  }
  return {x, y, w: el.offsetWidth, h: el.offsetHeight};
}

/**
 * Every block of words the edit adds is measured as it is laid out and must lie inside the layout's safe area (for 9:16:
 * none in the top 14%, the bottom 35% or 6% at each side); a line that overflows its own box fails too. A violation
 * throws, so the render stops rather than ships a word under Meta's overlays. `marketing/launch-video` does the same
 * at build time (`assert_safe`); here the measuring is done by the browser that draws the frame.
 */
export function useSafe<T extends HTMLElement>(layout: Layout, what: string) {
  const ref = useRef<T>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = layoutRect(el);
    if (!inside(layout.safe, r)) {
      throw new Error(
        `[safe zone] "${what}" lies at x ${Math.round(r.x)}..${Math.round(r.x + r.w)}, y ${Math.round(r.y)}..${Math.round(r.y + r.h)}, outside ${layout.kind}'s safe area x ${layout.safe.x}..${layout.safe.x + layout.safe.w}, y ${layout.safe.y}..${layout.safe.y + layout.safe.h}`,
      );
    }
    if (el.scrollWidth > el.clientWidth + 1) {
      throw new Error(`[safe zone] "${what}" overflows its box (${el.scrollWidth}px of ${el.clientWidth}px)`);
    }
  });
  return ref;
}

/** A positioned box whose layout extent is checked against the safe area like any other words. */
export const GuardedDiv: React.FC<{layout: Layout; what: string; style: React.CSSProperties; children: React.ReactNode}> = ({layout, what, style, children}) => {
  const ref = useSafe<HTMLDivElement>(layout, what);
  return (
    <div ref={ref} style={style}>
      {children}
    </div>
  );
};
