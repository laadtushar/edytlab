"use client";

/**
 * Flip: animate a layout change by recording where things were, letting
 * the DOM change, and tweening from the old boxes to the new ones. Used
 * by the toolbox filter, where groups appear, disappear and reflow.
 */

import { Flip } from "gsap/Flip";

import { gsap } from "./gsap";

gsap.registerPlugin(Flip);

export { Flip };
