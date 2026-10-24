/**
 * Five more how-to posts of 2026-10-24, written for the questions people
 * type into a search box (a song's BPM and key, removing background noise,
 * true peak against sample peak, crossfading two songs, editing with a local
 * model).
 *
 * They follow the rules of `../blog-posts-audio/index.ts` and
 * `../blog-posts-october.ts`, with one addition learned from the first
 * four: where the tool does something its name does not say, or does less
 * than a reader would assume (a placeholder in a result, a ceiling that is
 * not the kind its parameter is named for, a defect), the post says so
 * where it first describes the tool, and links the issue.
 *
 * Type-only import: `../blog.ts` imports this file for its value.
 */

import type { BlogPost } from "../blog";
import { bpmAndKey } from "./bpm-and-key";
import { crossfadeTwoSongs } from "./crossfade";
import { localOllama } from "./local-ollama";
import { noiseReduction } from "./noise-reduction";
import { truePeak } from "./true-peak";

/** In the order they are listed on the blog index among posts of one day. */
export const audioHowtoPosts2: BlogPost[] = [bpmAndKey, noiseReduction, truePeak, crossfadeTwoSongs, localOllama];
