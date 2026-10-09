"use client";

/**
 * The SVG plugins only the home page needs.
 *
 * - MorphSVG reshapes one path into another — the waveform before and
 *   after an edit, which is the whole of what "the silence came out and
 *   the gap closed" looks like.
 * - MotionPath sends a dot along a path — a request travelling round the
 *   agent loop.
 *
 * Registered here, once, rather than in `gsap.ts`, so the pages that do
 * not draw these (docs, blog, the legal pages) do not ship them.
 */

import { MorphSVGPlugin } from "gsap/MorphSVGPlugin";
import { MotionPathPlugin } from "gsap/MotionPathPlugin";

import { gsap } from "./gsap";

gsap.registerPlugin(MorphSVGPlugin, MotionPathPlugin);

export { MorphSVGPlugin, MotionPathPlugin };
