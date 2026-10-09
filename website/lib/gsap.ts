"use client";

/**
 * One place where GSAP is set up, so every animation on the site shares
 * the same easing, the same ScrollTrigger configuration, and — the part
 * that matters — the same answer to "should this move at all?".
 *
 * Plugins are registered here rather than at each call site because
 * registering twice is a no-op but *forgetting* is a silent failure:
 * `ScrollTrigger` simply never fires and the element sits at its
 * from-state, invisible. Importing this module is the guarantee.
 *
 * This file is client-only. GSAP touches `document` at import time, so
 * a server component that pulled it in would break the build.
 */

import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import { DrawSVGPlugin } from "gsap/DrawSVGPlugin";
import { useGSAP } from "@gsap/react";

// Every GSAP plugin has been free under the standard licence since 3.13,
// and they all ship inside the `gsap` package we already depend on —
// there is nothing extra to install and nothing to pay for, which is
// worth saying out loud because the internet is still full of posts
// explaining how to work around not having them.
//
// Only the light, site-wide ones are registered here: ScrollTrigger and
// DrawSVG (the line-drawn icons, the logo, every waveform stroke) are on
// every page. The heavier SVG and text plugins live in
// `gsap-svg.ts`, `gsap-text.ts` and `gsap-flip.ts`, each registered once
// in its own module, so a docs page does not download a morphing engine
// it never uses.
gsap.registerPlugin(ScrollTrigger, DrawSVGPlugin, useGSAP);

// The house curve. `power3.out` starts fast and settles, which reads as
// "the page is keeping up with you" rather than "the page is putting on
// a show". Everything overrides it deliberately or not at all.
gsap.defaults({ ease: "power3.out", duration: 0.7 });

/**
 * Where a reveal fires. Deliberately late — `85%` means the element is
 * already comfortably on screen before it animates, so a fast scroll
 * never leaves a reader looking at blank space waiting for text.
 */
export const REVEAL_START = "top 85%";

/**
 * Motion is opt-out, and the opt-out is the operating system's.
 *
 * `gsap.matchMedia` is used rather than a one-time media-query read
 * because the setting can change while the page is open, and because it
 * reverts every animation it created when the query stops matching —
 * which is what puts elements back at their natural position instead of
 * frozen halfway through a tween.
 *
 * Every animation on this site is written inside `motion()`, and every
 * element it touches is styled to look correct *without* it. Reduced
 * motion is then simply the absence of the tween, not a second code
 * path that can rot.
 */
export function motionOk(): gsap.MatchMedia {
  return gsap.matchMedia();
}

/** The query the whole site uses, spelled once. */
export const NO_PREFERENCE = "(prefers-reduced-motion: no-preference)";

/**
 * Motion that follows the pointer — a lean, a tilt, a lift on hover.
 *
 * Bound to a fine pointer as well as to the motion preference. On a
 * touchscreen there is no hover to lead with: the first "move" arrives
 * with the tap, and the thing being tapped would slide out from under
 * the finger pressing it.
 */
export const HOVER_MOTION = `${NO_PREFERENCE} and (hover: hover) and (pointer: fine)`;

/**
 * Run an endless animation only while `trigger` is on screen.
 *
 * A loop that keeps ticking below the fold costs a frame budget nobody
 * is looking at. Created inside a `matchMedia`/`useGSAP` context, the
 * trigger is reverted with everything else.
 */
export function whileVisible(anim: gsap.core.Animation, trigger: Element | null) {
  anim.pause();
  const st = ScrollTrigger.create({
    trigger,
    start: "top bottom",
    end: "bottom top",
    onToggle: (self) => (self.isActive ? anim.play() : anim.pause()),
  });
  if (st.isActive) anim.play();
  return st;
}

/** The parts of an SVG icon DrawSVG can draw. Circles cannot be. */
export const DRAWABLE = "path, line, polyline, polygon, rect, ellipse";

export { gsap, ScrollTrigger, DrawSVGPlugin, useGSAP };
