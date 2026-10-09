"use client";

import { gsap, useGSAP, motionOk, NO_PREFERENCE, HOVER_MOTION } from "@/lib/gsap";

/** Opted in with an attribute, so a server component can ask for it. */
const PRESS = "[data-press]";
const LIFT = "[data-lift]";
const NUDGE = "[data-nudge]";
const SWEEP = "[data-sweep]";
const HOVERABLE = [PRESS, LIFT, NUDGE, SWEEP].join(", ");

function closest(target: EventTarget | null, selector: string): HTMLElement | null {
  return target instanceof Element ? target.closest<HTMLElement>(selector) : null;
}

/** True when the pointer moved between two parts of the same element. */
function within(el: HTMLElement, related: EventTarget | null) {
  return related instanceof Node && el.contains(related);
}

/** Put every opted-in element back to its stylesheet state. */
function clear(selector: string, props: string) {
  const els = document.querySelectorAll(selector);
  if (els.length) gsap.set(els, { clearProps: props });
}

/**
 * Press and hover feedback for the whole site, from one place.
 *
 * - `data-press` — the control dips slightly while held and springs back
 *   on release, and its icon hops on hover. Every `Button` carries it.
 * - `data-lift` — a card rises a few pixels under a fine pointer.
 * - `data-nudge="3"` — a link slides that many pixels along x on hover
 *   (negative for a "back" link).
 * - `data-sweep` — an underline that draws in from the left on hover
 *   and leaves to the right, rather than shrinking back the way it
 *   came. The line is CSS (`--sweep` in `globals.css`); GSAP only moves
 *   the number.
 *
 * Delegated from the document rather than wired per element: most of
 * the cards and links on the site are rendered by server components,
 * which cannot hold a hook, and one listener covers every page —
 * including ones written after this — without a wrapper around each
 * control.
 *
 * Mounted once, in the root layout. Nothing here changes layout: every
 * property is a transform or a custom property feeding one, so a
 * pressed button never nudges its neighbours. With reduced motion none
 * of it is registered; the controls keep their colour changes, and the
 * sweep falls back to a plain underline, both of which are CSS.
 */
export function MicroInteractions() {
  useGSAP(() => {
    const mm = motionOk();

    mm.add(NO_PREFERENCE, () => {
      const down = (e: PointerEvent) => {
        if (e.button !== 0) return;
        const el = closest(e.target, PRESS);
        if (!el) return;
        gsap.to(el, { scale: 0.96, duration: 0.12, ease: "power2.out", overwrite: "auto" });
        // Released anywhere — a press that drags off the control still
        // has to spring back.
        const done = new AbortController();
        const up = () => {
          gsap.to(el, { scale: 1, duration: 0.45, ease: "back.out(3)", overwrite: "auto" });
          done.abort();
        };
        window.addEventListener("pointerup", up, { signal: done.signal });
        window.addEventListener("pointercancel", up, { signal: done.signal });
      };

      document.addEventListener("pointerdown", down);
      return () => {
        document.removeEventListener("pointerdown", down);
        clear(PRESS, "scale");
      };
    });

    mm.add(HOVER_MOTION, () => {
      // `pointerover`/`pointerout` bubble where enter/leave do not; the
      // `within` check ignores moves between an element's own children,
      // which would otherwise restart the tween on every one.
      const over = (e: PointerEvent) => {
        if (!closest(e.target, HOVERABLE)) return;

        const lift = closest(e.target, LIFT);
        if (lift && !within(lift, e.relatedTarget)) {
          gsap.to(lift, { y: -4, duration: 0.35, overwrite: "auto" });
        }

        const nudge = closest(e.target, NUDGE);
        if (nudge && !within(nudge, e.relatedTarget)) {
          gsap.to(nudge, { x: Number(nudge.dataset.nudge) || 3, duration: 0.3, overwrite: "auto" });
        }

        const sweep = closest(e.target, SWEEP);
        if (sweep && !within(sweep, e.relatedTarget)) {
          sweep.style.setProperty("--sweep-origin", "left");
          gsap.to(sweep, { "--sweep": 1, duration: 0.35, overwrite: "auto" });
        }

        const press = closest(e.target, PRESS);
        const icon = press?.querySelector("svg");
        if (press && icon && !within(press, e.relatedTarget)) {
          gsap.fromTo(
            icon,
            { y: 0 },
            { y: -2, duration: 0.16, ease: "power2.out", yoyo: true, repeat: 1, overwrite: "auto" },
          );
        }
      };

      const out = (e: PointerEvent) => {
        if (!closest(e.target, HOVERABLE)) return;

        const lift = closest(e.target, LIFT);
        if (lift && !within(lift, e.relatedTarget)) {
          gsap.to(lift, { y: 0, duration: 0.45, overwrite: "auto" });
        }

        const nudge = closest(e.target, NUDGE);
        if (nudge && !within(nudge, e.relatedTarget)) {
          gsap.to(nudge, { x: 0, duration: 0.4, overwrite: "auto" });
        }

        const sweep = closest(e.target, SWEEP);
        if (sweep && !within(sweep, e.relatedTarget)) {
          sweep.style.setProperty("--sweep-origin", "right");
          gsap.to(sweep, { "--sweep": 0, duration: 0.35, overwrite: "auto" });
        }
      };

      document.addEventListener("pointerover", over);
      document.addEventListener("pointerout", out);
      return () => {
        document.removeEventListener("pointerover", over);
        document.removeEventListener("pointerout", out);
        clear(`${LIFT}, ${NUDGE}`, "transform");
        clear(SWEEP, "--sweep,--sweep-origin");
        clear(`${PRESS} svg`, "transform");
      };
    });

    return () => mm.revert();
  });

  return null;
}
