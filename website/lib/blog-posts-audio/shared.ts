/**
 * What the four audio how-to posts share: the day their sources were
 * read, and where the edytlab files they cite are read at.
 *
 * Every outside fact in these posts links the page it came from, and each
 * link in a post's "Sources" list says on which day it was read
 * (`blog-posts-audio.test.ts` holds both). A figure that has no primary
 * page is not given: the post says that none was found.
 */

/** The day every outside page in these posts was read. */
export const READ = "2026-10-22";

/**
 * The commit the edytlab source files and `docs/tools-reference.md` were
 * read at, so a link to one is a permalink to the lines that were read
 * and not to whatever `main` holds later.
 */
export const COMMIT = "b1678feb4a09dd697ae6703acf78a21b77a6d96a";

/** A file in this repository, at the commit the posts were written against. */
export const SRC = (path: string): string =>
  `https://github.com/laadtushar/edytlab/blob/${COMMIT}/${path}`;

/** One line of a "Sources" list: the page, then the day it was read. */
export const source = (label: string, url: string, note = ""): string =>
  `[${label}](${url})${note ? `, ${note}` : ""}, read ${READ}`;
