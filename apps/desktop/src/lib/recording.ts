/**
 * Recording outcomes, as values rather than side effects (#248).
 *
 * These lived inline in `App.tsx` as `try { … } catch (e) {
 * console.error(…) }` — the only two handlers in the file that reported
 * to the console instead of the error banner, which is the app's single
 * error surface. In a packaged desktop build there is no console to
 * read, so a failed Record was indistinguishable from a dead button.
 *
 * They are here so the outcomes can be asserted: `App.tsx` mounts a
 * Tauri surface a unit test cannot render (#273), and these functions
 * are the part worth testing — the mapping from a bridge failure to
 * what the user is told.
 */

/** What happened when the user pressed Record. */
export type StartOutcome =
  | { kind: "recording" }
  | { kind: "failed"; message: string };

/**
 * What happened when the user pressed Stop.
 *
 * `saveFailed` and `loadFailed` are deliberately distinct. If the WAV
 * was written but importing it failed, the take is **on disk** — naming
 * the path is far more useful than calling it lost. Only `saveFailed`
 * means the audio is actually gone.
 */
export type StopOutcome =
  | { kind: "loaded"; path: string; nodeId: string | null }
  | { kind: "saveFailed"; message: string }
  | { kind: "loadFailed"; path: string; message: string };

export async function startTake(start: () => Promise<unknown>): Promise<StartOutcome> {
  try {
    await start();
    return { kind: "recording" };
  } catch (e) {
    // No input device, permission denied, device busy — all reachable
    // from `recorder`, and all previously silent.
    return { kind: "failed", message: `Could not start recording: ${String(e)}` };
  }
}

export async function stopTake(
  stop: () => Promise<{ path: string }>,
  load: (paths: string[]) => Promise<{ last_node_id: string | null }>,
): Promise<StopOutcome> {
  let saved: { path: string };
  try {
    saved = await stop();
  } catch (e) {
    return {
      kind: "saveFailed",
      message: `Recording stopped but could not be saved — the take was lost: ${String(e)}`,
    };
  }

  try {
    const { last_node_id } = await load([saved.path]);
    return { kind: "loaded", path: saved.path, nodeId: last_node_id };
  } catch (e) {
    return {
      kind: "loadFailed",
      path: saved.path,
      message:
        `Recording saved to ${saved.path}, but could not be added to the ` +
        `session: ${String(e)}`,
    };
  }
}
