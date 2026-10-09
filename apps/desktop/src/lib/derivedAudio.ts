/**
 * Audio the app wrote, as opposed to audio the user brought (#416).
 *
 * A destructive edit — reverse, stretch, normalise, a fade baked in, a
 * mixdown — writes its result to `<project>/.audiograph/derived/` under
 * a content-addressed name, the hash of its samples
 * (`tools::provenance`), and points the clip at it. A track cut into
 * several clips is drawn from a file flattened into the same directory
 * (`tools::lane_audio_path`). Either way the name is 64 hex digits and
 * means nothing to a person, so anything that names audio on screen
 * asks this first and says something they know instead.
 */

/** The store's directory in a project: `session::STORE_DIR`. */
const STORE_DIR = ".audiograph";
/** Derived audio's directory inside it: `session::relocate::DERIVED_DIR`. */
const DERIVED_DIR = "derived";

/**
 * Whether `path` is a file inside a project's `.audiograph/derived/`.
 *
 * Matched on whole path segments — `.audiograph` immediately followed by
 * `derived` — so a folder of the user's that happens to be called
 * `derived` is not mistaken for it. Either separator is accepted: the
 * app ships on Windows, where the backend's paths use `\`.
 */
export function isDerivedAudioPath(path: string): boolean {
  const segments = path.split(/[\\/]+/);
  // The file itself; a path ending at the directory names no file.
  if (!segments.pop()) return false;
  return segments.some(
    (segment, i) => segment === STORE_DIR && segments[i + 1] === DERIVED_DIR,
  );
}
