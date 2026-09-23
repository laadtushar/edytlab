/**
 * App code runs in a webview, which has no Node — so Node's globals must
 * not type-check here.
 *
 * They did: `@types/node` was included implicitly, so `process.cwd()` in
 * `src/` compiled cleanly and threw at runtime (#336). Each line below
 * is expected to be a type error. If Node's types leak back into the
 * app program, the expectation goes unused and `tsc` fails, naming this
 * file.
 *
 * Type-only, so it compiles to nothing and nothing imports it; `tsc`
 * checks it because it is under `src/`.
 */

// @ts-expect-error `process` is Node's, and the webview has none.
export type NodeProcess = typeof process;

// @ts-expect-error `Buffer` is Node's, and the webview has none.
export type NodeBuffer = typeof Buffer;
