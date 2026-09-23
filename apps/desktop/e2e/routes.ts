/**
 * The URL prefix `convertFileSrc` points at, shared by the page and the
 * server that answers it.
 *
 * A module of its own, with no imports, because both sides need it and
 * they cannot share anything heavier: the page is bundled for the
 * browser, and `vite.config.ts` imports Node's `fs`.
 */
export const FILE_ROUTE = "/__e2e_file__/";
