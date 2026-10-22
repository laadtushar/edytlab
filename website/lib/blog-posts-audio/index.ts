/**
 * Four how-to posts of 2026-10-22, written for the questions people type
 * into a search box (loudness targets, batch normalizing, removing
 * silence, changing tempo without changing pitch).
 *
 * They follow the rules at the top of `../blog-posts-october.ts`, and
 * three more:
 *
 * - What a platform or a standard asks for is stated only from that
 *   platform's or standard's own page, linked, and dated by the "Sources"
 *   list at the end of the post. Where no such page was found the post
 *   says so and gives no figure.
 * - What edytlab does is stated only from `docs/tools-reference.md` and
 *   the tool sources, at the commit in `shared.ts`. A run that was made
 *   to check a statement says what it was run on: synthetic audio, with
 *   the tools called directly. None of these is described as tested.
 * - Nothing here is a ranking: no "best", no count, no comparison that a
 *   reader cannot check on the other product's own page.
 *
 * Type-only import: `../blog.ts` imports this file for its value.
 */

import type { BlogPost } from "../blog";
import { batchNormalize } from "./batch-normalize";
import { lufsTargets } from "./lufs-targets";
import { removeSilence } from "./remove-silence";
import { tempoWithoutPitch } from "./tempo-without-pitch";

/** In the order they are listed on the blog index among posts of one day. */
export const audioHowtoPosts: BlogPost[] = [lufsTargets, batchNormalize, removeSilence, tempoWithoutPitch];
