"use client";

/**
 * ScrambleText: a label that resolves out of noise, the way a readout
 * settles. Used on short, decorative-weight text — section eyebrows and
 * stat units — never on a sentence someone is trying to read.
 *
 * The final text is always the text in the markup, and reverting the
 * tween puts it back, so a screen reader or a reader with reduced
 * motion only ever gets the real words.
 */

import { ScrambleTextPlugin } from "gsap/ScrambleTextPlugin";

import { gsap } from "./gsap";

gsap.registerPlugin(ScrambleTextPlugin);

export { ScrambleTextPlugin };
