/**
 * What the five audio how-to posts of 2026-10-24 share: the day their
 * sources were read, and where the edytlab files they cite are read at.
 *
 * The same rules as the first four (`../blog-posts-audio/shared.ts`):
 * every outside fact in a post links the page it came from, and each link
 * in its "Sources" list says on which day it was read
 * (`../blog-posts-audio-2.test.ts` holds both). A figure that has no
 * primary page is not given: the post says that none was found.
 */

/** The day every outside page in these posts was read. */
export const READ = "2026-10-24";

/**
 * The commit the edytlab source files and `docs/tools-reference.md` were
 * read at (`origin/main` on the day), so a link to one is a permalink to
 * the lines that were read and not to whatever `main` holds later.
 */
export const COMMIT = "8bf752ff4c89f2722ee4c500783e1478c89aaa27";

/** A file in this repository, at the commit the posts were written against. */
export const SRC = (path: string): string =>
  `https://github.com/laadtushar/edytlab/blob/${COMMIT}/${path}`;

/** One line of a "Sources" list: the page, then the day it was read. */
export const source = (label: string, url: string, note = ""): string =>
  `[${label}](${url})${note ? `, ${note}` : ""}, read ${READ}`;

/**
 * The day two of the posts (the BPM and key post and the noise reduction
 * post) were brought up to date for v0.9.0, and the day their edytlab
 * sources were read again.
 */
export const UPDATED = "2026-10-25";

/**
 * `origin/main` on the update day: it holds the tempo, beat grid and key
 * rework (#705) and the noise reduction edge fix (#697), which the two
 * updated posts describe. The other three posts keep `COMMIT`: the files
 * they cite are unchanged in what they say.
 */
export const COMMIT_V090 = "aa9f6cd00e0a0203d27e34dccd42de974848f4ba";

/** A file in this repository, at `COMMIT_V090`. */
export const SRC_V090 = (path: string): string =>
  `https://github.com/laadtushar/edytlab/blob/${COMMIT_V090}/${path}`;

/** Like `source`, for a page or file read on the update day. */
export const sourceUpdated = (label: string, url: string, note = ""): string =>
  `[${label}](${url})${note ? `, ${note}` : ""}, read ${UPDATED}`;
